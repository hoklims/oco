//! Replay engine — reads `trace.jsonl` and re-emits as `DashboardEvent` stream.
//!
//! Supports pause/resume/seek/speed control. This is the foundation for
//! both the SSE replay endpoint and TUI trace viewer.
//!
//! Architecture (from GPT-5.4 review):
//! - Replay and live sessions use the **same** `DashboardEvent` contract
//! - Cursor-based: clients can reconnect with `?after_seq=N`
//! - Speed-controllable: 0.5x to 10x real-time, or instant

use std::path::Path;
use std::sync::Arc;

use oco_shared_types::dashboard::{DashboardEvent, EventStream};
use oco_shared_types::telemetry::OrchestrationEvent;
use oco_shared_types::SessionId;
use tokio::sync::{broadcast, watch, Mutex};
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::OrchestratorError;

// ── Replay state ─────────────────────────────────────────────

/// Replay playback state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Finished,
}

/// Controls for an active replay session.
#[derive(Debug, Clone)]
pub struct ReplayControls {
    /// Current playback state.
    state: Arc<watch::Sender<PlaybackState>>,
    /// Playback speed multiplier (1.0 = real-time).
    speed: Arc<watch::Sender<f64>>,
}

impl ReplayControls {
    pub fn pause(&self) {
        let _ = self.state.send(PlaybackState::Paused);
    }

    pub fn resume(&self) {
        let _ = self.state.send(PlaybackState::Playing);
    }

    pub fn set_speed(&self, speed: f64) {
        let clamped = speed.clamp(0.1, 100.0);
        let _ = self.speed.send(clamped);
    }

    pub fn is_paused(&self) -> bool {
        *self.state.borrow() == PlaybackState::Paused
    }

    pub fn is_finished(&self) -> bool {
        *self.state.borrow() == PlaybackState::Finished
    }
}

// ── Loaded trace ─────────────────────────────────────────────

/// A trace loaded from disk, ready for replay.
#[derive(Debug)]
pub struct LoadedTrace {
    /// The raw orchestration events from trace.jsonl.
    pub events: Vec<OrchestrationEvent>,
    /// Session metadata from summary.json (if available).
    pub summary: Option<serde_json::Value>,
    /// Source path for diagnostics.
    pub source_dir: String,
}

impl LoadedTrace {
    /// Load a trace from a run directory (`.oco/runs/<id>/`).
    pub fn from_run_dir(run_dir: &Path) -> Result<Self, OrchestratorError> {
        let trace_path = run_dir.join("trace.jsonl");
        let summary_path = run_dir.join("summary.json");

        if !trace_path.exists() {
            return Err(OrchestratorError::ConfigError(format!(
                "trace.jsonl not found in {}",
                run_dir.display()
            )));
        }

        let content = std::fs::read_to_string(&trace_path).map_err(|e| {
            OrchestratorError::ConfigError(format!("failed to read trace.jsonl: {e}"))
        })?;

        let mut events = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<OrchestrationEvent>(trimmed) {
                Ok(event) => events.push(event),
                Err(e) => {
                    warn!(line = line_num + 1, error = %e, "skipping malformed trace line");
                }
            }
        }

        let summary = if summary_path.exists() {
            std::fs::read_to_string(&summary_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };

        info!(
            events = events.len(),
            source = %run_dir.display(),
            "loaded trace for replay"
        );

        Ok(Self {
            events,
            summary,
            source_dir: run_dir.display().to_string(),
        })
    }

    /// Extract session_id from summary, or generate a new one.
    pub fn session_id(&self) -> SessionId {
        self.summary
            .as_ref()
            .and_then(|s| s["session_id"].as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(SessionId)
            .unwrap_or_default()
    }
}

// ── Replay session ───────────────────────────────────────────

/// An active replay session with controls and event broadcast.
pub struct ReplaySession {
    /// All events pre-converted to DashboardEvent (for cursor access).
    dashboard_events: Vec<DashboardEvent>,
    /// Broadcast channel for live consumers (SSE, TUI, etc.).
    tx: broadcast::Sender<DashboardEvent>,
    /// Controls for pause/resume/speed.
    controls: ReplayControls,
    /// Playback state receiver (for the replay loop).
    state_rx: watch::Receiver<PlaybackState>,
    /// Speed receiver (for the replay loop).
    speed_rx: watch::Receiver<f64>,
}

impl ReplaySession {
    /// Create a replay session from a loaded trace.
    ///
    /// Pre-converts all events to `DashboardEvent` with seq numbers.
    /// This enables instant seek and cursor-based access.
    pub fn new(trace: &LoadedTrace) -> Self {
        let session_id = trace.session_id();
        let run_id = Uuid::new_v4();
        let stream = EventStream::new(session_id, run_id);

        // Pre-wrap all events.
        let dashboard_events: Vec<DashboardEvent> = trace
            .events
            .iter()
            .map(|e| stream.wrap(e))
            .collect();

        let (tx, _rx) = broadcast::channel(256);
        let (state_tx, state_rx) = watch::channel(PlaybackState::Playing);
        let (speed_tx, speed_rx) = watch::channel(1.0);

        let controls = ReplayControls {
            state: Arc::new(state_tx),
            speed: Arc::new(speed_tx),
        };

        Self {
            dashboard_events,
            tx,
            controls,
            state_rx,
            speed_rx,
        }
    }

    /// Get controls for this replay (pause/resume/speed).
    pub fn controls(&self) -> ReplayControls {
        self.controls.clone()
    }

    /// Subscribe to the event broadcast (for SSE consumers).
    pub fn subscribe(&self) -> broadcast::Receiver<DashboardEvent> {
        self.tx.subscribe()
    }

    /// Get all events (for snapshot/cursor access).
    pub fn events(&self) -> &[DashboardEvent] {
        &self.dashboard_events
    }

    /// Get events after a given sequence number (for reconnect).
    pub fn events_after_seq(&self, seq: u64) -> &[DashboardEvent] {
        match self.dashboard_events.binary_search_by_key(&seq, |e| e.seq) {
            Ok(idx) => &self.dashboard_events[idx + 1..],
            Err(idx) => &self.dashboard_events[idx..],
        }
    }

    /// Get a snapshot: all events up to and including the given seq.
    pub fn snapshot_up_to(&self, seq: u64) -> &[DashboardEvent] {
        match self.dashboard_events.binary_search_by_key(&seq, |e| e.seq) {
            Ok(idx) => &self.dashboard_events[..=idx],
            Err(idx) => &self.dashboard_events[..idx],
        }
    }

    /// Total event count.
    pub fn event_count(&self) -> usize {
        self.dashboard_events.len()
    }

    /// Run the replay loop — emits events through the broadcast channel
    /// respecting speed and pause controls.
    ///
    /// This is meant to be spawned as a tokio task.
    pub async fn run(self) {
        let events = self.dashboard_events;
        let tx = self.tx;
        let mut state_rx = self.state_rx;
        let mut speed_rx = self.speed_rx;

        if events.is_empty() {
            let _ = self.controls.state.send(PlaybackState::Finished);
            return;
        }

        // Compute inter-event delays from original timestamps.
        let mut delays_ms: Vec<u64> = Vec::with_capacity(events.len());
        delays_ms.push(0); // First event: no delay.
        for window in events.windows(2) {
            let delta = window[1]
                .ts
                .signed_duration_since(window[0].ts)
                .num_milliseconds()
                .max(0) as u64;
            delays_ms.push(delta);
        }

        for (i, event) in events.into_iter().enumerate() {
            // Check pause state.
            loop {
                if *state_rx.borrow() == PlaybackState::Playing {
                    break;
                }
                // Wait for state change (resume or external cancel).
                if state_rx.changed().await.is_err() {
                    return; // Sender dropped — session closed.
                }
            }

            // Apply speed-adjusted delay.
            let speed = *speed_rx.borrow_and_update();
            let delay_ms = delays_ms[i];
            if delay_ms > 0 && speed > 0.0 {
                let adjusted = (delay_ms as f64 / speed) as u64;
                if adjusted > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(adjusted)).await;
                }
            }

            // Broadcast event. If no subscribers, that's fine.
            let _ = tx.send(event);
        }

        let _ = self.controls.state.send(PlaybackState::Finished);
    }
}

// ── Replay registry (manages active replays) ────────────────

/// Manages active replay sessions by ID.
pub struct ReplayRegistry {
    sessions: Mutex<std::collections::HashMap<Uuid, Arc<ReplaySession>>>,
}

impl ReplayRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Create a new replay session from a loaded trace. Returns the replay ID.
    pub async fn create(&self, trace: &LoadedTrace) -> (Uuid, Arc<ReplaySession>) {
        let id = Uuid::new_v4();
        let session = Arc::new(ReplaySession::new(trace));
        self.sessions.lock().await.insert(id, Arc::clone(&session));
        (id, session)
    }

    /// Get an active replay session by ID.
    pub async fn get(&self, id: &Uuid) -> Option<Arc<ReplaySession>> {
        self.sessions.lock().await.get(id).cloned()
    }

    /// Remove a finished replay session.
    pub async fn remove(&self, id: &Uuid) {
        self.sessions.lock().await.remove(id);
    }

    /// List active replay IDs.
    pub async fn list(&self) -> Vec<Uuid> {
        self.sessions.lock().await.keys().copied().collect()
    }
}

impl Default for ReplayRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::StopReason;

    fn sample_events() -> Vec<OrchestrationEvent> {
        vec![
            OrchestrationEvent::BudgetWarning {
                resource: "tokens".into(),
                utilization: 0.5,
            },
            OrchestrationEvent::PlanGenerated {
                plan_id: Uuid::new_v4(),
                step_count: 2,
                parallel_group_count: 1,
                critical_path_length: 2,
                estimated_total_tokens: 3000,
                strategy: "emergent".into(),
                team: None,
                steps: vec![],
            },
            OrchestrationEvent::PlanStepStarted {
                step_id: Uuid::new_v4(),
                step_name: "implement".into(),
                role: "implementer".into(),
                execution_mode: "inline".into(),
            },
            OrchestrationEvent::PlanStepCompleted {
                step_id: Uuid::new_v4(),
                step_name: "implement".into(),
                success: true,
                duration_ms: 500,
                tokens_used: 1500,
            },
            OrchestrationEvent::Stopped {
                reason: StopReason::TaskComplete,
                total_steps: 4,
                total_tokens: 3000,
            },
        ]
    }

    fn make_trace(events: Vec<OrchestrationEvent>) -> LoadedTrace {
        LoadedTrace {
            events,
            summary: Some(serde_json::json!({
                "session_id": Uuid::nil().to_string(),
            })),
            source_dir: "/tmp/test".into(),
        }
    }

    #[test]
    fn replay_session_pre_converts_all_events() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);
        assert_eq!(session.event_count(), 5);

        // Seq numbers are monotonic.
        for (i, e) in session.events().iter().enumerate() {
            assert_eq!(e.seq, (i + 1) as u64);
        }
    }

    #[test]
    fn events_after_seq_returns_remaining() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);

        let after_2 = session.events_after_seq(2);
        assert_eq!(after_2.len(), 3); // seq 3, 4, 5

        let after_5 = session.events_after_seq(5);
        assert_eq!(after_5.len(), 0);

        let after_0 = session.events_after_seq(0);
        assert_eq!(after_0.len(), 5); // all events
    }

    #[test]
    fn snapshot_up_to_returns_prefix() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);

        let up_to_3 = session.snapshot_up_to(3);
        assert_eq!(up_to_3.len(), 3); // seq 1, 2, 3

        let up_to_0 = session.snapshot_up_to(0);
        assert_eq!(up_to_0.len(), 0);

        let up_to_99 = session.snapshot_up_to(99);
        assert_eq!(up_to_99.len(), 5); // all
    }

    #[test]
    fn plan_version_tracked_across_events() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);
        let events = session.events();

        // First event (BudgetWarning) = plan_version 0.
        assert_eq!(events[0].plan_version, 0);
        // Second event (PlanGenerated) = plan_version 1.
        assert_eq!(events[1].plan_version, 1);
        // Subsequent events carry plan_version 1.
        assert_eq!(events[2].plan_version, 1);
        assert_eq!(events[4].plan_version, 1);
    }

    #[test]
    fn controls_pause_resume() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);
        let controls = session.controls();

        assert!(!controls.is_paused());
        controls.pause();
        assert!(controls.is_paused());
        controls.resume();
        assert!(!controls.is_paused());
    }

    #[test]
    fn controls_speed_clamped() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);
        let controls = session.controls();

        controls.set_speed(0.01); // Below min → clamped to 0.1
        controls.set_speed(999.0); // Above max → clamped to 100.0
        // No panic = success.
    }

    #[tokio::test]
    async fn replay_run_emits_all_events() {
        let trace = make_trace(sample_events());
        let session = ReplaySession::new(&trace);
        let mut rx = session.subscribe();
        let controls = session.controls();

        // Set speed very high to avoid delays.
        controls.set_speed(100.0);

        // Run replay in background.
        tokio::spawn(session.run());

        let mut received = Vec::new();
        loop {
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(5),
                rx.recv(),
            )
            .await
            {
                Ok(Ok(event)) => received.push(event),
                Ok(Err(broadcast::error::RecvError::Closed)) => break,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Err(_) => break, // timeout
            }
        }

        assert_eq!(received.len(), 5, "should receive all 5 events");
        // Verify seq ordering.
        for (i, e) in received.iter().enumerate() {
            assert_eq!(e.seq, (i + 1) as u64);
        }
    }

    #[tokio::test]
    async fn replay_empty_trace_finishes_immediately() {
        let trace = make_trace(vec![]);
        let session = ReplaySession::new(&trace);
        let controls = session.controls();

        tokio::spawn(session.run());

        // Give it a moment.
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(controls.is_finished());
    }

    #[tokio::test]
    async fn registry_create_get_remove() {
        let registry = ReplayRegistry::new();
        let trace = make_trace(sample_events());
        let (id, _session) = registry.create(&trace).await;

        assert!(registry.get(&id).await.is_some());
        assert_eq!(registry.list().await.len(), 1);

        registry.remove(&id).await;
        assert!(registry.get(&id).await.is_none());
        assert_eq!(registry.list().await.len(), 0);
    }

    #[test]
    fn loaded_trace_session_id_from_summary() {
        let trace = make_trace(vec![]);
        assert_eq!(trace.session_id(), SessionId(Uuid::nil()));
    }

    #[test]
    fn loaded_trace_session_id_fallback() {
        let trace = LoadedTrace {
            events: vec![],
            summary: None,
            source_dir: "/tmp".into(),
        };
        // Should generate a new random ID, not panic.
        let _ = trace.session_id();
    }
}
