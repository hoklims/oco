//! Claude Code HTTP hook handlers.
//!
//! Claude Code v2.1.63+ supports HTTP hooks — `type: "http"` hooks POST JSON
//! to a URL and receive JSON responses. OCO exposes hook endpoints so that
//! Claude Code events flow into the orchestration state in real time.
//!
//! ## Unified endpoint (recommended)
//!
//! `POST /hooks/event` — accepts any [`ClaudeHookEvent`] via the
//! `oco-claude-adapter` crate. The adapter deserializes all 24 Claude Code
//! event types, maps them to `OrchestrationEvent` when applicable, and
//! dispatches side effects (re-index, session stop, etc.) automatically.
//! Returns a [`HookDecision`].
//!
//! ## Per-event endpoints
//!
//! | Endpoint                | Claude Code event   | OCO action                          |
//! |-------------------------|---------------------|--------------------------------------|
//! | `/hooks/post-tool`      | `PostToolUse`       | Record observation in telemetry      |
//! | `/hooks/task-completed` | `TaskCompleted`     | Update SharedTaskList step status    |
//! | `/hooks/file-changed`   | `FileChanged`       | Trigger incremental re-index         |
//! | `/hooks/post-compact`   | `PostCompact`       | Re-inject critical context           |
//! | `/hooks/stop`           | `Stop`              | Mark session as terminated           |
//! | `/hooks/session-start`  | `SessionStart`      | Log session init + capabilities      |
//! | `/hooks/teammate-idle`  | `TeammateIdle`      | Acknowledge idle teammate            |
//! | `/hooks/subagent-stop`  | `SubagentStop`      | Record subagent lifecycle event      |
//! | `/hooks/pre-compact`    | `PreCompact`        | Snapshot working memory before compact |
//! | `/hooks/config-change`  | `ConfigChange`      | Flag capability refresh needed       |
//! | `/hooks/elicitation`    | `Elicitation`       | Route to decision engine             |
//!
//! The original 5 endpoints remain fully supported. New integrations should
//! prefer `/hooks/event` which handles all event types through a single route.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware,
    response::IntoResponse,
};
use oco_claude_adapter::{ClaudeHookEvent, HookDecision};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Hook infrastructure constants
// ---------------------------------------------------------------------------

/// Maximum hook request body size (bytes).
const HOOK_BODY_LIMIT: usize = 64 * 1024;

/// Handler timeout — protects against lock contention or slow I/O.
const HOOK_TIMEOUT: Duration = Duration::from_secs(5);

/// Global rate limit for hook endpoints (requests per second).
const HOOK_RATE_LIMIT_RPS: u64 = 100;

use oco_claude_adapter::IntegrationMode;

use crate::server::AppState;

/// Middleware that returns 204 No Content when running in EnterpriseSafe mode.
///
/// In EnterpriseSafe mode, managed settings block user/plugin hooks, so
/// hook endpoints become graceful no-ops. This avoids confusing error logs
/// when Claude Code can't reach hooks it never called.
pub async fn enterprise_safe_middleware(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: middleware::Next,
) -> impl IntoResponse {
    if state.claude_capabilities.recommended_mode() == IntegrationMode::EnterpriseSafe {
        debug!("EnterpriseSafe mode: hook endpoint returning 204 noop");
        return StatusCode::NO_CONTENT.into_response();
    }
    next.run(request).await.into_response()
}

// ---------------------------------------------------------------------------
// Auth middleware for hook routes
// ---------------------------------------------------------------------------

/// Middleware that validates `Authorization: Bearer <secret>` on hook routes.
///
/// If `AppState::hook_secret` is `None`, auth is skipped (dev mode).
pub async fn hook_auth_middleware(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: middleware::Next,
) -> impl IntoResponse {
    if let Some(ref expected) = state.hook_secret {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        match provided {
            Some(token) if token == expected.as_str() => {}
            _ => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(HookResponse::error("unauthorized")),
                )
                    .into_response();
            }
        }
    }

    next.run(request).await.into_response()
}

/// Build the hook sub-router with auth, body limit, timeout, and rate limit.
///
/// Applied only to `/api/v1/hooks/*` routes.
///
/// Layer order (outermost → innermost):
/// 1. Auth — rejects unauthenticated requests immediately
/// 2. Rate limit — sheds excess load before parsing body
/// 3. Body limit — rejects oversized payloads
/// 4. Timeout — bounds handler execution time
pub fn hook_router(state: Arc<AppState>) -> axum::Router<Arc<AppState>> {
    use axum::routing::post;

    let rate_limiter = Arc::new(RateLimiter::new(HOOK_RATE_LIMIT_RPS));

    axum::Router::new()
        .route("/event", post(hook_unified))
        .route("/post-tool", post(hook_post_tool))
        .route("/task-completed", post(hook_task_completed))
        .route("/file-changed", post(hook_file_changed))
        .route("/post-compact", post(hook_post_compact))
        .route("/stop", post(hook_stop))
        .route("/session-start", post(hook_session_start))
        .route("/teammate-idle", post(hook_teammate_idle))
        .route("/subagent-stop", post(hook_subagent_stop))
        .route("/pre-compact", post(hook_pre_compact))
        .route("/config-change", post(hook_config_change))
        .route("/elicitation", post(hook_elicitation))
        .route("/{event}", post(hook_catchall))
        // Body limit (64 KB)
        .layer(RequestBodyLimitLayer::new(HOOK_BODY_LIMIT))
        // Rate limit, timeout, and custom error responses for 413
        .layer(middleware::from_fn(move |req, next: middleware::Next| {
            let limiter = Arc::clone(&rate_limiter);
            async move {
                // Rate limit check
                if !limiter.check() {
                    warn!("hook rate limit exceeded");
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(serde_json::json!({
                            "error": "rate limit exceeded",
                            "max_rps": HOOK_RATE_LIMIT_RPS,
                        })),
                    )
                        .into_response();
                }

                // Timeout: bound handler execution, then check for 413
                match tokio::time::timeout(HOOK_TIMEOUT, next.run(req)).await {
                    Ok(response) => {
                        let response = response.into_response();
                        // Convert bare 413 to JSON response
                        if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
                            return (
                                StatusCode::PAYLOAD_TOO_LARGE,
                                Json(serde_json::json!({
                                    "error": "payload too large",
                                    "max_bytes": HOOK_BODY_LIMIT,
                                })),
                            )
                                .into_response();
                        }
                        response
                    }
                    Err(_elapsed) => {
                        warn!(
                            timeout_secs = HOOK_TIMEOUT.as_secs(),
                            "hook handler timed out"
                        );
                        (
                            StatusCode::REQUEST_TIMEOUT,
                            Json(serde_json::json!({
                                "error": "handler timed out",
                                "timeout_secs": HOOK_TIMEOUT.as_secs(),
                            })),
                        )
                            .into_response()
                    }
                }
            }
        }))
        // EnterpriseSafe: graceful noop when managed settings restrict hooks
        .layer(middleware::from_fn_with_state(
            state.clone(),
            enterprise_safe_middleware,
        ))
        // Outermost: auth
        .layer(middleware::from_fn_with_state(state, hook_auth_middleware))
}

// ---------------------------------------------------------------------------
// Simple rate limiter (atomic counter, 1-second window)
// ---------------------------------------------------------------------------

/// Global rate limiter with a sliding 1-second window.
/// Uses atomic operations — no locks, no allocations.
pub(crate) struct RateLimiter {
    max_per_second: u64,
    count: std::sync::atomic::AtomicU64,
    pub(crate) window_start: std::sync::atomic::AtomicU64,
}

impl RateLimiter {
    pub(crate) fn new(max_per_second: u64) -> Self {
        Self {
            max_per_second,
            count: std::sync::atomic::AtomicU64::new(0),
            window_start: std::sync::atomic::AtomicU64::new(Self::now_secs()),
        }
    }

    /// Returns `true` if the request is within the rate limit.
    pub(crate) fn check(&self) -> bool {
        use std::sync::atomic::Ordering;

        let now = Self::now_secs();
        let window = self.window_start.load(Ordering::Acquire);

        if now != window {
            // CAS: only one thread wins the window reset
            if self
                .window_start
                .compare_exchange(window, now, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                self.count.store(1, Ordering::Release);
                return true;
            }
            // Lost the race — fall through to normal increment
        }

        let prev = self.count.fetch_add(1, Ordering::Relaxed);
        prev < self.max_per_second
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

// ---------------------------------------------------------------------------
// Hook payload types (Claude Code -> OCO)
// ---------------------------------------------------------------------------

/// Common envelope for all Claude Code HTTP hook payloads.
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    /// The hook event name (e.g., "PostToolUse", "FileChanged").
    pub event: String,
    /// Session identifier from Claude Code.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Timestamp in ISO 8601 format.
    #[serde(default)]
    pub timestamp: Option<String>,
    /// Event-specific data.
    #[serde(default)]
    pub data: serde_json::Value,
}

/// Response from OCO back to Claude Code.
///
/// Claude Code HTTP hooks expect JSON responses. An empty `{}` means "continue".
/// Setting `block: true` with `exit_code: 2` tells Claude Code to block the action.
#[derive(Debug, Serialize)]
pub struct HookResponse {
    /// Whether the hook processed successfully.
    pub ok: bool,
    /// Optional message for Claude Code logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// If true, block the action (requires exit_code = 2 in Claude Code hook config).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<bool>,
}

impl HookResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            message: None,
            block: None,
        }
    }

    pub fn ok_with_message(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: Some(msg.into()),
            block: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: Some(msg.into()),
            block: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate that the payload `event` field matches the expected event name.
fn validate_event(
    payload: &HookPayload,
    expected: &str,
) -> Result<(), (StatusCode, Json<HookResponse>)> {
    if payload.event != expected {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(HookResponse::error(format!(
                "event mismatch: expected \"{expected}\", got \"{}\"",
                payload.event
            ))),
        ));
    }
    Ok(())
}

/// Resolve the payload session_id to an OCO session ID.
///
/// The hook payload carries a Claude Code session ID which may differ from the
/// OCO internal UUID. This helper tries direct lookup first, then reverse
/// lookup by external_session_id.
async fn resolve_oco_session_id(
    state: &AppState,
    payload_session_id: Option<&str>,
) -> Option<String> {
    let sid = payload_session_id?;
    state.session_manager.resolve_session_id(sid).await
}

/// Validate the event name and deserialize the payload `data` field in one step.
///
/// Combines `validate_event` + `serde_json::from_value` with uniform error
/// handling, eliminating the repeated match block in each handler.
fn extract_hook_data<T: serde::de::DeserializeOwned>(
    payload: HookPayload,
    expected: &str,
) -> Result<(T, HookPayload), (StatusCode, Json<HookResponse>)> {
    if payload.event != expected {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(HookResponse::error(format!(
                "event mismatch: expected \"{expected}\", got \"{}\"",
                payload.event
            ))),
        ));
    }
    let data: T = serde_json::from_value(payload.data.clone()).map_err(|e| {
        warn!(error = %e, "invalid {expected} data payload");
        (
            StatusCode::BAD_REQUEST,
            Json(HookResponse::error("invalid payload")),
        )
    })?;
    Ok((data, payload))
}

// ---------------------------------------------------------------------------
// PostToolUse — record tool observations in telemetry
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PostToolUseData {
    /// Tool name that was used (required).
    pub tool_name: String,
    /// Whether the tool call succeeded.
    #[serde(default)]
    pub success: bool,
    /// Duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Truncated output snippet.
    #[serde(default)]
    pub output_preview: Option<String>,
}

/// `POST /api/v1/hooks/post-tool` — called after each tool use in Claude Code.
pub async fn hook_post_tool(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, payload): (PostToolUseData, _) = match extract_hook_data(payload, "PostToolUse") {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    debug!(
        tool = %data.tool_name,
        success = data.success,
        session = ?payload.session_id,
        "hook: post-tool-use"
    );

    // Record in active session telemetry if we can match the session
    if let Some(oco_sid) = resolve_oco_session_id(&state, payload.session_id.as_deref()).await {
        state
            .session_manager
            .record_hook_event(&oco_sid, "PostToolUse", &data.tool_name)
            .await;
    }

    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// TaskCompleted — sync with OCO's session tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TaskCompletedData {
    /// Task identifier from Claude Code (required).
    pub task_id: String,
    /// Whether the task succeeded.
    #[serde(default)]
    pub success: bool,
    /// Task output summary.
    #[serde(default)]
    pub output: Option<String>,
}

/// `POST /api/v1/hooks/task-completed` — called when a Claude Code task finishes.
pub async fn hook_task_completed(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, payload): (TaskCompletedData, _) = match extract_hook_data(payload, "TaskCompleted")
    {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    info!(
        task_id = %data.task_id,
        success = data.success,
        session = ?payload.session_id,
        "hook: task-completed"
    );

    if let Some(oco_sid) = resolve_oco_session_id(&state, payload.session_id.as_deref()).await {
        state
            .session_manager
            .record_hook_event(&oco_sid, "TaskCompleted", &data.task_id)
            .await;
    }

    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// FileChanged — trigger incremental re-index
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct FileChangedData {
    /// Paths that changed.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Type of change: "created", "modified", "deleted".
    #[serde(default)]
    pub change_type: Option<String>,
}

/// `POST /api/v1/hooks/file-changed` — called when files change in the workspace.
pub async fn hook_file_changed(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, _payload): (FileChangedData, _) = match extract_hook_data(payload, "FileChanged") {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    debug!(
        paths = ?data.paths,
        change_type = ?data.change_type,
        "hook: file-changed — incremental re-index queued"
    );

    // TODO(#45): trigger incremental re-index via code-intel
    // For now, just acknowledge the event.

    (
        StatusCode::OK,
        Json(HookResponse::ok_with_message(format!(
            "acknowledged {} file change(s)",
            data.paths.len()
        ))),
    )
}

// ---------------------------------------------------------------------------
// PostCompact — re-inject critical context after compaction
// ---------------------------------------------------------------------------

/// `POST /api/v1/hooks/post-compact` — called after Claude Code compacts context.
///
/// Re-injects a compact snapshot of the session's working memory so that
/// verified facts, active hypotheses, plan state, and open questions survive
/// context compaction.
pub async fn hook_post_compact(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    if let Err(resp) = validate_event(&payload, "PostCompact") {
        return resp;
    }

    info!(
        session = ?payload.session_id,
        "hook: post-compact — context compaction detected"
    );

    // Try to retrieve a compact snapshot from the active session
    let snapshot = match resolve_oco_session_id(&state, payload.session_id.as_deref()).await {
        Some(oco_sid) => state.session_manager.get_compact_snapshot(&oco_sid).await,
        None => None,
    };

    match snapshot {
        Some(snap) => {
            let message = format!(
                "OCO context to preserve after compact:\n{}",
                serde_json::to_string_pretty(&snap).unwrap_or_default()
            );
            debug!(
                session = ?payload.session_id,
                snapshot_keys = ?snap.as_object().map(|o| o.keys().collect::<Vec<_>>()),
                "post-compact: re-injecting working memory snapshot"
            );
            (StatusCode::OK, Json(HookResponse::ok_with_message(message)))
        }
        None => (StatusCode::OK, Json(HookResponse::ok())),
    }
}

// ---------------------------------------------------------------------------
// Stop — mark session as terminated
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StopData {
    /// Reason the session stopped.
    #[serde(default)]
    pub reason: Option<String>,
    /// Last assistant message (available in Claude Code v2.1.50+).
    #[serde(default)]
    pub last_message: Option<String>,
}

/// `POST /api/v1/hooks/stop` — called when Claude Code session stops.
pub async fn hook_stop(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, payload): (StopData, _) = match extract_hook_data(payload, "Stop") {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    info!(
        reason = ?data.reason,
        session = ?payload.session_id,
        "hook: stop — session terminated"
    );

    if let Some(oco_sid) = resolve_oco_session_id(&state, payload.session_id.as_deref()).await
        && let Err(e) = state.session_manager.stop_session(&oco_sid).await
    {
        warn!(session_id = %oco_sid, error = %e, "failed to stop session from hook");
    }

    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// SessionStart — log session init and capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SessionStartData {
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/v1/hooks/session-start` — called when a Claude Code session starts.
pub async fn hook_session_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, payload): (SessionStartData, _) = match extract_hook_data(payload, "SessionStart") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    info!(reason = ?data.reason, session = ?payload.session_id, "hook: session-start");
    // Log capabilities from state
    debug!(mode = %state.claude_capabilities.recommended_mode(), "session capabilities");
    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// TeammateIdle — acknowledge idle teammate
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TeammateIdleData {
    #[serde(default)]
    pub teammate_name: Option<String>,
}

/// `POST /api/v1/hooks/teammate-idle` — called when a teammate has no work assigned.
pub async fn hook_teammate_idle(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, _payload): (TeammateIdleData, _) = match extract_hook_data(payload, "TeammateIdle") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    debug!(teammate = ?data.teammate_name, "hook: teammate-idle — no work assigned");
    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// SubagentStop — record subagent lifecycle event
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SubagentStopData {
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub success: bool,
}

/// `POST /api/v1/hooks/subagent-stop` — called when a subagent finishes.
pub async fn hook_subagent_stop(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, payload): (SubagentStopData, _) = match extract_hook_data(payload, "SubagentStop") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    info!(agent = ?data.agent_name, success = data.success, session = ?payload.session_id, "hook: subagent-stop");
    if let Some(oco_sid) = resolve_oco_session_id(&state, payload.session_id.as_deref()).await {
        state
            .session_manager
            .record_hook_event(
                &oco_sid,
                "SubagentStop",
                data.agent_name.as_deref().unwrap_or("unknown"),
            )
            .await;
    }
    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// PreCompact — snapshot working memory before compaction
// ---------------------------------------------------------------------------

/// `POST /api/v1/hooks/pre-compact` — called before Claude Code compacts context.
///
/// Provides a hook point to snapshot critical working memory entries to a
/// persistent store before they are lost during compaction.
pub async fn hook_pre_compact(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    if let Err(resp) = validate_event(&payload, "PreCompact") {
        return resp;
    }
    info!(session = ?payload.session_id, "hook: pre-compact — snapshotting working memory");
    // TODO: Snapshot WorkingMemory critical entries to persistent store
    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// ConfigChange — flag capability refresh needed
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ConfigChangeData {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

/// `POST /api/v1/hooks/config-change` — called when Claude Code config changes.
pub async fn hook_config_change(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, _payload): (ConfigChangeData, _) = match extract_hook_data(payload, "ConfigChange") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    info!(key = ?data.key, "hook: config-change — capabilities may need refresh");
    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// Elicitation — route to decision engine
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ElicitationData {
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub request: Option<serde_json::Value>,
}

/// `POST /api/v1/hooks/elicitation` — called when an elicitation dialog is triggered.
pub async fn hook_elicitation(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (data, _payload): (ElicitationData, _) = match extract_hook_data(payload, "Elicitation") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    debug!(server = ?data.server, "hook: elicitation — routing to decision engine");
    // TODO: Route to ElicitationRequest handler
    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// Unified event handler (via oco-claude-adapter)
// ---------------------------------------------------------------------------

/// `POST /api/v1/hooks/event` — unified handler for all Claude Code hook events.
///
/// Accepts any [`ClaudeHookEvent`] (all 24 documented event types). Maps the
/// event to an [`OrchestrationEvent`] when applicable, logs it, and dispatches
/// side effects (re-index for `FileChanged`, session stop for `Stop`, etc.).
///
/// Returns a [`HookDecision`] (default: allow).
///
/// This is the recommended endpoint for new integrations. The legacy per-event
/// endpoints (`/post-tool`, `/file-changed`, etc.) remain for backward compat.
pub async fn hook_unified(
    State(_state): State<Arc<AppState>>,
    Json(event): Json<ClaudeHookEvent>,
) -> impl IntoResponse {
    let event_name = event.event_name();

    // Map to OrchestrationEvent and log if applicable.
    if let Some(oco_event) = event.to_orchestration_event() {
        info!(
            event = %event_name,
            ?oco_event,
            "hook/event: mapped to OrchestrationEvent"
        );
    } else {
        debug!(event = %event_name, "hook/event: side-effect only (no OrchestrationEvent)");
    }

    // Dispatch side effects based on event variant.
    match &event {
        ClaudeHookEvent::FileChanged { path, change_type } => {
            debug!(
                path = %path,
                change_type = %change_type,
                "hook/event: file-changed — incremental re-index queued"
            );
            // TODO(#45): trigger incremental re-index via code-intel
        }

        ClaudeHookEvent::Stop { reason } => {
            info!(reason = %reason, "hook/event: stop — session terminated");
            // No session_id available on ClaudeHookEvent; session teardown
            // is best-effort. The legacy /stop endpoint handles session
            // resolution via HookPayload.session_id.
        }

        ClaudeHookEvent::PostToolUse {
            tool_name, success, ..
        } => {
            debug!(
                tool = %tool_name,
                success = %success,
                "hook/event: post-tool-use recorded"
            );
        }

        ClaudeHookEvent::TaskCompleted {
            task_id, success, ..
        } => {
            info!(
                task_id = %task_id,
                success = %success,
                "hook/event: task-completed"
            );
        }

        ClaudeHookEvent::PostCompact { compact_summary } => {
            info!(
                summary_len = compact_summary.len(),
                "hook/event: post-compact — context compaction detected"
            );
        }

        ClaudeHookEvent::SessionEnd {} => {
            info!("hook/event: session-end");
        }

        // All other events: acknowledged, no extra side effects.
        _ => {}
    }

    (StatusCode::OK, Json(HookDecision::allow()))
}

// ---------------------------------------------------------------------------
// Generic catch-all for unknown hook events
// ---------------------------------------------------------------------------

/// `POST /api/v1/hooks/{event}` — catch-all for unhandled hook events.
pub async fn hook_catchall(
    axum::extract::Path(event): axum::extract::Path<String>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    debug!(
        event = %event,
        payload_event = %payload.event,
        "hook: unhandled event — acknowledged"
    );

    (StatusCode::OK, Json(HookResponse::ok()))
}
