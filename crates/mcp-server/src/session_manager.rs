//! Persistent session manager with real orchestration support.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use tokio::sync::{Mutex, broadcast, watch};
use tracing::{error, info};

use oco_orchestrator_core::graph_runner::CancellationToken;
use oco_orchestrator_core::llm::{LlmProvider, StubLlmProvider};
use oco_orchestrator_core::state::OrchestrationState;
use oco_orchestrator_core::{OrchestrationLoop, OrchestratorConfig};
use oco_shared_types::dashboard::{DashboardEvent, EventStream};
use oco_shared_types::{DecisionTrace, SessionId, SessionStatus};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Status broadcast via the watch channel.
#[derive(Debug, Clone, Serialize)]
pub struct SessionStatusUpdate {
    pub status: String,
    pub steps: u32,
    pub tokens_used: u64,
}

/// Serializable snapshot of a session, returned by the API.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub status: String,
    pub steps: u32,
    pub tokens_used: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub complexity: String,
    pub user_request: String,
    /// External session ID for correlation (e.g. Claude Code session).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_session_id: Option<String>,
    /// Number of hook events received during this session.
    pub hook_events_count: u32,
}

/// A recorded hook event — kept in memory for debug/observability.
#[derive(Debug, Clone, Serialize)]
pub struct HookEvent {
    pub timestamp: DateTime<Utc>,
    /// Hook event name (e.g. "PostToolUse", "TaskCompleted", "Stop").
    pub hook_name: String,
    /// Event-specific detail (tool_name, task_id, file paths, reason…).
    pub detail: String,
    /// Claude Code session_id from the hook payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Whether the event was recorded against an active session.
    pub recorded: bool,
}

/// Maximum hook events retained per session.
const MAX_HOOK_EVENTS: usize = 1000;

/// Broadcast channel capacity for live dashboard events.
const LIVE_BROADCAST_CAPACITY: usize = 256;

/// Internal state held per session.
pub struct SessionState {
    pub orchestration_state: Option<OrchestrationState>,
    pub status: SessionStatus,
    pub status_tx: watch::Sender<SessionStatusUpdate>,
    pub status_rx: watch::Receiver<SessionStatusUpdate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub user_request: String,
    /// Shared cancellation token — signaled on stop to interrupt the orchestration loop.
    pub cancel: CancellationToken,
    /// External session ID for correlation (e.g. Claude Code session).
    pub external_session_id: Option<String>,
    /// Hook events received during this session (bounded, append-only).
    pub hook_events: Vec<HookEvent>,
    /// Broadcast channel for live dashboard events (SSE consumers subscribe here).
    pub dashboard_tx: broadcast::Sender<DashboardEvent>,
    /// Accumulated dashboard events for catch-up on late subscribers.
    pub dashboard_events: Vec<DashboardEvent>,
}

// ---------------------------------------------------------------------------
// SessionManager
// ---------------------------------------------------------------------------

/// Manages active orchestration sessions.
pub struct SessionManager {
    sessions: DashMap<String, Arc<Mutex<SessionState>>>,
    config: OrchestratorConfig,
    llm: Arc<dyn LlmProvider>,
}

impl SessionManager {
    /// Create a new manager.
    ///
    /// If no LLM provider is supplied (i.e. the caller passes `None`), a
    /// [`StubLlmProvider`] is used as a development fallback.
    pub fn new(config: OrchestratorConfig, llm: Option<Arc<dyn LlmProvider>>) -> Self {
        let llm = llm.unwrap_or_else(|| {
            Arc::new(StubLlmProvider {
                model: "stub-dev".into(),
            })
        });
        Self {
            sessions: DashMap::new(),
            config,
            llm,
        }
    }

    /// Start a new orchestration session. Returns the session ID.
    ///
    /// The orchestration loop runs on a dedicated OS thread (with its own
    /// single-threaded tokio runtime) because `OrchestrationLoop` is `!Send`
    /// due to the underlying `rusqlite::Connection`.
    pub fn create_session(
        &self,
        request: &str,
        workspace: Option<&str>,
        external_session_id: Option<&str>,
    ) -> anyhow::Result<String> {
        // Enforce concurrency limit (upper bound: total sessions count).
        let total = self.sessions.len();
        if total as u32 >= self.config.max_concurrent_sessions {
            anyhow::bail!(
                "max concurrent sessions ({}) reached",
                self.config.max_concurrent_sessions
            );
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let initial_update = SessionStatusUpdate {
            status: "active".into(),
            steps: 0,
            tokens_used: 0,
        };
        let (status_tx, status_rx) = watch::channel(initial_update);

        let cancel = CancellationToken::new();

        let (dashboard_tx, _) = broadcast::channel(LIVE_BROADCAST_CAPACITY);

        let state = SessionState {
            orchestration_state: None,
            status: SessionStatus::Active,
            status_tx,
            status_rx,
            created_at: now,
            updated_at: now,
            user_request: request.to_string(),
            cancel: cancel.clone(),
            external_session_id: external_session_id.map(|s| s.to_string()),
            hook_events: Vec::new(),
            dashboard_tx: dashboard_tx.clone(),
            dashboard_events: Vec::new(),
        };

        let state = Arc::new(Mutex::new(state));
        self.sessions.insert(session_id.clone(), Arc::clone(&state));

        // Prepare values for the background thread.
        let config = self.config.clone();
        let llm = Arc::clone(&self.llm);
        let user_request = request.to_string();
        let workspace_owned = workspace.map(|s| s.to_string());
        let sid = session_id.clone();
        let cancel_for_thread = cancel;
        let ext_sid = external_session_id.map(|s| s.to_string());

        // Create mpsc channel for the orchestration loop → bridge task.
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<oco_shared_types::OrchestrationEvent>();

        // Bridge task: receives OrchestrationEvent from mpsc, wraps as
        // DashboardEvent, stores for catch-up, and broadcasts to SSE consumers.
        let bridge_state = Arc::clone(&state);
        let bridge_sid = SessionId(uuid::Uuid::parse_str(&session_id).expect("valid session UUID"));
        tokio::spawn(async move {
            let event_stream = EventStream::new(bridge_sid, uuid::Uuid::new_v4());
            while let Some(orch_event) = event_rx.recv().await {
                let dashboard_event = event_stream.wrap(&orch_event);
                let mut guard = bridge_state.lock().await;
                guard.dashboard_events.push(dashboard_event.clone());
                // Broadcast — ignore error (no receivers is fine).
                let _ = guard.dashboard_tx.send(dashboard_event);
            }
        });

        // `OrchestrationLoop` is !Send (rusqlite Connection).
        // Run it on a dedicated OS thread with its own current-thread runtime.
        let state_for_thread = Arc::clone(&state);
        let thread_handle = std::thread::Builder::new()
            .name(format!("oco-session-{}", &session_id[..8]))
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build per-session tokio runtime");

                rt.block_on(async move {
                    info!(session_id = %sid, "Background orchestration starting");

                    let mut oloop = OrchestrationLoop::new(config, llm);
                    oloop.with_event_channel(event_tx);
                    oloop.with_cancellation(cancel_for_thread);
                    if let Some(ref ext_id) = ext_sid {
                        oloop.with_external_session_id(ext_id);
                    }

                    match oloop.run(user_request, workspace_owned).await {
                        Ok(final_state) => {
                            let mut guard = state_for_thread.lock().await;
                            let tokens = final_state.session.budget.tokens_used;
                            let steps = final_state.session.step_count;
                            let _ = guard.status_tx.send(SessionStatusUpdate {
                                status: "completed".into(),
                                steps,
                                tokens_used: tokens,
                            });
                            guard.status = SessionStatus::Completed;
                            guard.updated_at = Utc::now();
                            guard.orchestration_state = Some(final_state);
                            info!(session_id = %sid, "Orchestration completed");
                        }
                        Err(e) => {
                            error!(session_id = %sid, error = %e, "Orchestration failed");
                            let mut guard = state_for_thread.lock().await;
                            let _ = guard.status_tx.send(SessionStatusUpdate {
                                status: "failed".into(),
                                steps: 0,
                                tokens_used: 0,
                            });
                            guard.status = SessionStatus::Failed;
                            guard.updated_at = Utc::now();
                        }
                    }
                });
            })?;

        // Fire-and-forget: wait for thread completion on a blocking task
        // so we don't leak OS threads.
        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = thread_handle.join();
            })
            .await;
        });

        Ok(session_id)
    }

    /// Get information about a session.
    pub async fn get_session(&self, id: &str) -> Option<SessionInfo> {
        let session = self.sessions.get(id).map(|e| e.value().clone())?;
        let guard = session.lock().await;
        Some(self.build_info(id, &guard))
    }

    /// Stop (cancel) a running session.
    ///
    /// Sets the status to `Cancelled`. The background thread will observe this
    /// on its next budget/step check and exit gracefully.
    pub async fn stop_session(&self, id: &str) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get(id)
            .map(|e| e.value().clone())
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

        let mut guard = session.lock().await;

        if guard.status != SessionStatus::Active {
            anyhow::bail!("session {id} is not active (status: {:?})", guard.status);
        }

        let _ = guard.status_tx.send(SessionStatusUpdate {
            status: "cancelled".into(),
            steps: guard
                .orchestration_state
                .as_ref()
                .map(|s| s.session.step_count)
                .unwrap_or(0),
            tokens_used: guard
                .orchestration_state
                .as_ref()
                .map(|s| s.session.budget.tokens_used)
                .unwrap_or(0),
        });

        guard.status = SessionStatus::Cancelled;
        guard.updated_at = Utc::now();
        // Signal the cancellation token — the orchestration loop checks this cooperatively.
        guard.cancel.cancel();
        info!(session_id = %id, "Session cancelled (token signaled)");
        Ok(())
    }

    /// Get a compact snapshot of the session's working memory for post-compact re-injection.
    ///
    /// Returns `None` if the session doesn't exist, has no orchestration state,
    /// or if the working memory is empty.
    pub async fn get_compact_snapshot(&self, id: &str) -> Option<serde_json::Value> {
        let session = self.sessions.get(id).map(|e| e.value().clone())?;
        let guard = session.lock().await;
        let state = guard.orchestration_state.as_ref()?;
        let snapshot = state.memory.compact_snapshot();
        // Return None for empty snapshots — don't inject noise
        let obj = snapshot.as_object()?;
        let has_content = obj.values().any(|v| match v {
            serde_json::Value::Array(a) => !a.is_empty(),
            serde_json::Value::Object(o) => !o.is_empty(),
            serde_json::Value::Null => false,
            _ => true,
        });
        if has_content { Some(snapshot) } else { None }
    }

    /// Resolve a session ID — tries direct OCO key first, then reverse lookup
    /// by `external_session_id` (Claude Code session ID).
    pub async fn resolve_session_id(&self, id: &str) -> Option<String> {
        // Direct lookup by OCO session UUID
        if self.sessions.contains_key(id) {
            return Some(id.to_string());
        }
        // Reverse lookup by external_session_id (Claude Code session)
        let snapshot: Vec<(String, Arc<Mutex<SessionState>>)> = self
            .sessions
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();
        for (oco_id, session) in snapshot {
            let guard = session.lock().await;
            if guard.external_session_id.as_deref() == Some(id) {
                return Some(oco_id);
            }
        }
        None
    }

    /// Get all hook events for a session.
    pub async fn get_hook_events(&self, id: &str) -> anyhow::Result<Vec<HookEvent>> {
        let session = self
            .sessions
            .get(id)
            .map(|e| e.value().clone())
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

        let guard = session.lock().await;
        Ok(guard.hook_events.clone())
    }

    /// Get the decision trace for a session.
    pub async fn get_trace(&self, id: &str) -> anyhow::Result<Vec<DecisionTrace>> {
        let session = self
            .sessions
            .get(id)
            .map(|e| e.value().clone())
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

        let guard = session.lock().await;
        Ok(guard
            .orchestration_state
            .as_ref()
            .map(|s| s.traces.clone())
            .unwrap_or_default())
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        // Collect Arc clones first, then release DashMap guards before awaiting.
        let snapshot: Vec<(String, Arc<Mutex<SessionState>>)> = self
            .sessions
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        let mut infos = Vec::with_capacity(snapshot.len());
        for (id, session) in &snapshot {
            let guard = session.lock().await;
            infos.push(self.build_info(id, &guard));
        }
        infos
    }

    /// Number of currently active sessions.
    pub async fn active_count(&self) -> u32 {
        let snapshot: Vec<Arc<Mutex<SessionState>>> =
            self.sessions.iter().map(|e| e.value().clone()).collect();

        let mut count = 0u32;
        for session in &snapshot {
            let guard = session.lock().await;
            if guard.status == SessionStatus::Active {
                count += 1;
            }
        }
        count
    }

    /// Record a hook event for a session (telemetry + persistence).
    ///
    /// Returns `true` if the event was recorded against an active session,
    /// `false` if it was dropped (session not found or not active).
    /// The event is always stored (with `recorded: false` for drops) as long
    /// as the session exists, for post-mortem debug.
    pub async fn record_hook_event(&self, session_id: &str, hook_name: &str, detail: &str) -> bool {
        // Clone the Arc before releasing the DashMap read guard to avoid
        // holding it across the `.lock().await` (potential deadlock).
        let session = self.sessions.get(session_id).map(|e| e.value().clone());
        match session {
            Some(session) => {
                let mut guard = session.lock().await;
                let recorded = guard.status == SessionStatus::Active;

                // Store the event (bounded)
                if guard.hook_events.len() < MAX_HOOK_EVENTS {
                    guard.hook_events.push(HookEvent {
                        timestamp: Utc::now(),
                        hook_name: hook_name.to_string(),
                        detail: detail.to_string(),
                        session_id: Some(session_id.to_string()),
                        recorded,
                    });
                } else if guard.hook_events.len() == MAX_HOOK_EVENTS {
                    tracing::warn!(
                        session_id,
                        max = MAX_HOOK_EVENTS,
                        "hook event buffer full, further events will be dropped"
                    );
                }

                if recorded {
                    tracing::debug!(
                        session_id,
                        hook_name,
                        detail,
                        "recorded hook event for active session"
                    );
                } else {
                    tracing::warn!(
                        session_id,
                        hook_name,
                        detail,
                        status = ?guard.status,
                        "hook event dropped: session not active"
                    );
                }
                recorded
            }
            None => {
                tracing::warn!(
                    session_id,
                    hook_name,
                    detail,
                    "hook event dropped: session not found"
                );
                false
            }
        }
    }

    /// Subscribe to live dashboard events for a session.
    /// Returns the accumulated events (for catch-up) and a broadcast receiver.
    /// Create a tracking-only session (no OrchestrationLoop).
    /// Used by the MCP bridge for dashboard event tracking only.
    pub fn create_tracking_session(&self, task: &str) -> anyhow::Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let initial_update = SessionStatusUpdate {
            status: "tracking".into(),
            steps: 0,
            tokens_used: 0,
        };
        let (status_tx, status_rx) = watch::channel(initial_update);
        let (dashboard_tx, _) = broadcast::channel(LIVE_BROADCAST_CAPACITY);

        let state = SessionState {
            orchestration_state: None,
            status: SessionStatus::Active,
            status_tx,
            status_rx,
            created_at: now,
            updated_at: now,
            user_request: task.to_string(),
            cancel: CancellationToken::new(),
            external_session_id: None,
            hook_events: Vec::new(),
            dashboard_tx,
            dashboard_events: Vec::new(),
        };

        self.sessions
            .insert(session_id.clone(), Arc::new(Mutex::new(state)));
        info!(session_id = %session_id, "Created tracking-only session");
        Ok(session_id)
    }

    pub async fn subscribe_dashboard(
        &self,
        id: &str,
    ) -> Option<(Vec<DashboardEvent>, broadcast::Receiver<DashboardEvent>)> {
        let session = self.sessions.get(id).map(|e| e.value().clone())?;
        let guard = session.lock().await;
        let events = guard.dashboard_events.clone();
        let rx = guard.dashboard_tx.subscribe();
        Some((events, rx))
    }

    /// Inject an externally-created dashboard event into a session's live broadcast.
    /// Used by the MCP bridge to push phase updates from Claude Code into the dashboard.
    pub async fn inject_dashboard_event(
        &self,
        id: &str,
        kind: oco_shared_types::dashboard::DashboardEventKind,
    ) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get(id)
            .map(|e| e.value().clone())
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

        let mut guard = session.lock().await;

        // Assign a sequence number to the event.
        let seq = guard.dashboard_events.len() as u64 + 1;
        let event = DashboardEvent::new(
            seq,
            SessionId(uuid::Uuid::parse_str(id).unwrap_or_default()),
            uuid::Uuid::nil(),
            0,
            kind,
        );

        guard.dashboard_events.push(event.clone());
        let _ = guard.dashboard_tx.send(event);
        Ok(())
    }

    // -- helpers ------------------------------------------------------------

    fn build_info(&self, id: &str, guard: &SessionState) -> SessionInfo {
        let (steps, tokens, complexity) = match &guard.orchestration_state {
            Some(s) => (
                s.session.step_count,
                s.session.budget.tokens_used,
                format!("{:?}", s.task_complexity),
            ),
            None => {
                // Fall back to the watch channel's latest value.
                let latest = guard.status_rx.borrow();
                (latest.steps, latest.tokens_used, "Unknown".into())
            }
        };

        SessionInfo {
            id: id.to_string(),
            status: format!("{:?}", guard.status),
            steps,
            tokens_used: tokens,
            created_at: guard.created_at,
            updated_at: guard.updated_at,
            complexity,
            user_request: guard.user_request.clone(),
            external_session_id: guard.external_session_id.clone(),
            hook_events_count: guard.hook_events.len() as u32,
        }
    }

    /// Create a session without starting the background orchestration loop (test-only).
    /// The session stays Active indefinitely, making tests deterministic.
    #[cfg(test)]
    pub fn create_test_session(&self, request: &str, external_session_id: Option<&str>) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let initial_update = SessionStatusUpdate {
            status: "active".into(),
            steps: 0,
            tokens_used: 0,
        };
        let (status_tx, status_rx) = watch::channel(initial_update);
        let (dashboard_tx, _) = broadcast::channel(LIVE_BROADCAST_CAPACITY);
        let state = SessionState {
            orchestration_state: None,
            status: SessionStatus::Active,
            status_tx,
            status_rx,
            created_at: now,
            updated_at: now,
            user_request: request.to_string(),
            cancel: CancellationToken::new(),
            external_session_id: external_session_id.map(|s| s.to_string()),
            hook_events: Vec::new(),
            dashboard_tx,
            dashboard_events: Vec::new(),
        };
        self.sessions
            .insert(session_id.clone(), Arc::new(Mutex::new(state)));
        session_id
    }

    /// Inject an OrchestrationState into a session (test-only).
    #[cfg(test)]
    pub async fn inject_state(&self, id: &str, state: OrchestrationState) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get(id)
            .map(|e| e.value().clone())
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;
        let mut guard = session.lock().await;
        guard.orchestration_state = Some(state);
        Ok(())
    }
}
