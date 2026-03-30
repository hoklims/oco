//! Execution plan types for emergent orchestration.
//!
//! An `ExecutionPlan` is a DAG of `PlanStep`s generated per-request by the Planner.
//! Each step declares its agent role, allowed tools, execution strategy, and dependencies.
//! The `GraphRunner` walks the DAG, executing ready steps (possibly in parallel) and
//! replanning on verification failures.
//!
//! **Design principles:**
//! - Each plan is unique — emergent from task + repo context + available capabilities.
//! - Steps declare *what* to do; the runner decides *how* (inline, subagent, teammate).
//! - Native support for Claude Code Agent Teams (Mesh communication) and Subagents (HubSpoke).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use uuid::Uuid;

use crate::TaskComplexity;
use crate::agent::AgentId;

// ---------------------------------------------------------------------------
// PlanStep — a node in the execution DAG
// ---------------------------------------------------------------------------

/// A single step in the execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Unique step identifier.
    pub id: Uuid,
    /// Short human-readable name (e.g., "investigate-auth", "implement-middleware").
    pub name: String,
    /// What this step should accomplish.
    pub description: String,
    /// Which kind of agent should execute this step.
    pub agent_role: AgentRole,
    /// Tools this step is allowed to use (empty = inherit all from agent role).
    pub allowed_tools: Vec<String>,
    /// Steps that must complete before this one can start (DAG edges).
    pub depends_on: Vec<Uuid>,
    /// How to execute this step.
    pub execution: StepExecution,
    /// Run verification after this step completes?
    pub verify_after: bool,
    /// Current lifecycle status.
    pub status: StepStatus,
    /// Estimated token cost for budget planning.
    pub estimated_tokens: u32,
    /// Actual output produced when completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Step contract defining required inputs/outputs and transition guards (#61).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<crate::lease::StepContract>,
    /// Optional nested execution plan for Subagent/Teammate steps (ADR-008).
    /// The sub-plan is executed recursively by GraphRunner.
    /// Max nesting depth: 2 (enforced at validation time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_plan: Option<Box<ExecutionPlan>>,
}

impl PlanStep {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            agent_role: AgentRole::default(),
            allowed_tools: Vec::new(),
            depends_on: Vec::new(),
            execution: StepExecution::Inline,
            verify_after: false,
            status: StepStatus::Pending,
            estimated_tokens: 0,
            output: None,
            contract: None,
            sub_plan: None,
        }
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.agent_role = role;
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    pub fn with_depends_on(mut self, deps: Vec<Uuid>) -> Self {
        self.depends_on = deps;
        self
    }

    pub fn with_execution(mut self, exec: StepExecution) -> Self {
        self.execution = exec;
        self
    }

    pub fn with_verify(mut self) -> Self {
        self.verify_after = true;
        self
    }

    pub fn with_estimated_tokens(mut self, tokens: u32) -> Self {
        self.estimated_tokens = tokens;
        self
    }

    pub fn with_contract(mut self, contract: crate::lease::StepContract) -> Self {
        self.contract = Some(contract);
        self
    }

    /// Attach a nested sub-plan to this step (ADR-008).
    /// Only valid for Subagent or Teammate execution modes.
    pub fn with_sub_plan(mut self, plan: ExecutionPlan) -> Self {
        self.sub_plan = Some(Box::new(plan));
        self
    }

    /// Whether this step is ready to execute (all dependencies completed).
    pub fn is_ready(&self, completed: &HashSet<Uuid>) -> bool {
        self.status == StepStatus::Pending && self.depends_on.iter().all(|d| completed.contains(d))
    }

    /// Whether this step has finished (successfully or not).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            StepStatus::Completed | StepStatus::Failed { .. } | StepStatus::Replanned
        )
    }
}

impl fmt::Display for PlanStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format: "name[status](role, execution)"
        // e.g. "investigate[pending](investigator, inline)"
        //      "implement[in-progress](coder, subagent(sonnet))"
        write!(
            f,
            "{}[{}]({}, {})",
            self.name, self.status, self.agent_role.name, self.execution
        )?;
        if self.verify_after {
            f.write_str(" +verify")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// StepExecution — how a step is executed
// ---------------------------------------------------------------------------

/// Execution strategy for a plan step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum StepExecution {
    /// Run in the main orchestration loop (existing behavior).
    Inline,
    /// Spawn an isolated subagent (Claude Code subagent model — hub-spoke).
    Subagent {
        /// Model override: "opus", "sonnet", "haiku", or None for default.
        model: Option<String>,
    },
    /// Spawn as a teammate (Claude Code Agent Teams — mesh communication).
    Teammate {
        /// Team this agent belongs to.
        team_name: String,
    },
    /// Delegate to an MCP tool directly.
    McpTool {
        /// MCP server identifier.
        server: String,
        /// Tool name on that server.
        tool: String,
    },
}

impl fmt::Display for StepExecution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inline => f.write_str("inline"),
            Self::Subagent { model } => match model {
                Some(m) => write!(f, "subagent({m})"),
                None => f.write_str("subagent"),
            },
            Self::Teammate { team_name } => write!(f, "teammate({team_name})"),
            Self::McpTool { server, tool } => write!(f, "mcp({server}/{tool})"),
        }
    }
}

// ---------------------------------------------------------------------------
// StepStatus — lifecycle of a step
// ---------------------------------------------------------------------------

/// Lifecycle status of a plan step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepStatus {
    /// Not yet started; waiting for dependencies.
    Pending,
    /// Dependencies not met — explicitly blocked.
    Blocked,
    /// Currently executing.
    InProgress,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed { reason: String },
    /// Replaced by new steps via replanning.
    Replanned,
}

impl fmt::Display for StepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Blocked => f.write_str("blocked"),
            Self::InProgress => f.write_str("in-progress"),
            Self::Completed => f.write_str("completed"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
            Self::Replanned => f.write_str("replanned"),
        }
    }
}

// ---------------------------------------------------------------------------
// AgentRole — what kind of agent a step needs
// ---------------------------------------------------------------------------

/// LLM effort level — controls reasoning depth (maps to Claude Code `--effort` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    /// Fast, minimal reasoning — exploration, formatting, linting.
    Low,
    /// Balanced reasoning — implementation, review, debugging.
    Medium,
    /// Deep reasoning — architecture, planning, security review.
    High,
}

impl EffortLevel {
    /// CLI flag value for `claude --effort`.
    pub fn as_flag(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl std::fmt::Display for EffortLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_flag())
    }
}

/// Describes the agent profile needed for a step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRole {
    /// Role name (e.g., "investigator", "coder", "reviewer", "tester").
    pub name: String,
    /// Capabilities the agent must have (matched against CapabilityRegistry).
    pub required_capabilities: Vec<String>,
    /// Preferred LLM model for this role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_model: Option<String>,
    /// Preferred effort level for this role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_effort: Option<EffortLevel>,
    /// Whether this role is read-only (no file writes, no shell mutations).
    #[serde(default)]
    pub read_only: bool,
}

impl Default for AgentRole {
    fn default() -> Self {
        Self {
            name: "general".into(),
            required_capabilities: Vec::new(),
            preferred_model: None,
            preferred_effort: None,
            read_only: false,
        }
    }
}

impl AgentRole {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.required_capabilities = caps;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.preferred_model = Some(model.into());
        self
    }

    pub fn with_effort(mut self, effort: EffortLevel) -> Self {
        self.preferred_effort = Some(effort);
        self
    }

    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

// ---------------------------------------------------------------------------
// ExecutionPlan — the full DAG
// ---------------------------------------------------------------------------

/// A complete execution plan — a DAG of steps with optional team config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Unique plan identifier.
    pub id: Uuid,
    /// Steps in this plan (topological order preferred but not required).
    pub steps: Vec<PlanStep>,
    /// When this plan was created.
    pub created_at: DateTime<Utc>,
    /// How this plan was generated.
    pub strategy: PlanStrategy,
    /// Team configuration (if multi-agent execution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamConfig>,
}

impl ExecutionPlan {
    /// Create a new plan with the given steps and strategy.
    pub fn new(steps: Vec<PlanStep>, strategy: PlanStrategy) -> Self {
        Self {
            id: Uuid::new_v4(),
            steps,
            created_at: Utc::now(),
            strategy,
            team: None,
        }
    }

    /// Create a direct (no-plan) single-step plan for trivial tasks.
    pub fn direct(step: PlanStep) -> Self {
        Self::new(vec![step], PlanStrategy::Direct)
    }

    pub fn with_team(mut self, team: TeamConfig) -> Self {
        self.team = Some(team);
        self
    }

    // -- DAG queries --

    /// Steps whose dependencies are all completed and that are still pending.
    pub fn ready_steps(&self) -> Vec<&PlanStep> {
        let completed: HashSet<Uuid> = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .map(|s| s.id)
            .collect();

        self.steps
            .iter()
            .filter(|s| s.is_ready(&completed))
            .collect()
    }

    /// Whether all steps have reached a terminal state.
    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|s| s.is_terminal())
    }

    /// Whether any step has failed (and the plan needs replanning or abort).
    pub fn has_failures(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s.status, StepStatus::Failed { .. }))
    }

    /// Get a step by ID.
    pub fn get_step(&self, id: Uuid) -> Option<&PlanStep> {
        self.steps.iter().find(|s| s.id == id)
    }

    /// Get a mutable step by ID.
    pub fn get_step_mut(&mut self, id: Uuid) -> Option<&mut PlanStep> {
        self.steps.iter_mut().find(|s| s.id == id)
    }

    /// Steps that can run concurrently (same depth level, no mutual dependencies).
    pub fn parallel_groups(&self) -> Vec<Vec<Uuid>> {
        let depths = self.compute_depths();
        let mut groups: HashMap<u32, Vec<Uuid>> = HashMap::new();

        for step in &self.steps {
            if let Some(&depth) = depths.get(&step.id) {
                groups.entry(depth).or_default().push(step.id);
            }
        }

        let mut sorted: Vec<(u32, Vec<Uuid>)> = groups.into_iter().collect();
        sorted.sort_by_key(|(depth, _)| *depth);
        sorted.into_iter().map(|(_, ids)| ids).collect()
    }

    /// Length of the longest dependency chain (critical path).
    pub fn critical_path_length(&self) -> u32 {
        self.compute_depths()
            .values()
            .copied()
            .max()
            .map(|d| d + 1)
            .unwrap_or(0)
    }

    /// Total estimated token cost across all steps (including sub-plans).
    /// Returns `u64` to avoid silent overflow when many steps have large estimates.
    pub fn estimated_total_tokens(&self) -> u64 {
        self.steps
            .iter()
            .map(|s| {
                let own = s.estimated_tokens as u64;
                let sub = s
                    .sub_plan
                    .as_ref()
                    .map(|p| p.estimated_total_tokens())
                    .unwrap_or(0);
                own + sub
            })
            .sum()
    }

    /// Maximum nesting depth of sub-plans (0 = flat plan, 1 = one level of sub-plans, etc.).
    pub fn max_depth(&self) -> u32 {
        self.steps
            .iter()
            .filter_map(|s| s.sub_plan.as_ref())
            .map(|p| 1 + p.max_depth())
            .max()
            .unwrap_or(0)
    }

    /// Maximum allowed steps for a given complexity tier.
    /// Enforced at plan validation time to prevent over-planning.
    pub fn max_steps_for_complexity(complexity: TaskComplexity) -> usize {
        match complexity {
            TaskComplexity::Trivial => 1,
            TaskComplexity::Low => 3,
            TaskComplexity::Medium => 7,
            TaskComplexity::High => 10,
            TaskComplexity::Critical => 15,
        }
    }

    /// Check whether this plan exceeds the step limit for the given complexity.
    /// Returns the count of active (non-Replanned) steps vs the max allowed.
    pub fn check_step_limit(&self, complexity: TaskComplexity) -> Result<(), PlanValidationError> {
        let max = Self::max_steps_for_complexity(complexity);
        let active_count = self
            .steps
            .iter()
            .filter(|s| s.status != StepStatus::Replanned)
            .count();
        if active_count > max {
            return Err(PlanValidationError::TooManySteps {
                count: active_count,
                max,
                complexity: format!("{complexity:?}"),
            });
        }
        Ok(())
    }

    /// Validate the DAG structure: cycles, dangling deps, duplicate IDs.
    pub fn validate(&self) -> Result<(), PlanValidationError> {
        // Check for duplicate step IDs first (HashSet dedup would hide them)
        let mut seen = HashSet::new();
        for step in &self.steps {
            if !seen.insert(step.id) {
                return Err(PlanValidationError::DuplicateStepId);
            }
        }

        // Check for dangling dependencies
        for step in &self.steps {
            for dep in &step.depends_on {
                if !seen.contains(dep) {
                    return Err(PlanValidationError::DanglingDependency {
                        step_id: step.id,
                        missing_dep: *dep,
                    });
                }
            }
        }

        // Check for cycles via topological sort (Kahn's algorithm)
        if self.has_cycle() {
            return Err(PlanValidationError::CycleDetected);
        }

        // Validate sub-plans: structural validity FIRST (catches cycles before
        // max_depth recurses into them — prevents stack overflow on malformed plans).
        for step in &self.steps {
            if let Some(ref sub) = step.sub_plan {
                if step.execution == StepExecution::Inline {
                    return Err(PlanValidationError::SubPlanOnInlineStep {
                        step_id: step.id,
                        step_name: step.name.clone(),
                    });
                }
                sub.validate()?;
            }
        }

        // Validate sub-plan depth (max 2 levels per ADR-008)
        // Safe to recurse now — sub-plans are cycle-free after validate() above.
        if self.max_depth() > 2 {
            return Err(PlanValidationError::SubPlanTooDeep {
                depth: self.max_depth(),
                max: 2,
            });
        }

        Ok(())
    }

    /// Semantic validation: check that steps reference valid capabilities,
    /// tools, and execution targets. Call after structural validate().
    ///
    /// `available_tools`: set of known tool names (from CapabilityRegistry).
    /// `available_models`: set of known model names (e.g., "opus", "sonnet", "haiku").
    pub fn validate_semantic(
        &self,
        available_tools: &HashSet<String>,
        available_models: &HashSet<String>,
    ) -> Vec<PlanValidationWarning> {
        let mut warnings = Vec::new();

        for step in &self.steps {
            // Check allowed_tools against registry
            for tool in &step.allowed_tools {
                if !available_tools.is_empty() && !available_tools.contains(tool) {
                    warnings.push(PlanValidationWarning::UnknownTool {
                        step_id: step.id,
                        step_name: step.name.clone(),
                        tool: tool.clone(),
                    });
                }
            }

            // Check preferred model
            if let Some(ref model) = step.agent_role.preferred_model
                && !available_models.is_empty()
                && !available_models.contains(model)
            {
                warnings.push(PlanValidationWarning::UnknownModel {
                    step_id: step.id,
                    step_name: step.name.clone(),
                    model: model.clone(),
                });
            }

            // Check read_only consistency: read_only role should not have write tools
            if step.agent_role.read_only {
                let write_tools = ["edit", "write", "shell", "bash", "delete", "move"];
                for tool in &step.allowed_tools {
                    let lower = tool.to_lowercase();
                    if write_tools.iter().any(|w| lower.contains(w)) {
                        warnings.push(PlanValidationWarning::ReadOnlyWithWriteTool {
                            step_id: step.id,
                            step_name: step.name.clone(),
                            tool: tool.clone(),
                        });
                    }
                }
            }
        }

        warnings
    }

    // -- Internal helpers --

    /// Compute depth (distance from roots) for each step via BFS.
    fn compute_depths(&self) -> HashMap<Uuid, u32> {
        let mut depths: HashMap<Uuid, u32> = HashMap::new();
        let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
        let mut children: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        for step in &self.steps {
            in_degree.insert(step.id, step.depends_on.len());
            for dep in &step.depends_on {
                children.entry(*dep).or_default().push(step.id);
            }
        }

        // BFS from roots (in_degree == 0)
        let mut queue: VecDeque<Uuid> = VecDeque::new();
        for step in &self.steps {
            if step.depends_on.is_empty() {
                depths.insert(step.id, 0);
                queue.push_back(step.id);
            }
        }

        while let Some(id) = queue.pop_front() {
            let current_depth = depths[&id];
            if let Some(kids) = children.get(&id) {
                for &child in kids {
                    let entry = depths.entry(child).or_insert(0);
                    *entry = (*entry).max(current_depth + 1);
                    if let Some(deg) = in_degree.get_mut(&child) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            queue.push_back(child);
                        }
                    }
                }
            }
        }

        depths
    }

    /// Cycle detection via Kahn's topological sort.
    fn has_cycle(&self) -> bool {
        let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
        let mut children: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        for step in &self.steps {
            in_degree.entry(step.id).or_insert(0);
            for dep in &step.depends_on {
                children.entry(*dep).or_default().push(step.id);
                *in_degree.entry(step.id).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<Uuid> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut visited = 0usize;
        while let Some(id) = queue.pop_front() {
            visited += 1;
            if let Some(kids) = children.get(&id) {
                for &child in kids {
                    if let Some(deg) = in_degree.get_mut(&child) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            queue.push_back(child);
                        }
                    }
                }
            }
        }

        visited != self.steps.len()
    }
}

impl fmt::Display for ExecutionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self
            .steps
            .iter()
            .filter(|s| s.status != StepStatus::Replanned)
            .count();
        write!(
            f,
            "Plan({}, {:?}, {} steps, ~{} tokens)",
            &self.id.to_string()[..8],
            self.strategy,
            active,
            self.estimated_total_tokens(),
        )?;
        if let Some(ref team) = self.team {
            write!(f, " team={}", team.name)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PlanStrategy — how the plan was generated
// ---------------------------------------------------------------------------

/// Describes how an execution plan was created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanStrategy {
    /// No planning — direct execution for trivial/low complexity tasks.
    Direct,
    /// Generated by an LLM planner.
    Generated { model: String, tokens_used: u64 },
    /// Replanned after a step failure.
    Replanned {
        original_plan_id: Uuid,
        failed_step_id: Uuid,
    },
}

// ---------------------------------------------------------------------------
// Team configuration (maps to Claude Code Agent Teams)
// ---------------------------------------------------------------------------

/// Configuration for a multi-agent team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    /// Team name (e.g., "auth-feature", "migration-crew").
    pub name: String,
    /// Team members with their assigned steps.
    pub members: Vec<TeamMember>,
    /// Communication topology.
    pub communication: TeamCommunication,
}

/// A member of a team, mapped to a plan step's agent role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    /// Unique agent ID (assigned at spawn time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<AgentId>,
    /// Role this member fulfills.
    pub role: AgentRole,
    /// Step IDs this member is responsible for.
    pub assigned_steps: Vec<Uuid>,
}

/// Communication topology for the team.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamCommunication {
    /// Hub-spoke: all communication goes through the lead agent.
    /// Maps to Claude Code **Subagents** model.
    HubSpoke,
    /// Mesh: any member can message any other directly.
    /// Maps to Claude Code **Agent Teams** model (mailbox messaging).
    Mesh,
    /// Pipeline: each member passes output to the next in sequence.
    /// Maps to Factory AI assembly line model.
    Pipeline,
}

// ---------------------------------------------------------------------------
// Validation errors
// ---------------------------------------------------------------------------

/// Errors when validating an execution plan.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PlanValidationError {
    #[error("cycle detected in plan DAG")]
    CycleDetected,
    #[error("step {step_id} depends on non-existent step {missing_dep}")]
    DanglingDependency { step_id: Uuid, missing_dep: Uuid },
    #[error("duplicate step ID in plan")]
    DuplicateStepId,
    #[error("too many steps: {count} exceeds max {max} for {complexity} complexity")]
    TooManySteps {
        count: usize,
        max: usize,
        complexity: String,
    },
    #[error("sub-plan nesting depth {depth} exceeds max {max}")]
    SubPlanTooDeep { depth: u32, max: u32 },
    #[error("step {step_id} ({step_name}) has sub_plan but uses Inline execution")]
    SubPlanOnInlineStep { step_id: Uuid, step_name: String },
}

/// Non-fatal warnings from semantic validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanValidationWarning {
    /// Step references a tool not found in the registry.
    UnknownTool {
        step_id: Uuid,
        step_name: String,
        tool: String,
    },
    /// Step references a model not found in available providers.
    UnknownModel {
        step_id: Uuid,
        step_name: String,
        model: String,
    },
    /// Read-only role has write-capable tools assigned.
    ReadOnlyWithWriteTool {
        step_id: Uuid,
        step_name: String,
        tool: String,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn step(name: &str) -> PlanStep {
        PlanStep::new(name, format!("Do {name}"))
    }

    // -- DAG basics --

    #[test]
    fn single_step_plan() {
        let s = step("only");
        let plan = ExecutionPlan::direct(s);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.ready_steps().len(), 1);
        assert!(!plan.is_complete());
        assert_eq!(plan.critical_path_length(), 1);
    }

    #[test]
    fn linear_chain() {
        let s1 = step("a");
        let s2 = step("b").with_depends_on(vec![s1.id]);
        let s3 = step("c").with_depends_on(vec![s2.id]);
        let plan = ExecutionPlan::new(
            vec![s1, s2, s3],
            PlanStrategy::Generated {
                model: "sonnet".into(),
                tokens_used: 500,
            },
        );

        // Only first step is ready
        assert_eq!(plan.ready_steps().len(), 1);
        assert_eq!(plan.ready_steps()[0].name, "a");
        assert_eq!(plan.critical_path_length(), 3);
        assert_eq!(plan.parallel_groups().len(), 3);
    }

    #[test]
    fn diamond_dag() {
        //   a
        //  / \
        // b   c
        //  \ /
        //   d
        let a = step("a");
        let b = step("b").with_depends_on(vec![a.id]);
        let c = step("c").with_depends_on(vec![a.id]);
        let d = step("d").with_depends_on(vec![b.id, c.id]);
        let plan = ExecutionPlan::new(vec![a, b, c, d], PlanStrategy::Direct);

        assert_eq!(plan.ready_steps().len(), 1); // only a
        assert_eq!(plan.critical_path_length(), 3); // a→b→d or a→c→d
        let groups = plan.parallel_groups();
        assert_eq!(groups.len(), 3); // [a], [b, c], [d]
        assert_eq!(groups[1].len(), 2); // b and c are parallel
    }

    #[test]
    fn ready_steps_after_completion() {
        let a = step("a");
        let b = step("b").with_depends_on(vec![a.id]);
        let c = step("c").with_depends_on(vec![a.id]);
        let mut plan = ExecutionPlan::new(vec![a, b, c], PlanStrategy::Direct);

        // Complete step a
        plan.steps[0].status = StepStatus::Completed;
        let ready = plan.ready_steps();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn is_complete_all_terminal() {
        let a = step("a");
        let b = step("b").with_depends_on(vec![a.id]);
        let mut plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);

        assert!(!plan.is_complete());

        plan.steps[0].status = StepStatus::Completed;
        assert!(!plan.is_complete());

        plan.steps[1].status = StepStatus::Failed {
            reason: "test".into(),
        };
        assert!(plan.is_complete());
    }

    #[test]
    fn has_failures_detection() {
        let mut plan = ExecutionPlan::direct(step("x"));
        assert!(!plan.has_failures());

        plan.steps[0].status = StepStatus::Failed {
            reason: "oops".into(),
        };
        assert!(plan.has_failures());
    }

    // -- Validation --

    #[test]
    fn valid_plan_passes() {
        let a = step("a");
        let b = step("b").with_depends_on(vec![a.id]);
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        assert!(plan.validate().is_ok());
    }

    #[test]
    fn cycle_detected() {
        let mut a = step("a");
        let mut b = step("b");
        a.depends_on = vec![b.id];
        b.depends_on = vec![a.id];
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        assert!(matches!(
            plan.validate(),
            Err(PlanValidationError::CycleDetected)
        ));
    }

    #[test]
    fn self_cycle_detected() {
        let mut a = step("a");
        a.depends_on = vec![a.id];
        let plan = ExecutionPlan::new(vec![a], PlanStrategy::Direct);
        assert!(matches!(
            plan.validate(),
            Err(PlanValidationError::CycleDetected)
        ));
    }

    #[test]
    fn dangling_dependency() {
        let a = step("a").with_depends_on(vec![Uuid::new_v4()]);
        let plan = ExecutionPlan::new(vec![a], PlanStrategy::Direct);
        assert!(matches!(
            plan.validate(),
            Err(PlanValidationError::DanglingDependency { .. })
        ));
    }

    #[test]
    fn duplicate_step_id() {
        let a = step("a");
        let mut b = step("b");
        b.id = a.id; // duplicate
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        assert!(matches!(
            plan.validate(),
            Err(PlanValidationError::DuplicateStepId)
        ));
    }

    // -- StepExecution variants --

    #[test]
    fn step_execution_variants() {
        let inline = step("a").with_execution(StepExecution::Inline);
        assert_eq!(inline.execution, StepExecution::Inline);

        let sub = step("b").with_execution(StepExecution::Subagent {
            model: Some("opus".into()),
        });
        assert!(matches!(sub.execution, StepExecution::Subagent { .. }));

        let team = step("c").with_execution(StepExecution::Teammate {
            team_name: "auth-crew".into(),
        });
        assert!(matches!(team.execution, StepExecution::Teammate { .. }));

        let mcp = step("d").with_execution(StepExecution::McpTool {
            server: "yoyo".into(),
            tool: "search".into(),
        });
        assert!(matches!(mcp.execution, StepExecution::McpTool { .. }));
    }

    // -- AgentRole --

    #[test]
    fn agent_role_builder() {
        let role = AgentRole::new("reviewer")
            .with_capabilities(vec!["code_review".into(), "security_scan".into()])
            .with_model("opus")
            .read_only();

        assert_eq!(role.name, "reviewer");
        assert_eq!(role.required_capabilities.len(), 2);
        assert_eq!(role.preferred_model.as_deref(), Some("opus"));
        assert!(role.read_only);
    }

    // -- TeamConfig --

    #[test]
    fn team_config_mesh() {
        let a = step("implement");
        let b = step("test");
        let team = TeamConfig {
            name: "auth-feature".into(),
            members: vec![
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("coder"),
                    assigned_steps: vec![a.id],
                },
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("tester"),
                    assigned_steps: vec![b.id],
                },
            ],
            communication: TeamCommunication::Mesh,
        };

        assert_eq!(team.members.len(), 2);
        assert_eq!(team.communication, TeamCommunication::Mesh);
    }

    // -- Serialization round-trip --

    #[test]
    fn json_round_trip() {
        let a = step("investigate")
            .with_role(AgentRole::new("explorer").read_only().with_model("haiku"))
            .with_tools(vec!["search".into(), "file_read".into()])
            .with_execution(StepExecution::Subagent {
                model: Some("haiku".into()),
            })
            .with_verify()
            .with_estimated_tokens(2000);

        let b = step("implement")
            .with_depends_on(vec![a.id])
            .with_role(AgentRole::new("coder").with_capabilities(vec!["code_edit".into()]))
            .with_execution(StepExecution::Teammate {
                team_name: "feature-crew".into(),
            })
            .with_verify();

        let plan = ExecutionPlan::new(
            vec![a, b],
            PlanStrategy::Generated {
                model: "opus".into(),
                tokens_used: 1200,
            },
        )
        .with_team(TeamConfig {
            name: "feature-crew".into(),
            members: vec![],
            communication: TeamCommunication::Mesh,
        });

        let json = serde_json::to_string_pretty(&plan).expect("serialize");
        let restored: ExecutionPlan = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.id, plan.id);
        assert_eq!(restored.steps.len(), 2);
        assert_eq!(restored.steps[0].name, "investigate");
        assert_eq!(restored.steps[1].depends_on.len(), 1);
        assert!(restored.team.is_some());
        assert_eq!(
            restored.team.unwrap().communication,
            TeamCommunication::Mesh
        );
    }

    // -- Estimated tokens --

    #[test]
    fn estimated_total_tokens() {
        let a = step("a").with_estimated_tokens(1000);
        let b = step("b").with_estimated_tokens(2000);
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        assert_eq!(plan.estimated_total_tokens(), 3000);
    }

    // -- Parallel groups with wide fan-out --

    #[test]
    fn wide_fan_out() {
        let root = step("root");
        let leaves: Vec<PlanStep> = (0..5)
            .map(|i| step(&format!("leaf-{i}")).with_depends_on(vec![root.id]))
            .collect();
        let mut steps = vec![root];
        steps.extend(leaves);
        let plan = ExecutionPlan::new(steps, PlanStrategy::Direct);

        let groups = plan.parallel_groups();
        assert_eq!(groups.len(), 2); // [root], [leaf-0..4]
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[1].len(), 5);
    }

    // -- Replanned strategy --

    #[test]
    fn replanned_strategy() {
        let original_id = Uuid::new_v4();
        let failed_id = Uuid::new_v4();
        let plan = ExecutionPlan::new(
            vec![step("fix")],
            PlanStrategy::Replanned {
                original_plan_id: original_id,
                failed_step_id: failed_id,
            },
        );
        assert!(matches!(plan.strategy, PlanStrategy::Replanned { .. }));
    }

    // -- Step limit enforcement --

    #[test]
    fn step_limit_values() {
        assert_eq!(
            ExecutionPlan::max_steps_for_complexity(TaskComplexity::Trivial),
            1
        );
        assert_eq!(
            ExecutionPlan::max_steps_for_complexity(TaskComplexity::Low),
            3
        );
        assert_eq!(
            ExecutionPlan::max_steps_for_complexity(TaskComplexity::Medium),
            7
        );
        assert_eq!(
            ExecutionPlan::max_steps_for_complexity(TaskComplexity::High),
            10
        );
        assert_eq!(
            ExecutionPlan::max_steps_for_complexity(TaskComplexity::Critical),
            15
        );
    }

    #[test]
    fn check_step_limit_passes_within_bound() {
        let steps: Vec<PlanStep> = (0..5).map(|i| step(&format!("s{i}"))).collect();
        let plan = ExecutionPlan::new(steps, PlanStrategy::Direct);
        assert!(plan.check_step_limit(TaskComplexity::Medium).is_ok());
    }

    #[test]
    fn check_step_limit_fails_over_bound() {
        let steps: Vec<PlanStep> = (0..8).map(|i| step(&format!("s{i}"))).collect();
        let plan = ExecutionPlan::new(steps, PlanStrategy::Direct);
        assert!(plan.check_step_limit(TaskComplexity::Medium).is_err());
    }

    #[test]
    fn check_step_limit_ignores_replanned_steps() {
        let mut steps: Vec<PlanStep> = (0..8).map(|i| step(&format!("s{i}"))).collect();
        // Mark 2 as replanned — only 6 active, within Medium limit of 7
        steps[6].status = StepStatus::Replanned;
        steps[7].status = StepStatus::Replanned;
        let plan = ExecutionPlan::new(steps, PlanStrategy::Direct);
        assert!(plan.check_step_limit(TaskComplexity::Medium).is_ok());
    }

    // -- Display impls --

    #[test]
    fn display_step_execution() {
        assert_eq!(StepExecution::Inline.to_string(), "inline");
        assert_eq!(
            StepExecution::Subagent {
                model: Some("opus".into())
            }
            .to_string(),
            "subagent(opus)"
        );
        assert_eq!(
            StepExecution::Subagent { model: None }.to_string(),
            "subagent"
        );
        assert_eq!(
            StepExecution::Teammate {
                team_name: "crew".into()
            }
            .to_string(),
            "teammate(crew)"
        );
        assert_eq!(
            StepExecution::McpTool {
                server: "oco".into(),
                tool: "search".into()
            }
            .to_string(),
            "mcp(oco/search)"
        );
    }

    #[test]
    fn display_step_status() {
        assert_eq!(StepStatus::Pending.to_string(), "pending");
        assert_eq!(StepStatus::InProgress.to_string(), "in-progress");
        assert_eq!(StepStatus::Completed.to_string(), "completed");
        assert_eq!(
            StepStatus::Failed {
                reason: "tests failed".into()
            }
            .to_string(),
            "failed: tests failed"
        );
        assert_eq!(StepStatus::Replanned.to_string(), "replanned");
    }

    #[test]
    fn display_plan_step() {
        let s = step("investigate")
            .with_role(AgentRole::new("investigator").read_only())
            .with_verify();
        let display = s.to_string();
        assert!(display.contains("investigate"));
        assert!(display.contains("pending"));
        assert!(display.contains("investigator"));
        assert!(display.contains("inline"));
        assert!(display.contains("+verify"));
    }

    #[test]
    fn display_plan_step_without_verify() {
        let s = step("fix").with_role(AgentRole::new("coder"));
        let display = s.to_string();
        assert!(!display.contains("+verify"));
    }

    #[test]
    fn display_execution_plan() {
        let a = step("a").with_estimated_tokens(1000);
        let b = step("b").with_estimated_tokens(2000);
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        let display = plan.to_string();
        assert!(display.contains("Plan("));
        assert!(display.contains("Direct"));
        assert!(display.contains("2 steps"));
        assert!(display.contains("3000 tokens"));
    }

    #[test]
    fn display_execution_plan_excludes_replanned_from_count() {
        let mut a = step("a");
        a.status = StepStatus::Replanned;
        let b = step("b");
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        let display = plan.to_string();
        assert!(display.contains("1 steps"));
    }

    #[test]
    fn display_execution_plan_with_team() {
        let plan =
            ExecutionPlan::new(vec![step("a")], PlanStrategy::Direct).with_team(TeamConfig {
                name: "alpha".into(),
                members: vec![],
                communication: TeamCommunication::Mesh,
            });
        let display = plan.to_string();
        assert!(display.contains("team=alpha"));
    }

    // -- Sub-plan (ADR-008) --

    #[test]
    fn sub_plan_on_subagent_step() {
        let sub_step = step("sub-task");
        let sub_plan = ExecutionPlan::new(vec![sub_step], PlanStrategy::Direct);
        let parent = step("delegator")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(sub_plan);

        assert!(parent.sub_plan.is_some());
        assert_eq!(parent.sub_plan.as_ref().unwrap().steps.len(), 1);
    }

    #[test]
    fn sub_plan_on_teammate_step() {
        let sub_step = step("team-sub-task");
        let sub_plan = ExecutionPlan::new(vec![sub_step], PlanStrategy::Direct);
        let parent = step("team-worker")
            .with_execution(StepExecution::Teammate {
                team_name: "alpha".into(),
            })
            .with_sub_plan(sub_plan);

        let plan = ExecutionPlan::new(vec![parent], PlanStrategy::Direct);
        assert!(plan.validate().is_ok());
    }

    #[test]
    fn sub_plan_on_inline_step_rejected() {
        let sub_step = step("inner");
        let sub_plan = ExecutionPlan::new(vec![sub_step], PlanStrategy::Direct);
        let parent = step("inline-with-sub")
            .with_execution(StepExecution::Inline)
            .with_sub_plan(sub_plan);

        let plan = ExecutionPlan::new(vec![parent], PlanStrategy::Direct);
        let err = plan.validate().unwrap_err();
        assert!(matches!(
            err,
            PlanValidationError::SubPlanOnInlineStep { .. }
        ));
    }

    #[test]
    fn max_depth_flat_plan() {
        let plan = ExecutionPlan::new(vec![step("a"), step("b")], PlanStrategy::Direct);
        assert_eq!(plan.max_depth(), 0);
    }

    #[test]
    fn max_depth_one_level() {
        let sub = ExecutionPlan::new(vec![step("sub")], PlanStrategy::Direct);
        let parent = step("parent")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(sub);
        let plan = ExecutionPlan::new(vec![parent], PlanStrategy::Direct);
        assert_eq!(plan.max_depth(), 1);
        assert!(plan.validate().is_ok());
    }

    #[test]
    fn max_depth_two_levels() {
        let inner = ExecutionPlan::new(vec![step("inner")], PlanStrategy::Direct);
        let mid_step = step("mid")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(inner);
        let mid_plan = ExecutionPlan::new(vec![mid_step], PlanStrategy::Direct);
        let outer_step = step("outer")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(mid_plan);
        let plan = ExecutionPlan::new(vec![outer_step], PlanStrategy::Direct);
        assert_eq!(plan.max_depth(), 2);
        assert!(plan.validate().is_ok());
    }

    #[test]
    fn max_depth_three_levels_rejected() {
        let level3 = ExecutionPlan::new(vec![step("l3")], PlanStrategy::Direct);
        let l2_step = step("l2")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(level3);
        let level2 = ExecutionPlan::new(vec![l2_step], PlanStrategy::Direct);
        let l1_step = step("l1")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(level2);
        let level1 = ExecutionPlan::new(vec![l1_step], PlanStrategy::Direct);
        let root_step = step("root")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(level1);
        let plan = ExecutionPlan::new(vec![root_step], PlanStrategy::Direct);
        assert_eq!(plan.max_depth(), 3);
        let err = plan.validate().unwrap_err();
        assert!(matches!(err, PlanValidationError::SubPlanTooDeep { .. }));
    }

    #[test]
    fn estimated_tokens_includes_sub_plans() {
        let sub = ExecutionPlan::new(
            vec![
                step("sub-a").with_estimated_tokens(500),
                step("sub-b").with_estimated_tokens(300),
            ],
            PlanStrategy::Direct,
        );
        let parent = step("parent")
            .with_estimated_tokens(1000)
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(sub);
        let plan = ExecutionPlan::new(
            vec![parent, step("other").with_estimated_tokens(200)],
            PlanStrategy::Direct,
        );
        // parent(1000) + sub-a(500) + sub-b(300) + other(200) = 2000
        assert_eq!(plan.estimated_total_tokens(), 2000);
    }

    #[test]
    fn sub_plan_serde_round_trip() {
        let sub = ExecutionPlan::new(vec![step("sub-task")], PlanStrategy::Direct);
        let parent = step("delegator")
            .with_execution(StepExecution::Subagent {
                model: Some("sonnet".into()),
            })
            .with_sub_plan(sub);
        let plan = ExecutionPlan::new(vec![parent], PlanStrategy::Direct);

        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: ExecutionPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.steps.len(), 1);
        assert!(deserialized.steps[0].sub_plan.is_some());
        let sub = deserialized.steps[0].sub_plan.as_ref().unwrap();
        assert_eq!(sub.steps.len(), 1);
        assert_eq!(sub.steps[0].name, "sub-task");
    }
}
