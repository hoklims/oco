//! Claude Code Agent Teams backend for the GraphRunner.
//!
//! Maps OCO's `PlanStep` execution to Claude Code Agent Teams primitives:
//! - `StepExecution::Subagent` → `Agent` tool with `isolation: "worktree"`
//! - `StepExecution::Teammate` → `Agent` tool with named Mesh communication
//! - Model/effort routing → frontmatter `model:` + `effort:` per teammate
//!
//! ## Requirements
//!
//! Agent Teams is still experimental in Claude Code (v2.1.32+).
//! Requires: `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`
//!
//! ## Architecture
//!
//! ```text
//! GraphRunner
//!   └── AgentTeamsExecutor
//!         ├── spawn_teammate() → Claude Code Agent tool
//!         ├── collect_result() → StepResult
//!         └── teardown() → cleanup worktrees
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use oco_shared_types::PlanStep;

/// Configuration for an Agent Teams teammate spawned from a PlanStep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateConfig {
    /// Name for the teammate (used with `SendMessage({to: name})`).
    pub name: String,
    /// The plan step this teammate executes.
    pub step_id: Uuid,
    /// Model to use (opus/sonnet/haiku).
    pub model: String,
    /// Effort level (low/medium/high).
    pub effort: String,
    /// Whether this teammate should run in a worktree (isolated git context).
    pub isolated: bool,
    /// Whether to run in the background.
    pub background: bool,
    /// The prompt/task description for this teammate.
    pub prompt: String,
}

impl TeammateConfig {
    /// Build a teammate config from a plan step and routing decision.
    pub fn from_step(step: &PlanStep, model: &str, effort: &str) -> Self {
        let isolated = step.modifies_files();
        let name = format!("oco-{}", step.name);

        Self {
            name,
            step_id: step.id,
            model: model.to_string(),
            effort: effort.to_string(),
            isolated,
            background: true,
            prompt: step.description.clone(),
        }
    }
}

/// Tracks active teammates and their results.
#[derive(Debug, Default)]
pub struct AgentTeamsExecutor {
    /// Active teammates, keyed by step ID.
    active: HashMap<Uuid, TeammateConfig>,
    /// Collected results from completed teammates.
    results: HashMap<Uuid, TeammateResult>,
}

/// Result from a completed teammate execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateResult {
    pub step_id: Uuid,
    pub success: bool,
    pub output: String,
    pub tokens_used: u32,
}

impl AgentTeamsExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a teammate for execution.
    pub fn register(&mut self, config: TeammateConfig) {
        self.active.insert(config.step_id, config);
    }

    /// Record that a teammate has completed.
    pub fn record_result(&mut self, result: TeammateResult) {
        self.active.remove(&result.step_id);
        self.results.insert(result.step_id, result);
    }

    /// Get the result for a completed step.
    pub fn get_result(&self, step_id: &Uuid) -> Option<&TeammateResult> {
        self.results.get(step_id)
    }

    /// Check if a step has an active teammate.
    pub fn is_active(&self, step_id: &Uuid) -> bool {
        self.active.contains_key(step_id)
    }

    /// Number of currently active teammates.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Number of completed teammates.
    pub fn completed_count(&self) -> usize {
        self.results.len()
    }

    /// All active teammate configs (for status display).
    pub fn active_teammates(&self) -> Vec<&TeammateConfig> {
        self.active.values().collect()
    }

    /// Generate the Claude Code Agent tool parameters for a teammate.
    ///
    /// Returns a JSON value that maps to the Agent tool's parameters.
    /// This is what the GraphRunner would pass to `claude` CLI or the
    /// Agent tool when spawning a teammate.
    pub fn agent_tool_params(&self, step_id: &Uuid) -> Option<serde_json::Value> {
        self.active.get(step_id).map(|config| {
            let mut params = serde_json::json!({
                "prompt": config.prompt,
                "description": format!("Execute step: {}", config.name),
                "name": config.name,
                "model": config.model,
                "run_in_background": config.background,
            });

            if config.isolated {
                params["isolation"] = serde_json::json!("worktree");
            }

            params
        })
    }

    /// Cleanup: remove all tracking state.
    pub fn teardown(&mut self) {
        self.active.clear();
        self.results.clear();
    }
}

// ---------------------------------------------------------------------------
// PlanStep extension — determine if a step modifies files
// ---------------------------------------------------------------------------

/// Extension trait for PlanStep to check if it modifies files.
trait PlanStepExt {
    fn modifies_files(&self) -> bool;
}

impl PlanStepExt for PlanStep {
    /// Heuristic: a step modifies files if it's not read-only and has implementation-like role.
    fn modifies_files(&self) -> bool {
        if self.agent_role.read_only {
            return false;
        }
        matches!(
            self.agent_role.name.as_str(),
            "coder"
                | "implementer"
                | "frontend-dev"
                | "backend"
                | "refactorer"
                | "devops"
                | "tester"
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{AgentRole, PlanStep, StepExecution};

    fn make_step(name: &str, role: &str) -> PlanStep {
        PlanStep::new(name, format!("Execute {name}"))
            .with_role(AgentRole::new(role))
            .with_execution(StepExecution::Subagent { model: None })
    }

    #[test]
    fn teammate_config_from_coder_step() {
        let step = make_step("implement-auth", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", "medium");

        assert_eq!(config.name, "oco-implement-auth");
        assert_eq!(config.model, "sonnet");
        assert_eq!(config.effort, "medium");
        assert!(config.isolated, "coder should use worktree isolation");
        assert!(config.background);
    }

    #[test]
    fn teammate_config_from_explorer_step() {
        let step = make_step("search-code", "explorer").with_role(AgentRole::new("explorer"));
        let config = TeammateConfig::from_step(&step, "haiku", "low");

        assert!(!config.isolated, "explorer is read-only heuristic");
    }

    #[test]
    fn teammate_config_from_read_only_role() {
        let step =
            make_step("review", "reviewer").with_role(AgentRole::new("reviewer").read_only());
        let config = TeammateConfig::from_step(&step, "sonnet", "high");

        assert!(!config.isolated);
    }

    #[test]
    fn executor_register_and_complete() {
        let mut exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", "medium");
        let step_id = config.step_id;

        exec.register(config);
        assert_eq!(exec.active_count(), 1);
        assert!(exec.is_active(&step_id));

        exec.record_result(TeammateResult {
            step_id,
            success: true,
            output: "done".into(),
            tokens_used: 1000,
        });

        assert_eq!(exec.active_count(), 0);
        assert_eq!(exec.completed_count(), 1);
        assert!(exec.get_result(&step_id).unwrap().success);
    }

    #[test]
    fn agent_tool_params_includes_worktree() {
        let mut exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "opus", "high");
        let step_id = config.step_id;

        exec.register(config);
        let params = exec.agent_tool_params(&step_id).unwrap();

        assert_eq!(params["model"], "opus");
        assert_eq!(params["isolation"], "worktree");
        assert_eq!(params["run_in_background"], true);
    }

    #[test]
    fn agent_tool_params_no_worktree_for_explorer() {
        let mut exec = AgentTeamsExecutor::new();
        let step = make_step("search", "explorer");
        let config = TeammateConfig::from_step(&step, "haiku", "low");
        let step_id = config.step_id;

        exec.register(config);
        let params = exec.agent_tool_params(&step_id).unwrap();

        assert!(params.get("isolation").is_none());
    }

    #[test]
    fn teardown_clears_all() {
        let mut exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        exec.register(TeammateConfig::from_step(&step, "sonnet", "medium"));

        exec.teardown();
        assert_eq!(exec.active_count(), 0);
        assert_eq!(exec.completed_count(), 0);
    }
}
