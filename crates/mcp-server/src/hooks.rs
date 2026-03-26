//! Claude Code HTTP hook handlers.
//!
//! Claude Code v2.1.63+ supports HTTP hooks — `type: "http"` hooks POST JSON
//! to a URL and receive JSON responses. OCO exposes hook endpoints so that
//! Claude Code events flow into the orchestration state in real time.
//!
//! ## Supported hook events
//!
//! | Endpoint               | Claude Code event    | OCO action                          |
//! |------------------------|---------------------|--------------------------------------|
//! | `/hooks/post-tool`     | `PostToolUse`       | Record observation in telemetry      |
//! | `/hooks/task-completed` | `TaskCompleted`    | Update SharedTaskList step status    |
//! | `/hooks/file-changed`  | `FileChanged`       | Trigger incremental re-index         |
//! | `/hooks/post-compact`  | `PostCompact`       | Re-inject critical context           |
//! | `/hooks/stop`          | `Stop`              | Mark session as terminated           |

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::server::AppState;

// ---------------------------------------------------------------------------
// Hook payload types (Claude Code → OCO)
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
// PostToolUse — record tool observations in telemetry
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PostToolUseData {
    /// Tool name that was used.
    #[serde(default)]
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
    let data: PostToolUseData = match serde_json::from_value(payload.data) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "invalid PostToolUse payload");
            return (
                StatusCode::BAD_REQUEST,
                Json(HookResponse::error(e.to_string())),
            );
        }
    };

    debug!(
        tool = %data.tool_name,
        success = data.success,
        session = ?payload.session_id,
        "hook: post-tool-use"
    );

    // Record in active session telemetry if we can match the session
    if let Some(ref session_id) = payload.session_id {
        let _ = state
            .session_manager
            .record_hook_event(session_id, "post_tool_use", &data.tool_name)
            .await;
    }

    (StatusCode::OK, Json(HookResponse::ok()))
}

// ---------------------------------------------------------------------------
// TaskCompleted — sync with OCO's session tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TaskCompletedData {
    /// Task identifier from Claude Code.
    #[serde(default)]
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
    let data: TaskCompletedData = match serde_json::from_value(payload.data) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "invalid TaskCompleted payload");
            return (
                StatusCode::BAD_REQUEST,
                Json(HookResponse::error(e.to_string())),
            );
        }
    };

    info!(
        task_id = %data.task_id,
        success = data.success,
        session = ?payload.session_id,
        "hook: task-completed"
    );

    if let Some(ref session_id) = payload.session_id {
        let _ = state
            .session_manager
            .record_hook_event(session_id, "task_completed", &data.task_id)
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
    let data: FileChangedData = match serde_json::from_value(payload.data) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "invalid FileChanged payload");
            return (
                StatusCode::BAD_REQUEST,
                Json(HookResponse::error(e.to_string())),
            );
        }
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
pub async fn hook_post_compact(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    info!(
        session = ?payload.session_id,
        "hook: post-compact — context compaction detected"
    );

    // TODO(#45): re-inject WorkingMemory and current plan into context
    // This requires the session to have a plan reference.

    (
        StatusCode::OK,
        Json(HookResponse::ok_with_message(
            "compaction acknowledged, context re-injection pending",
        )),
    )
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
    let data: StopData = match serde_json::from_value(payload.data) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "invalid Stop payload");
            return (
                StatusCode::BAD_REQUEST,
                Json(HookResponse::error(e.to_string())),
            );
        }
    };

    info!(
        reason = ?data.reason,
        session = ?payload.session_id,
        "hook: stop — session terminated"
    );

    if let Some(ref session_id) = payload.session_id {
        let _ = state.session_manager.stop_session(session_id).await;
    }

    (StatusCode::OK, Json(HookResponse::ok()))
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
