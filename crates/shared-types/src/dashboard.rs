//! Canonical dashboard event envelope — the single contract for all observers.
//!
//! Every event (live or replayed) is wrapped in a [`DashboardEvent`] with
//! monotonic sequence number, wall-clock timestamp, session/run/plan context,
//! and a versioned payload. This envelope is the **only** type that SSE
//! endpoints, TUI renderers, and JSONL trace files should produce/consume.
//!
//! Design rationale (from GPT-5.4 review):
//! - `seq` enables cursor-based reconnect (`?after_seq=N`)
//! - `plan_version` tracks lineage across replans
//! - `summary` vs `detail_ref` keeps stream payloads small
//! - Schema version enables forward-compatible evolution

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::telemetry::{BudgetSnapshot, CheckResult, StepSummary, TeamSummary};
use crate::{SessionId, StopReason};

/// Current schema version. Bump on breaking changes to payload variants.
pub const DASHBOARD_SCHEMA_VERSION: u32 = 1;

/// The universal envelope for all dashboard-facing events.
///
/// Consumers should:
/// 1. Check `schema_version` and ignore unknown versions gracefully
/// 2. Use `seq` for cursor-based reconnect
/// 3. Use `session_id` + `run_id` for isolation
/// 4. Use `plan_version` to detect replan boundaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardEvent {
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// Monotonically increasing sequence number (per run).
    pub seq: u64,
    /// Wall-clock timestamp.
    pub ts: DateTime<Utc>,
    /// Session that owns this event.
    pub session_id: SessionId,
    /// Run ID within the session (a session may have multiple runs).
    pub run_id: Uuid,
    /// Current plan version (increments on each replan). 0 = no plan.
    pub plan_version: u32,
    /// The event payload.
    pub kind: DashboardEventKind,
}

impl DashboardEvent {
    /// Create a new event with the current schema version and timestamp.
    pub fn new(
        seq: u64,
        session_id: SessionId,
        run_id: Uuid,
        plan_version: u32,
        kind: DashboardEventKind,
    ) -> Self {
        Self {
            schema_version: DASHBOARD_SCHEMA_VERSION,
            seq,
            ts: Utc::now(),
            session_id,
            run_id,
            plan_version,
            kind,
        }
    }
}

/// Event payload — what happened. Summaries only; heavy data behind detail_ref.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DashboardEventKind {
    // ── Lifecycle ────────────────────────────────────────────
    /// Run started.
    RunStarted {
        provider: String,
        model: String,
        request_summary: String,
        /// Task complexity label (e.g. "medium − research + architecture").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        complexity: Option<String>,
    },

    /// Run stopped.
    RunStopped {
        reason: StopReason,
        total_steps: u32,
        total_tokens: u64,
    },

    // ── Plan ─────────────────────────────────────────────────
    /// Competitive planning: multiple candidates explored and scored.
    PlanExploration {
        candidates: Vec<crate::telemetry::PlanCandidateSummary>,
        winner_strategy: String,
        winner_score: f64,
    },

    /// A new execution plan was generated (or regenerated after replan).
    PlanGenerated {
        plan_id: String,
        step_count: usize,
        parallel_group_count: usize,
        critical_path_length: u32,
        estimated_total_tokens: u64,
        strategy: String,
        team: Option<TeamSummary>,
        steps: Vec<StepSummary>,
    },

    /// A plan step started executing.
    StepStarted {
        step_id: String,
        step_name: String,
        role: String,
        execution_mode: String,
    },

    /// A plan step completed.
    StepCompleted {
        step_id: String,
        step_name: String,
        success: bool,
        duration_ms: u64,
        tokens_used: u64,
        /// Reference to fetch full output (e.g. tool stdout).
        /// Keeps the stream payload small.
        detail_ref: Option<String>,
    },

    // ── Flat loop step (non-plan tasks) ─────────────────────
    /// A flat-loop step completed (Trivial/Low complexity tasks without a plan).
    FlatStepCompleted {
        step: u32,
        action_type: String,
        reason: String,
        duration_ms: u64,
        budget_snapshot: BudgetSnapshot,
    },

    // ── Progress ─────────────────────────────────────────────
    /// Live progress during plan execution.
    Progress {
        completed: usize,
        total: usize,
        active_steps: Vec<ActiveStep>,
        budget: BudgetSnapshot,
    },

    // ── Verification ─────────────────────────────────────────
    /// Verify gate evaluated after a step.
    VerifyGateResult {
        step_id: String,
        step_name: String,
        checks: Vec<CheckResult>,
        overall_passed: bool,
        replan_triggered: bool,
    },

    /// Replanning triggered.
    ReplanTriggered {
        failed_step_name: String,
        attempt: u32,
        max_attempts: u32,
        steps_preserved: usize,
        steps_removed: usize,
        steps_added: usize,
    },

    // ── Budget ───────────────────────────────────────────────
    /// Budget crossed a warning threshold.
    BudgetWarning { resource: String, utilization: f64 },

    /// Full budget snapshot (emitted periodically or on request).
    BudgetSnapshot(BudgetSnapshot),

    // ── Index ────────────────────────────────────────────────
    /// Indexing progress.
    IndexProgress {
        files_done: u32,
        symbols_so_far: u32,
    },

    // ── Sub-plans (ADR-008) ────────────────────────────────────
    /// A sub-plan started executing.
    SubPlanStarted {
        parent_step_id: String,
        parent_step_name: String,
        sub_steps: Vec<SubStepSummary>,
    },
    /// Sub-step status changed.
    SubStepProgress {
        parent_step_id: String,
        sub_step_id: String,
        sub_step_name: String,
        status: String,
    },
    /// A sub-plan completed.
    SubPlanCompleted {
        parent_step_id: String,
        parent_step_name: String,
        success: bool,
    },

    // ── Teammate communication ───────────────────────────────
    /// A teammate sent a message to another.
    TeammateMessage {
        from_step_id: String,
        to_step_id: String,
        from_name: String,
        to_name: String,
        summary: String,
    },

    // ── Heartbeat ────────────────────────────────────────────
    /// Keepalive for SSE connections. Clients should ignore this.
    Heartbeat,
}

/// Summary of a sub-step in a sub-plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubStepSummary {
    pub id: String,
    pub name: String,
}

/// A currently-active step (for progress display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveStep {
    pub step_id: String,
    pub step_name: String,
}

// ── Conversion from OrchestrationEvent ─────────────────────────

use crate::telemetry::OrchestrationEvent;

/// Extract a human-readable action type name from an OrchestratorAction.
fn action_type_name(action: &crate::OrchestratorAction) -> String {
    use crate::OrchestratorAction;
    match action {
        OrchestratorAction::Respond { .. } => "respond".into(),
        OrchestratorAction::Retrieve { .. } => "retrieve".into(),
        OrchestratorAction::ToolCall { tool_name, .. } => format!("tool:{tool_name}"),
        OrchestratorAction::Verify { strategy, .. } => format!("verify:{strategy:?}"),
        OrchestratorAction::UpdateMemory { .. } => "memory".into(),
        OrchestratorAction::Stop { reason, .. } => format!("stop:{reason:?}"),
        OrchestratorAction::Plan { .. } => "plan".into(),
        OrchestratorAction::Delegate { agent_role, .. } => format!("delegate:{}", agent_role.name),
        OrchestratorAction::Message { message_type, .. } => format!("message:{message_type:?}"),
        OrchestratorAction::Replan { .. } => "replan".into(),
    }
}

impl DashboardEventKind {
    /// Convert a raw `OrchestrationEvent` into a `DashboardEventKind`.
    ///
    /// This is the single mapping point. No other code should inspect
    /// `OrchestrationEvent` for dashboard/UI purposes.
    pub fn from_orchestration_event(event: &OrchestrationEvent) -> Self {
        match event {
            OrchestrationEvent::RunStarted {
                provider,
                model,
                request_summary,
                complexity,
            } => DashboardEventKind::RunStarted {
                provider: provider.clone(),
                model: model.clone(),
                request_summary: request_summary.clone(),
                complexity: Some(complexity.clone()),
            },
            OrchestrationEvent::StepCompleted {
                step,
                action,
                reason,
                duration_ms,
                budget_snapshot,
                ..
            } => DashboardEventKind::FlatStepCompleted {
                step: *step,
                action_type: action_type_name(action),
                reason: reason.clone(),
                duration_ms: *duration_ms,
                budget_snapshot: budget_snapshot.clone(),
            },
            OrchestrationEvent::PlanGenerated {
                plan_id,
                step_count,
                parallel_group_count,
                critical_path_length,
                estimated_total_tokens,
                strategy,
                team,
                steps,
            } => DashboardEventKind::PlanGenerated {
                plan_id: plan_id.to_string(),
                step_count: *step_count,
                parallel_group_count: *parallel_group_count,
                critical_path_length: *critical_path_length,
                estimated_total_tokens: *estimated_total_tokens,
                strategy: strategy.clone(),
                team: team.clone(),
                steps: steps.clone(),
            },
            OrchestrationEvent::PlanStepStarted {
                step_id,
                step_name,
                role,
                execution_mode,
            } => DashboardEventKind::StepStarted {
                step_id: step_id.to_string(),
                step_name: step_name.clone(),
                role: role.clone(),
                execution_mode: execution_mode.clone(),
            },
            OrchestrationEvent::PlanStepCompleted {
                step_id,
                step_name,
                success,
                duration_ms,
                tokens_used,
            } => DashboardEventKind::StepCompleted {
                step_id: step_id.to_string(),
                step_name: step_name.clone(),
                success: *success,
                duration_ms: *duration_ms,
                tokens_used: *tokens_used,
                detail_ref: None,
            },
            OrchestrationEvent::PlanProgress {
                completed,
                total,
                active_steps,
                budget_used_pct: _,
                tokens_used,
                tokens_budget,
            } => DashboardEventKind::Progress {
                completed: *completed,
                total: *total,
                active_steps: active_steps
                    .iter()
                    .map(|(id, name)| ActiveStep {
                        step_id: id.to_string(),
                        step_name: name.clone(),
                    })
                    .collect(),
                budget: BudgetSnapshot {
                    tokens_used: *tokens_used,
                    tokens_remaining: tokens_budget.saturating_sub(*tokens_used),
                    tool_calls_used: 0,
                    tool_calls_remaining: 0,
                    retrievals_used: 0,
                    verify_cycles_used: 0,
                    elapsed_secs: 0,
                },
            },
            OrchestrationEvent::VerifyGateResult {
                step_id,
                step_name,
                checks,
                overall_passed,
                replan_triggered,
            } => DashboardEventKind::VerifyGateResult {
                step_id: step_id.to_string(),
                step_name: step_name.clone(),
                checks: checks.clone(),
                overall_passed: *overall_passed,
                replan_triggered: *replan_triggered,
            },
            OrchestrationEvent::ReplanTriggered {
                failed_step_name,
                attempt,
                max_attempts,
                steps_preserved,
                steps_removed,
                steps_added,
            } => DashboardEventKind::ReplanTriggered {
                failed_step_name: failed_step_name.clone(),
                attempt: *attempt,
                max_attempts: *max_attempts,
                steps_preserved: *steps_preserved,
                steps_removed: *steps_removed,
                steps_added: *steps_added,
            },
            OrchestrationEvent::BudgetWarning {
                resource,
                utilization,
            } => DashboardEventKind::BudgetWarning {
                resource: resource.clone(),
                utilization: *utilization,
            },
            OrchestrationEvent::Stopped {
                reason,
                total_steps,
                total_tokens,
            } => DashboardEventKind::RunStopped {
                reason: reason.clone(),
                total_steps: *total_steps,
                total_tokens: *total_tokens,
            },
            OrchestrationEvent::PlanExploration {
                candidates,
                winner_strategy,
                winner_score,
            } => DashboardEventKind::PlanExploration {
                candidates: candidates.clone(),
                winner_strategy: winner_strategy.clone(),
                winner_score: *winner_score,
            },
            OrchestrationEvent::IndexProgress {
                files_done,
                symbols_so_far,
            } => DashboardEventKind::IndexProgress {
                files_done: *files_done,
                symbols_so_far: *symbols_so_far,
            },
            OrchestrationEvent::SubPlanStarted {
                parent_step_id,
                parent_step_name,
                sub_steps,
            } => DashboardEventKind::SubPlanStarted {
                parent_step_id: parent_step_id.to_string(),
                parent_step_name: parent_step_name.clone(),
                sub_steps: sub_steps
                    .iter()
                    .map(|(id, name)| SubStepSummary {
                        id: id.to_string(),
                        name: name.clone(),
                    })
                    .collect(),
            },
            OrchestrationEvent::SubStepProgress {
                parent_step_id,
                sub_step_id,
                sub_step_name,
                status,
            } => DashboardEventKind::SubStepProgress {
                parent_step_id: parent_step_id.to_string(),
                sub_step_id: sub_step_id.to_string(),
                sub_step_name: sub_step_name.clone(),
                status: status.clone(),
            },
            OrchestrationEvent::SubPlanCompleted {
                parent_step_id,
                parent_step_name,
                success,
            } => DashboardEventKind::SubPlanCompleted {
                parent_step_id: parent_step_id.to_string(),
                parent_step_name: parent_step_name.clone(),
                success: *success,
            },
            OrchestrationEvent::TeammateMessage {
                from_step_id,
                to_step_id,
                from_name,
                to_name,
                summary,
            } => DashboardEventKind::TeammateMessage {
                from_step_id: from_step_id.to_string(),
                to_step_id: to_step_id.to_string(),
                from_name: from_name.clone(),
                to_name: to_name.clone(),
                summary: summary.clone(),
            },
        }
    }
}

// ── Event stream builder (assigns seq + context) ───────────────

use std::sync::atomic::{AtomicU64, Ordering};

/// Assigns monotonic sequence numbers and context to raw events.
///
/// One `EventStream` per run. It converts `OrchestrationEvent` into
/// fully-wrapped `DashboardEvent` with seq, timestamps, and context.
pub struct EventStream {
    session_id: SessionId,
    run_id: Uuid,
    plan_version: AtomicU64,
    next_seq: AtomicU64,
}

impl EventStream {
    pub fn new(session_id: SessionId, run_id: Uuid) -> Self {
        Self {
            session_id,
            run_id,
            plan_version: AtomicU64::new(0),
            next_seq: AtomicU64::new(1),
        }
    }

    /// Wrap a raw OrchestrationEvent into a DashboardEvent.
    pub fn wrap(&self, event: &OrchestrationEvent) -> DashboardEvent {
        // Auto-increment plan_version on PlanGenerated events.
        if matches!(event, OrchestrationEvent::PlanGenerated { .. }) {
            self.plan_version.fetch_add(1, Ordering::SeqCst);
        }

        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let plan_version = self.plan_version.load(Ordering::SeqCst) as u32;
        let kind = DashboardEventKind::from_orchestration_event(event);

        DashboardEvent::new(seq, self.session_id, self.run_id, plan_version, kind)
    }

    /// Create a heartbeat event (for SSE keepalive).
    pub fn heartbeat(&self) -> DashboardEvent {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let plan_version = self.plan_version.load(Ordering::SeqCst) as u32;
        DashboardEvent::new(
            seq,
            self.session_id,
            self.run_id,
            plan_version,
            DashboardEventKind::Heartbeat,
        )
    }

    /// Current sequence number (for snapshot cursor).
    pub fn current_seq(&self) -> u64 {
        self.next_seq.load(Ordering::SeqCst) - 1
    }

    /// Current plan version.
    pub fn current_plan_version(&self) -> u32 {
        self.plan_version.load(Ordering::SeqCst) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_session() -> SessionId {
        SessionId(Uuid::nil())
    }

    fn test_run() -> Uuid {
        Uuid::nil()
    }

    #[test]
    fn seq_increments_monotonically() {
        let stream = EventStream::new(test_session(), test_run());
        let e1 = stream.wrap(&OrchestrationEvent::BudgetWarning {
            resource: "tokens".into(),
            utilization: 0.8,
        });
        let e2 = stream.wrap(&OrchestrationEvent::BudgetWarning {
            resource: "tokens".into(),
            utilization: 0.9,
        });
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);
    }

    #[test]
    fn plan_version_increments_on_plan_generated() {
        let stream = EventStream::new(test_session(), test_run());

        let e1 = stream.wrap(&OrchestrationEvent::BudgetWarning {
            resource: "tokens".into(),
            utilization: 0.5,
        });
        assert_eq!(e1.plan_version, 0);

        let e2 = stream.wrap(&OrchestrationEvent::PlanGenerated {
            plan_id: Uuid::new_v4(),
            step_count: 3,
            parallel_group_count: 2,
            critical_path_length: 2,
            estimated_total_tokens: 5000,
            strategy: "emergent".into(),
            team: None,
            steps: vec![],
        });
        assert_eq!(e2.plan_version, 1);

        // Subsequent events carry the new plan version.
        let e3 = stream.wrap(&OrchestrationEvent::BudgetWarning {
            resource: "tokens".into(),
            utilization: 0.6,
        });
        assert_eq!(e3.plan_version, 1);
    }

    #[test]
    fn heartbeat_increments_seq() {
        let stream = EventStream::new(test_session(), test_run());
        let e1 = stream.heartbeat();
        let e2 = stream.heartbeat();
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);
        assert!(matches!(e1.kind, DashboardEventKind::Heartbeat));
    }

    #[test]
    fn schema_version_is_current() {
        let stream = EventStream::new(test_session(), test_run());
        let event = stream.heartbeat();
        assert_eq!(event.schema_version, DASHBOARD_SCHEMA_VERSION);
    }

    #[test]
    fn session_and_run_ids_propagate() {
        let sid = SessionId::new();
        let rid = Uuid::new_v4();
        let stream = EventStream::new(sid, rid);
        let event = stream.heartbeat();
        assert_eq!(event.session_id, sid);
        assert_eq!(event.run_id, rid);
    }

    #[test]
    fn current_seq_returns_latest() {
        let stream = EventStream::new(test_session(), test_run());
        assert_eq!(stream.current_seq(), 0);
        stream.heartbeat();
        assert_eq!(stream.current_seq(), 1);
        stream.heartbeat();
        assert_eq!(stream.current_seq(), 2);
    }

    #[test]
    fn round_trip_serialization() {
        let stream = EventStream::new(test_session(), test_run());
        let event = stream.wrap(&OrchestrationEvent::Stopped {
            reason: StopReason::TaskComplete,
            total_steps: 5,
            total_tokens: 1000,
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: DashboardEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, event.seq);
        assert_eq!(parsed.schema_version, DASHBOARD_SCHEMA_VERSION);
        assert!(matches!(parsed.kind, DashboardEventKind::RunStopped { .. }));
    }

    #[test]
    fn flat_step_completed_conversion() {
        let stream = EventStream::new(test_session(), test_run());
        let event = stream.wrap(&OrchestrationEvent::StepCompleted {
            step: 1,
            action: crate::OrchestratorAction::Stop {
                reason: StopReason::TaskComplete,
            },
            reason: "done".into(),
            duration_ms: 100,
            budget_snapshot: crate::telemetry::BudgetSnapshot {
                tokens_used: 50,
                tokens_remaining: 950,
                tool_calls_used: 1,
                tool_calls_remaining: 9,
                retrievals_used: 0,
                verify_cycles_used: 0,
                elapsed_secs: 1,
            },
            knowledge_confidence: 0.9,
            success: true,
        });
        assert!(matches!(
            event.kind,
            DashboardEventKind::FlatStepCompleted { step: 1, .. }
        ));
    }

    #[test]
    fn all_orchestration_events_convert() {
        let stream = EventStream::new(test_session(), test_run());

        // Verify every variant converts without panic.
        let events = vec![
            OrchestrationEvent::RunStarted {
                provider: "stub".into(),
                model: "stub-dev".into(),
                request_summary: "test task".into(),
                complexity: "Medium".into(),
            },
            OrchestrationEvent::StepCompleted {
                step: 0,
                action: crate::OrchestratorAction::Stop {
                    reason: StopReason::TaskComplete,
                },
                reason: "r".into(),
                duration_ms: 0,
                budget_snapshot: crate::telemetry::BudgetSnapshot {
                    tokens_used: 0,
                    tokens_remaining: 0,
                    tool_calls_used: 0,
                    tool_calls_remaining: 0,
                    retrievals_used: 0,
                    verify_cycles_used: 0,
                    elapsed_secs: 0,
                },
                knowledge_confidence: 0.0,
                success: true,
            },
            OrchestrationEvent::PlanGenerated {
                plan_id: Uuid::nil(),
                step_count: 0,
                parallel_group_count: 0,
                critical_path_length: 0,
                estimated_total_tokens: 0,
                strategy: "s".into(),
                team: None,
                steps: vec![],
            },
            OrchestrationEvent::PlanStepStarted {
                step_id: Uuid::nil(),
                step_name: "s".into(),
                role: "r".into(),
                execution_mode: "e".into(),
            },
            OrchestrationEvent::PlanStepCompleted {
                step_id: Uuid::nil(),
                step_name: "s".into(),
                success: true,
                duration_ms: 0,
                tokens_used: 0,
            },
            OrchestrationEvent::PlanProgress {
                completed: 0,
                total: 0,
                active_steps: vec![],
                budget_used_pct: 0.0,
                tokens_used: 0,
                tokens_budget: 100_000,
            },
            OrchestrationEvent::VerifyGateResult {
                step_id: Uuid::nil(),
                step_name: "s".into(),
                checks: vec![],
                overall_passed: true,
                replan_triggered: false,
            },
            OrchestrationEvent::ReplanTriggered {
                failed_step_name: "s".into(),
                attempt: 1,
                max_attempts: 3,
                steps_preserved: 0,
                steps_removed: 0,
                steps_added: 0,
            },
            OrchestrationEvent::BudgetWarning {
                resource: "r".into(),
                utilization: 0.0,
            },
            OrchestrationEvent::Stopped {
                reason: StopReason::TaskComplete,
                total_steps: 0,
                total_tokens: 0,
            },
            OrchestrationEvent::PlanExploration {
                candidates: vec![],
                winner_strategy: "speed".into(),
                winner_score: 0.8,
            },
            OrchestrationEvent::IndexProgress {
                files_done: 0,
                symbols_so_far: 0,
            },
            OrchestrationEvent::SubPlanStarted {
                parent_step_id: Uuid::nil(),
                parent_step_name: "p".into(),
                sub_steps: vec![(Uuid::nil(), "sub".into())],
            },
            OrchestrationEvent::SubStepProgress {
                parent_step_id: Uuid::nil(),
                sub_step_id: Uuid::nil(),
                sub_step_name: "sub".into(),
                status: "running".into(),
            },
            OrchestrationEvent::SubPlanCompleted {
                parent_step_id: Uuid::nil(),
                parent_step_name: "p".into(),
                success: true,
            },
            OrchestrationEvent::TeammateMessage {
                from_step_id: Uuid::nil(),
                to_step_id: Uuid::nil(),
                from_name: "a".into(),
                to_name: "b".into(),
                summary: "sync".into(),
            },
        ];

        for (i, e) in events.iter().enumerate() {
            let wrapped = stream.wrap(e);
            assert_eq!(wrapped.seq, (i + 1) as u64, "seq mismatch for event {i}");
        }
    }
}
