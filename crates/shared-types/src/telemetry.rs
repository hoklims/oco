use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{OrchestratorAction, SessionId, TaskComplexity};

/// A structured decision trace for one orchestration step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTrace {
    pub id: Uuid,
    pub session_id: SessionId,
    pub step: u32,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
    /// The action that was selected.
    pub action: OrchestratorAction,
    /// Why this action was chosen.
    pub reason: String,
    /// Task complexity assessment at this step.
    pub complexity: TaskComplexity,
    /// Confidence that the model can handle this step (0.0 to 1.0).
    pub knowledge_confidence: f64,
    /// Budget state at decision time.
    pub budget_snapshot: BudgetSnapshot,
    /// Context utilization at decision time.
    pub context_utilization: f64,
    /// Alternative actions considered (for auditability).
    pub alternatives_considered: Vec<ActionCandidate>,
    /// External session ID for correlation (e.g. Claude Code session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub tokens_used: u64,
    pub tokens_remaining: u64,
    pub tool_calls_used: u32,
    pub tool_calls_remaining: u32,
    pub retrievals_used: u32,
    pub verify_cycles_used: u32,
    pub elapsed_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCandidate {
    pub action_type: String,
    pub score: f64,
    pub reason: String,
}

/// Session-level telemetry summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTelemetry {
    pub session_id: SessionId,
    pub total_steps: u32,
    pub total_tokens: u64,
    pub total_tool_calls: u32,
    pub total_retrievals: u32,
    pub total_verify_cycles: u32,
    pub total_duration_ms: u64,
    pub outcome: String,
    pub traces: Vec<DecisionTrace>,
    /// v2: richer intervention tracking.
    #[serde(default)]
    pub events: Vec<TelemetryEvent>,
    /// v2: intervention effectiveness summary.
    #[serde(default)]
    pub intervention_summary: Option<InterventionSummary>,
}

/// v2: Fine-grained telemetry events for measurable decision tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: TelemetryEventType,
    /// Optional: was this intervention useful? Set after outcome is known.
    pub outcome: Option<InterventionOutcome>,
}

/// Types of telemetry events emitted by the v2 system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TelemetryEventType {
    /// A hook was triggered (pre/post tool use, stop, etc.).
    HookTriggered {
        hook_name: String,
        tool_name: Option<String>,
    },
    /// A skill was invoked.
    SkillInvoked { skill_name: String },
    /// A subagent was launched.
    SubagentLaunched {
        agent_type: String,
        task_description: String,
    },
    /// Verification was run with result.
    VerifyCompleted {
        strategy: String,
        passed: bool,
        duration_ms: u64,
    },
    /// Context was assembled for an LLM call.
    ContextAssembled {
        total_tokens: u32,
        item_count: u32,
        excluded_count: u32,
        utilization: f64,
    },
    /// Working memory was updated.
    MemoryUpdated {
        operation: String,
        active_count: usize,
    },
    /// Verification staleness detected.
    VerificationStale { stale_files: Vec<String> },
    /// Budget threshold crossed.
    BudgetThreshold {
        resource: String,
        utilization: f64,
        status: String,
    },
}

/// Whether an intervention was useful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionOutcome {
    /// The intervention contributed to a successful outcome.
    Useful,
    /// The intervention was redundant or had no effect.
    Redundant,
    /// The intervention was counterproductive.
    Harmful,
    /// Not yet determined.
    Unknown,
}

/// Summary of intervention effectiveness for a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterventionSummary {
    pub total_interventions: u32,
    pub useful: u32,
    pub redundant: u32,
    pub harmful: u32,
    pub unknown: u32,
}

// ── Live orchestration events ─────────────────────────────

/// Summary of a single step for plan overview display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub role: String,
    pub execution_mode: String,
    pub depends_on: Vec<Uuid>,
    pub verify_after: bool,
    pub estimated_tokens: u32,
    pub preferred_model: Option<String>,
}

/// Summary of a team configuration for plan overview display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSummary {
    pub name: String,
    pub topology: String,
    pub member_count: usize,
}

/// Result of a single verification check within a verify gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_type: String,
    pub passed: bool,
    pub summary: String,
}

/// Summary of a plan candidate from competitive planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCandidateSummary {
    pub strategy: String,
    pub step_count: usize,
    pub estimated_tokens: u64,
    pub verify_count: usize,
    pub parallel_groups: usize,
    pub score: f64,
    pub winner: bool,
}

/// Events emitted by the orchestration loop in real time via channel.
/// Decoupled from UI — the CLI converts these to UiEvents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OrchestrationEvent {
    /// The orchestration run started.
    RunStarted {
        provider: String,
        model: String,
        request_summary: String,
        complexity: String,
    },

    /// A step was completed (action executed + trace recorded).
    StepCompleted {
        step: u32,
        action: crate::OrchestratorAction,
        reason: String,
        duration_ms: u64,
        budget_snapshot: BudgetSnapshot,
        knowledge_confidence: f64,
        success: bool,
    },

    /// An execution plan was generated (or regenerated after replan).
    PlanGenerated {
        plan_id: Uuid,
        step_count: usize,
        parallel_group_count: usize,
        critical_path_length: u32,
        estimated_total_tokens: u64,
        strategy: String,
        team: Option<TeamSummary>,
        steps: Vec<StepSummary>,
    },

    /// A plan step started executing.
    PlanStepStarted {
        step_id: Uuid,
        step_name: String,
        role: String,
        execution_mode: String,
    },

    /// A plan step completed (success or failure).
    PlanStepCompleted {
        step_id: Uuid,
        step_name: String,
        success: bool,
        duration_ms: u64,
        tokens_used: u64,
    },

    /// Live progress update during plan execution.
    PlanProgress {
        completed: usize,
        total: usize,
        active_steps: Vec<(Uuid, String)>,
        budget_used_pct: f32,
    },

    /// A verify gate was evaluated after a step.
    VerifyGateResult {
        step_id: Uuid,
        step_name: String,
        checks: Vec<CheckResult>,
        overall_passed: bool,
        replan_triggered: bool,
    },

    /// Replanning was triggered after a verification failure.
    ReplanTriggered {
        failed_step_name: String,
        attempt: u32,
        max_attempts: u32,
        steps_preserved: usize,
        steps_removed: usize,
        steps_added: usize,
    },

    /// Budget crossed a warning threshold.
    BudgetWarning { resource: String, utilization: f64 },
    /// Competitive planning: multiple candidates were explored and scored.
    PlanExploration {
        candidates: Vec<PlanCandidateSummary>,
        winner_strategy: String,
        winner_score: f64,
    },
    /// A sub-plan started executing for a parent step (ADR-008).
    SubPlanStarted {
        parent_step_id: Uuid,
        parent_step_name: String,
        sub_steps: Vec<(Uuid, String)>,
    },
    /// Progress update for a sub-plan step.
    SubStepProgress {
        parent_step_id: Uuid,
        sub_step_id: Uuid,
        sub_step_name: String,
        status: String,
    },
    /// A sub-plan completed execution.
    SubPlanCompleted {
        parent_step_id: Uuid,
        parent_step_name: String,
        success: bool,
    },
    /// A teammate sent a message to another teammate.
    TeammateMessage {
        from_step_id: Uuid,
        to_step_id: Uuid,
        from_name: String,
        to_name: String,
        summary: String,
    },
    /// The orchestration loop stopped.
    Stopped {
        reason: crate::StopReason,
        total_steps: u32,
        total_tokens: u64,
    },
    /// Indexing progress (file-by-file).
    IndexProgress {
        files_done: u32,
        symbols_so_far: u32,
    },
}

// ── Q3: Run Summary & Trust Verdict ──────────────────────

/// A review-friendly summary of an orchestration run.
///
/// Designed to answer: what changed? what was verified? what's risky?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub session_id: crate::SessionId,
    pub request: String,
    pub complexity: crate::TaskComplexity,
    pub policy_pack: crate::PolicyPack,
    pub total_steps: u32,
    pub total_tokens: u64,
    pub total_duration_ms: u64,
    pub files_modified: Vec<String>,
    pub files_verified: Vec<String>,
    pub files_unverified: Vec<String>,
    pub verification_freshness: crate::VerificationFreshness,
    pub checks_run: Vec<RunCheckSummary>,
    pub replans: u32,
    pub key_decisions: Vec<String>,
    pub trust_verdict: TrustVerdict,
    pub risks: Vec<String>,
}

/// Summary of a single verification check within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCheckSummary {
    pub strategy: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub mandatory: bool,
}

/// Composite trust verdict for a run.
///
/// Answers: should a reviewer trust this run's output?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustVerdict {
    /// All mandatory checks passed, verification is fresh, no unverified files.
    High,
    /// Some checks passed, minor gaps remain (e.g. partial freshness).
    Medium,
    /// Significant gaps: stale verification, failed mandatory checks, or high risk.
    Low,
    /// No verification at all, or critical failures.
    None,
}

impl TrustVerdict {
    /// Compute verdict from verification state, policy pack, and check results.
    pub fn compute(
        freshness: crate::VerificationFreshness,
        all_mandatory_passed: bool,
        has_unverified_sensitive: bool,
    ) -> Self {
        use crate::VerificationFreshness;
        match (freshness, all_mandatory_passed, has_unverified_sensitive) {
            (VerificationFreshness::Fresh, true, false) => Self::High,
            (VerificationFreshness::Fresh, true, true) => Self::Medium,
            (VerificationFreshness::Partial, true, _) => Self::Medium,
            (VerificationFreshness::Stale, _, _) => Self::Low,
            (_, false, _) => Self::Low,
            (VerificationFreshness::None, _, _) => Self::None,
        }
    }

    /// Human-readable label for display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
        }
    }

    /// Emoji-free symbol for terminal display.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::High => "[PASS]",
            Self::Medium => "[WARN]",
            Self::Low => "[RISK]",
            Self::None => "[NONE]",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_trace_without_external_id_omits_field() {
        let trace = DecisionTrace {
            id: uuid::Uuid::nil(),
            session_id: crate::SessionId::new(),
            step: 0,
            timestamp: chrono::Utc::now(),
            duration_ms: 10,
            action: crate::OrchestratorAction::Stop {
                reason: crate::StopReason::TaskComplete,
            },
            reason: "test".into(),
            complexity: crate::TaskComplexity::Trivial,
            knowledge_confidence: 0.5,
            budget_snapshot: BudgetSnapshot {
                tokens_used: 0,
                tokens_remaining: 1000,
                tool_calls_used: 0,
                tool_calls_remaining: 10,
                retrievals_used: 0,
                verify_cycles_used: 0,
                elapsed_secs: 0,
            },
            context_utilization: 0.0,
            alternatives_considered: vec![],
            external_session_id: None,
        };
        let json = serde_json::to_string(&trace).unwrap();
        assert!(!json.contains("external_session_id"));
    }

    #[test]
    fn decision_trace_with_external_id_serializes() {
        let trace = DecisionTrace {
            id: uuid::Uuid::nil(),
            session_id: crate::SessionId::new(),
            step: 0,
            timestamp: chrono::Utc::now(),
            duration_ms: 10,
            action: crate::OrchestratorAction::Stop {
                reason: crate::StopReason::TaskComplete,
            },
            reason: "test".into(),
            complexity: crate::TaskComplexity::Trivial,
            knowledge_confidence: 0.5,
            budget_snapshot: BudgetSnapshot {
                tokens_used: 0,
                tokens_remaining: 1000,
                tool_calls_used: 0,
                tool_calls_remaining: 10,
                retrievals_used: 0,
                verify_cycles_used: 0,
                elapsed_secs: 0,
            },
            context_utilization: 0.0,
            alternatives_considered: vec![],
            external_session_id: Some("claude-xyz".into()),
        };
        let json = serde_json::to_string(&trace).unwrap();
        assert!(json.contains("\"external_session_id\":\"claude-xyz\""));
    }

    // ── TrustVerdict tests ──

    #[test]
    fn trust_verdict_high_when_fresh_and_all_pass() {
        let v = TrustVerdict::compute(crate::VerificationFreshness::Fresh, true, false);
        assert_eq!(v, TrustVerdict::High);
    }

    #[test]
    fn trust_verdict_medium_when_fresh_but_sensitive_unverified() {
        let v = TrustVerdict::compute(crate::VerificationFreshness::Fresh, true, true);
        assert_eq!(v, TrustVerdict::Medium);
    }

    #[test]
    fn trust_verdict_medium_when_partial() {
        let v = TrustVerdict::compute(crate::VerificationFreshness::Partial, true, false);
        assert_eq!(v, TrustVerdict::Medium);
    }

    #[test]
    fn trust_verdict_low_when_stale() {
        let v = TrustVerdict::compute(crate::VerificationFreshness::Stale, true, false);
        assert_eq!(v, TrustVerdict::Low);
    }

    #[test]
    fn trust_verdict_low_when_mandatory_failed() {
        let v = TrustVerdict::compute(crate::VerificationFreshness::Fresh, false, false);
        assert_eq!(v, TrustVerdict::Low);
    }

    #[test]
    fn trust_verdict_none_when_no_verification() {
        let v = TrustVerdict::compute(crate::VerificationFreshness::None, true, false);
        assert_eq!(v, TrustVerdict::None);
    }

    #[test]
    fn trust_verdict_labels() {
        assert_eq!(TrustVerdict::High.label(), "high");
        assert_eq!(TrustVerdict::Medium.label(), "medium");
        assert_eq!(TrustVerdict::Low.label(), "low");
        assert_eq!(TrustVerdict::None.label(), "none");
    }

    #[test]
    fn trust_verdict_serde_round_trip() {
        for verdict in [
            TrustVerdict::High,
            TrustVerdict::Medium,
            TrustVerdict::Low,
            TrustVerdict::None,
        ] {
            let json = serde_json::to_string(&verdict).unwrap();
            let parsed: TrustVerdict = serde_json::from_str(&json).unwrap();
            assert_eq!(verdict, parsed);
        }
    }

    #[test]
    fn run_summary_serializes() {
        let summary = RunSummary {
            session_id: crate::SessionId::new(),
            request: "fix auth bug".into(),
            complexity: crate::TaskComplexity::Medium,
            policy_pack: crate::PolicyPack::Balanced,
            total_steps: 5,
            total_tokens: 10000,
            total_duration_ms: 30000,
            files_modified: vec!["src/auth.rs".into()],
            files_verified: vec!["src/auth.rs".into()],
            files_unverified: vec![],
            verification_freshness: crate::VerificationFreshness::Fresh,
            checks_run: vec![RunCheckSummary {
                strategy: "test".into(),
                passed: true,
                duration_ms: 5000,
                mandatory: true,
            }],
            replans: 0,
            key_decisions: vec!["chose direct fix over refactor".into()],
            trust_verdict: TrustVerdict::High,
            risks: vec![],
        };
        let json = serde_json::to_string_pretty(&summary).unwrap();
        assert!(json.contains("\"trust_verdict\": \"high\""));
        assert!(json.contains("\"policy_pack\": \"balanced\""));
    }
}
