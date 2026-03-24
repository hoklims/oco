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
