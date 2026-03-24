//! Persistent session manager with real orchestration support.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use tokio::sync::{watch, Mutex};
use tracing::{error, info};

use oco_orchestrator_core::llm::{LlmProvider, StubLlmProvider};
use oco_orchestrator_core::state::OrchestrationState;
use oco_orchestrator_core::{OrchestrationLoop, OrchestratorConfig};
use oco_shared_types::{DecisionTrace, SessionStatus};

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
}

/// Internal state held per session.
pub struct SessionState {
    pub orchestration_state: Option<OrchestrationState>,
    pub status: SessionStatus,
    pub status_tx: watch::Sender<SessionStatusUpdate>,
    pub status_rx: watch::Receiver<SessionStatusUpdate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub user_request: String,
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

        let state = SessionState {
            orchestration_state: None,
            status: SessionStatus::Active,
            status_tx,
            status_rx,
            created_at: now,
            updated_at: now,
            user_request: request.to_string(),
        };

        let state = Arc::new(Mutex::new(state));
        self.sessions.insert(session_id.clone(), Arc::clone(&state));

        // Prepare values for the background thread.
        let config = self.config.clone();
        let llm = Arc::clone(&self.llm);
        let user_request = request.to_string();
        let workspace_owned = workspace.map(|s| s.to_string());
        let sid = session_id.clone();

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
        let entry = self.sessions.get(id)?;
        let guard = entry.lock().await;
        Some(self.build_info(id, &guard))
    }

    /// Stop (cancel) a running session.
    ///
    /// Sets the status to `Cancelled`. The background thread will observe this
    /// on its next budget/step check and exit gracefully.
    pub async fn stop_session(&self, id: &str) -> anyhow::Result<()> {
        let entry = self
            .sessions
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

        let mut guard = entry.lock().await;

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
        info!(session_id = %id, "Session cancelled");
        Ok(())
    }

    /// Get the decision trace for a session.
    pub async fn get_trace(&self, id: &str) -> anyhow::Result<Vec<DecisionTrace>> {
        let entry = self
            .sessions
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

        let guard = entry.lock().await;
        Ok(guard
            .orchestration_state
            .as_ref()
            .map(|s| s.traces.clone())
            .unwrap_or_default())
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let mut infos = Vec::with_capacity(self.sessions.len());
        for entry in self.sessions.iter() {
            let id = entry.key().clone();
            let guard = entry.value().lock().await;
            infos.push(self.build_info(&id, &guard));
        }
        infos
    }

    /// Number of currently active sessions.
    pub async fn active_count(&self) -> u32 {
        let mut count = 0u32;
        for entry in self.sessions.iter() {
            let guard = entry.value().lock().await;
            if guard.status == SessionStatus::Active {
                count += 1;
            }
        }
        count
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
        }
    }
}
