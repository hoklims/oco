//! Execution lease, task packet, and step contract types for OCO-as-Runtime.
//!
//! These types enable Claude Code to delegate structured work to OCO
//! instead of calling individual tools. The delegation model is:
//!
//! 1. **ExecutionLease** (#59) — a bounded contract for delegated work
//! 2. **TaskPacket** (#60) — compiled task with constraints and forbidden shortcuts
//! 3. **StepContract** (#61) — per-step input/output contracts with transition guards

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{TaskCategory, TaskComplexity};

// ---------------------------------------------------------------------------
// ExecutionLease — bounded delegation contract (#59)
// ---------------------------------------------------------------------------

/// A lease that Claude Code grants to OCO for executing a task.
/// The lease bounds what OCO can do (budget, permissions, tools)
/// and defines what it must return (patch, trace, verification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLease {
    /// Unique lease identifier.
    pub id: Uuid,
    /// What needs to be done — the user's intent.
    pub task: String,
    /// Execution mode.
    pub mode: LeaseMode,
    /// Constraints on what OCO can do under this lease.
    pub constraints: LeaseConstraints,
    /// What OCO must return when done.
    pub return_mode: ReturnMode,
    /// Current state of the lease.
    pub status: LeaseStatus,
}

impl ExecutionLease {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            task: task.into(),
            mode: LeaseMode::Delegated,
            constraints: LeaseConstraints::default(),
            return_mode: ReturnMode::Full,
            status: LeaseStatus::Active,
        }
    }

    pub fn with_mode(mut self, mode: LeaseMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_constraints(mut self, constraints: LeaseConstraints) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn with_return_mode(mut self, mode: ReturnMode) -> Self {
        self.return_mode = mode;
        self
    }

    pub fn is_active(&self) -> bool {
        self.status == LeaseStatus::Active
    }
}

/// How OCO operates under the lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LeaseMode {
    /// OCO plans and executes autonomously within constraints.
    Delegated,
    /// OCO plans but asks for confirmation before execution.
    PlanOnly,
    /// OCO executes a pre-provided plan.
    ExecuteOnly,
}

/// Bounds on what OCO can do under a lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseConstraints {
    /// Maximum number of plan steps.
    pub max_steps: u32,
    /// Whether verification is required before completion.
    pub verify_required: bool,
    /// Whether file writes are permitted.
    pub allow_file_writes: bool,
    /// Token budget for this lease (overrides session default if set).
    pub token_budget: Option<u64>,
    /// Maximum wall-clock duration in seconds.
    pub max_duration_secs: Option<u64>,
}

impl Default for LeaseConstraints {
    fn default() -> Self {
        Self {
            max_steps: 8,
            verify_required: true,
            allow_file_writes: true,
            token_budget: None,
            max_duration_secs: None,
        }
    }
}

/// What OCO returns when the lease completes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReturnMode {
    /// Return everything: patch summary + trace + verification result.
    Full,
    /// Return only the final response text.
    ResponseOnly,
    /// Return patch diff + verification result (no trace).
    PatchAndVerify,
}

/// Lifecycle of an execution lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LeaseStatus {
    /// Lease is active — OCO is working.
    Active,
    /// OCO completed the task successfully.
    Completed,
    /// OCO failed the task.
    Failed { reason: String },
    /// Lease was revoked by the caller.
    Revoked,
    /// Lease expired (budget/time exceeded).
    Expired,
}

/// Structured result returned when a lease completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseResult {
    /// The lease this result belongs to.
    pub lease_id: Uuid,
    /// Final response text (always present).
    pub response: String,
    /// Trace of steps executed (if return mode includes it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<LeaseTraceEntry>,
    /// Verification result (if verify_required was true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<LeaseVerification>,
    /// Files modified during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    /// Token usage.
    pub tokens_used: u64,
}

/// A single trace entry in a lease execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseTraceEntry {
    pub step_name: String,
    pub action: String,
    pub success: bool,
    pub duration_ms: u64,
}

/// Verification outcome of a lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseVerification {
    pub passed: bool,
    pub checks_run: Vec<String>,
    pub failures: Vec<String>,
}

// ---------------------------------------------------------------------------
// TaskPacket — compiled task with constraints (#60)
// ---------------------------------------------------------------------------

/// A task compiled by OCO from a raw user request.
/// This is the "intermediate representation" that structures Claude Code's work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPacket {
    /// Unique packet identifier.
    pub id: Uuid,
    /// Detected intent.
    pub intent: String,
    /// Classified complexity.
    pub complexity: TaskComplexity,
    /// Classified category.
    pub category: TaskCategory,
    /// Risk level (0.0 = safe, 1.0 = critical).
    pub risk: f64,
    /// Repo zones that are relevant.
    pub repo_zones: Vec<String>,
    /// Capabilities required to complete the task.
    pub required_capabilities: Vec<String>,
    /// Execution contract (what must happen for completion).
    pub execution_contract: ExecutionContract,
    /// Shortcuts that the agent must NOT take.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_shortcuts: Vec<ForbiddenShortcut>,
    /// Recommended team topology (if multi-agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_topology: Option<String>,
}

impl TaskPacket {
    pub fn new(intent: impl Into<String>, complexity: TaskComplexity, category: TaskCategory) -> Self {
        Self {
            id: Uuid::new_v4(),
            intent: intent.into(),
            complexity,
            category,
            risk: 0.0,
            repo_zones: Vec::new(),
            required_capabilities: Vec::new(),
            execution_contract: ExecutionContract::default(),
            forbidden_shortcuts: Vec::new(),
            recommended_topology: None,
        }
    }

    pub fn with_risk(mut self, risk: f64) -> Self {
        self.risk = risk.clamp(0.0, 1.0);
        self
    }

    pub fn with_zones(mut self, zones: Vec<String>) -> Self {
        self.repo_zones = zones;
        self
    }

    pub fn with_forbidden(mut self, shortcut: ForbiddenShortcut) -> Self {
        self.forbidden_shortcuts.push(shortcut);
        self
    }
}

/// What must happen for a task to be considered complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContract {
    /// Whether a verify gate is mandatory.
    pub verify_gate: VerifyGatePolicy,
    /// Conditions that must be satisfied before completion.
    pub completion_requires: Vec<String>,
    /// Maximum replan attempts.
    pub max_replan_attempts: u32,
}

impl Default for ExecutionContract {
    fn default() -> Self {
        Self {
            verify_gate: VerifyGatePolicy::WhenModified,
            completion_requires: Vec::new(),
            max_replan_attempts: 3,
        }
    }
}

/// When verification gates are enforced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifyGatePolicy {
    /// Always verify before completion.
    Mandatory,
    /// Verify only if files were modified.
    WhenModified,
    /// Never verify (read-only tasks).
    Never,
}

/// Something the agent must NOT do — enforced by OCO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForbiddenShortcut {
    /// What is forbidden (machine-readable identifier).
    pub id: String,
    /// Human-readable description of why it's forbidden.
    pub reason: String,
}

impl ForbiddenShortcut {
    pub fn new(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            reason: reason.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// StepContract — per-step input/output contracts (#61)
// ---------------------------------------------------------------------------

/// A contract that defines what a plan step must receive and produce.
/// The GraphRunner validates conformity before allowing step transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepContract {
    /// Inputs required before the step can start.
    pub requires_inputs: Vec<ContractField>,
    /// Outputs the step must produce.
    pub requires_outputs: Vec<ContractField>,
    /// Transitions that are blocked until conditions are met.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transition_guards: Vec<TransitionGuard>,
}

impl StepContract {
    pub fn new() -> Self {
        Self {
            requires_inputs: Vec::new(),
            requires_outputs: Vec::new(),
            transition_guards: Vec::new(),
        }
    }

    pub fn with_input(mut self, field: ContractField) -> Self {
        self.requires_inputs.push(field);
        self
    }

    pub fn with_output(mut self, field: ContractField) -> Self {
        self.requires_outputs.push(field);
        self
    }

    pub fn with_guard(mut self, guard: TransitionGuard) -> Self {
        self.transition_guards.push(guard);
        self
    }

    /// Check if all required outputs are present in the step's output.
    /// Tries JSON key lookup first; falls back to word-boundary matching.
    pub fn validate_outputs(&self, output: &str) -> ContractValidation {
        let json_keys: Option<Vec<String>> = serde_json::from_str::<serde_json::Value>(output)
            .ok()
            .and_then(|v| v.as_object().map(|m| m.keys().cloned().collect()));

        let mut missing = Vec::new();
        for field in &self.requires_outputs {
            if !field.required {
                continue;
            }
            let found = if let Some(ref keys) = json_keys {
                // Structured output: exact key match
                keys.iter().any(|k| k == &field.name)
            } else {
                // Unstructured output: exact token match (split on non-alphanumeric boundaries)
                output.split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|token| token == field.name)
            };
            if !found {
                missing.push(field.name.clone());
            }
        }
        if missing.is_empty() {
            ContractValidation::Satisfied
        } else {
            ContractValidation::Violated { missing_fields: missing }
        }
    }

    /// Check if all transition guards allow the target status.
    pub fn can_transition_to(&self, target: &str, context: &ContractContext) -> bool {
        self.transition_guards.iter().all(|g| g.allows(target, context))
    }
}

impl Default for StepContract {
    fn default() -> Self {
        Self::new()
    }
}

/// A named field in a step contract (input or output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractField {
    /// Field name (e.g., "impacted_symbols", "patch_summary").
    pub name: String,
    /// Whether this field is required (default: true).
    #[serde(default = "default_true")]
    pub required: bool,
    /// Description of what this field contains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_true() -> bool {
    true
}

impl ContractField {
    pub fn required(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            required: true,
            description: None,
        }
    }

    pub fn optional(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            required: false,
            description: None,
        }
    }
}

/// A guard that blocks state transitions unless conditions are met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionGuard {
    /// The target status that requires this guard (e.g., "completed").
    pub target_status: String,
    /// The condition that must be true to allow the transition.
    pub condition: GuardCondition,
}

impl TransitionGuard {
    /// Create a guard that blocks transition to "completed" unless verified.
    pub fn verify_before_complete() -> Self {
        Self {
            target_status: "completed".into(),
            condition: GuardCondition::VerifyGatePassed,
        }
    }

    /// Check if this guard allows the transition.
    pub fn allows(&self, target: &str, context: &ContractContext) -> bool {
        if target != self.target_status {
            return true; // guard doesn't apply to this transition
        }
        match &self.condition {
            GuardCondition::VerifyGatePassed => context.verify_passed,
            GuardCondition::OutputFieldPresent { field } => context.output_fields.contains(field),
            GuardCondition::Custom { key } => context.custom_flags.contains(key),
        }
    }
}

/// Conditions that can gate a step transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuardCondition {
    /// Verify gate must have passed.
    VerifyGatePassed,
    /// A specific output field must be present.
    OutputFieldPresent { field: String },
    /// A custom flag must be set in the context.
    Custom { key: String },
}

/// Runtime context for evaluating transition guards.
#[derive(Debug, Clone, Default)]
pub struct ContractContext {
    /// Whether the verify gate has passed.
    pub verify_passed: bool,
    /// Output fields that have been produced.
    pub output_fields: Vec<String>,
    /// Custom flags set by the execution engine.
    pub custom_flags: Vec<String>,
}

/// Result of contract validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractValidation {
    /// All contract requirements are satisfied.
    Satisfied,
    /// Contract requirements are not met.
    Violated { missing_fields: Vec<String> },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TaskCategory, TaskComplexity};

    // -- ExecutionLease tests --

    #[test]
    fn lease_default_constraints() {
        let lease = ExecutionLease::new("fix the auth bug");
        assert!(lease.is_active());
        assert_eq!(lease.constraints.max_steps, 8);
        assert!(lease.constraints.verify_required);
        assert!(lease.constraints.allow_file_writes);
        assert_eq!(lease.mode, LeaseMode::Delegated);
    }

    #[test]
    fn lease_plan_only_mode() {
        let lease = ExecutionLease::new("refactor auth")
            .with_mode(LeaseMode::PlanOnly)
            .with_constraints(LeaseConstraints {
                max_steps: 4,
                verify_required: false,
                allow_file_writes: false,
                token_budget: Some(50_000),
                max_duration_secs: Some(120),
            });
        assert_eq!(lease.mode, LeaseMode::PlanOnly);
        assert_eq!(lease.constraints.max_steps, 4);
        assert!(!lease.constraints.allow_file_writes);
        assert_eq!(lease.constraints.token_budget, Some(50_000));
    }

    #[test]
    fn lease_status_transitions() {
        let mut lease = ExecutionLease::new("task");
        assert!(lease.is_active());

        lease.status = LeaseStatus::Completed;
        assert!(!lease.is_active());

        lease.status = LeaseStatus::Failed {
            reason: "timeout".into(),
        };
        assert!(!lease.is_active());
    }

    #[test]
    fn lease_serialization_roundtrip() {
        let lease = ExecutionLease::new("test task")
            .with_mode(LeaseMode::Delegated)
            .with_return_mode(ReturnMode::PatchAndVerify);
        let json = serde_json::to_string(&lease).unwrap();
        let parsed: ExecutionLease = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task, "test task");
        assert_eq!(parsed.mode, LeaseMode::Delegated);
        assert_eq!(parsed.return_mode, ReturnMode::PatchAndVerify);
    }

    // -- TaskPacket tests --

    #[test]
    fn task_packet_construction() {
        let packet = TaskPacket::new("safe_refactor", TaskComplexity::Medium, TaskCategory::Refactor)
            .with_risk(0.7)
            .with_zones(vec!["api/auth".into(), "shared/session".into()])
            .with_forbidden(ForbiddenShortcut::new(
                "skip_impact_scan",
                "must scan impact before modifying auth contract",
            ));

        assert_eq!(packet.intent, "safe_refactor");
        assert_eq!(packet.complexity, TaskComplexity::Medium);
        assert_eq!(packet.risk, 0.7);
        assert_eq!(packet.repo_zones.len(), 2);
        assert_eq!(packet.forbidden_shortcuts.len(), 1);
        assert_eq!(packet.forbidden_shortcuts[0].id, "skip_impact_scan");
    }

    #[test]
    fn task_packet_risk_clamped() {
        let packet = TaskPacket::new("test", TaskComplexity::Low, TaskCategory::General)
            .with_risk(1.5);
        assert_eq!(packet.risk, 1.0);

        let packet2 = TaskPacket::new("test", TaskComplexity::Low, TaskCategory::General)
            .with_risk(-0.5);
        assert_eq!(packet2.risk, 0.0);
    }

    #[test]
    fn execution_contract_default() {
        let contract = ExecutionContract::default();
        assert_eq!(contract.verify_gate, VerifyGatePolicy::WhenModified);
        assert!(contract.completion_requires.is_empty());
        assert_eq!(contract.max_replan_attempts, 3);
    }

    #[test]
    fn task_packet_serialization_roundtrip() {
        let packet = TaskPacket::new("investigate_bug", TaskComplexity::High, TaskCategory::Bug)
            .with_risk(0.8)
            .with_zones(vec!["core/engine".into()]);
        let json = serde_json::to_string(&packet).unwrap();
        let parsed: TaskPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.intent, "investigate_bug");
        assert_eq!(parsed.risk, 0.8);
        assert_eq!(parsed.repo_zones, vec!["core/engine"]);
    }

    // -- StepContract tests --

    #[test]
    fn step_contract_builder() {
        let contract = StepContract::new()
            .with_input(ContractField::required("impacted_symbols"))
            .with_input(ContractField::required("target_files"))
            .with_output(ContractField::required("patch_summary"))
            .with_output(ContractField::optional("test_results"))
            .with_guard(TransitionGuard::verify_before_complete());

        assert_eq!(contract.requires_inputs.len(), 2);
        assert_eq!(contract.requires_outputs.len(), 2);
        assert_eq!(contract.transition_guards.len(), 1);
    }

    #[test]
    fn contract_validates_outputs() {
        let contract = StepContract::new()
            .with_output(ContractField::required("patch_summary"))
            .with_output(ContractField::required("verification_plan"));

        // Output contains both fields
        let result = contract.validate_outputs("patch_summary: changed 3 files\nverification_plan: run cargo test");
        assert_eq!(result, ContractValidation::Satisfied);

        // Output missing one field
        let result = contract.validate_outputs("patch_summary: changed 3 files");
        assert!(matches!(result, ContractValidation::Violated { missing_fields } if missing_fields == vec!["verification_plan"]));
    }

    #[test]
    fn transition_guard_verify_gate() {
        let guard = TransitionGuard::verify_before_complete();

        // Without verification
        let ctx = ContractContext::default();
        assert!(!guard.allows("completed", &ctx));

        // With verification
        let ctx = ContractContext {
            verify_passed: true,
            ..Default::default()
        };
        assert!(guard.allows("completed", &ctx));

        // Guard doesn't apply to other transitions
        assert!(guard.allows("failed", &ctx));
    }

    #[test]
    fn transition_guard_output_field() {
        let guard = TransitionGuard {
            target_status: "completed".into(),
            condition: GuardCondition::OutputFieldPresent {
                field: "test_results".into(),
            },
        };

        let ctx = ContractContext {
            output_fields: vec!["patch_summary".into()],
            ..Default::default()
        };
        assert!(!guard.allows("completed", &ctx));

        let ctx = ContractContext {
            output_fields: vec!["patch_summary".into(), "test_results".into()],
            ..Default::default()
        };
        assert!(guard.allows("completed", &ctx));
    }

    #[test]
    fn contract_can_transition_with_multiple_guards() {
        let contract = StepContract::new()
            .with_guard(TransitionGuard::verify_before_complete())
            .with_guard(TransitionGuard {
                target_status: "completed".into(),
                condition: GuardCondition::OutputFieldPresent {
                    field: "patch_summary".into(),
                },
            });

        // Both guards must pass
        let ctx = ContractContext {
            verify_passed: true,
            output_fields: vec!["patch_summary".into()],
            ..Default::default()
        };
        assert!(contract.can_transition_to("completed", &ctx));

        // Only one passes
        let ctx = ContractContext {
            verify_passed: true,
            output_fields: vec![],
            ..Default::default()
        };
        assert!(!contract.can_transition_to("completed", &ctx));
    }

    #[test]
    fn step_contract_serialization_roundtrip() {
        let contract = StepContract::new()
            .with_input(ContractField::required("symbols"))
            .with_output(ContractField::required("patch"))
            .with_guard(TransitionGuard::verify_before_complete());

        let json = serde_json::to_string(&contract).unwrap();
        let parsed: StepContract = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.requires_inputs.len(), 1);
        assert_eq!(parsed.requires_outputs.len(), 1);
        assert_eq!(parsed.transition_guards.len(), 1);
    }
}
