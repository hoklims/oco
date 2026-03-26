//! Claude Code Agent Teams backend for the GraphRunner.
//!
//! Maps OCO's `PlanStep` execution to Claude Code Agent Teams primitives:
//! - `StepExecution::Subagent` -> `Agent` tool with `isolation: "worktree"`
//! - `StepExecution::Teammate` -> `Agent` tool with named Mesh communication
//! - Model/effort routing -> frontmatter `model:` + `effort:` per teammate
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
//!   +-- AgentTeamsExecutor
//!         +-- spawn_teammate() -> Claude Code Agent tool
//!         +-- collect_result() -> StepResult
//!         +-- teardown() -> cleanup worktrees
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use oco_shared_types::PlanStep;

// ---------------------------------------------------------------------------
// EffortLevel — local enum until shared-types provides one
// ---------------------------------------------------------------------------

// TODO: Move EffortLevel to oco-shared-types once PR #52 lands.
/// Effort level for a teammate, controlling depth of reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for EffortLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

impl std::str::FromStr for EffortLevel {
    type Err = AgentTeamsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(AgentTeamsError::InvalidEffortLevel(other.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from the Agent Teams executor.
#[derive(Debug, thiserror::Error)]
pub enum AgentTeamsError {
    #[error("step {0} already registered")]
    AlreadyRegistered(Uuid),
    #[error("step {0} not found")]
    NotFound(Uuid),
    #[error("step {0} already completed")]
    AlreadyCompleted(Uuid),
    #[error("invalid effort level: {0}")]
    InvalidEffortLevel(String),
}

// ---------------------------------------------------------------------------
// TeammateStatus — richer state model
// ---------------------------------------------------------------------------

/// Status of a teammate through its lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TeammateStatus {
    /// Registered but not yet running.
    Pending,
    /// Currently executing.
    Running {
        #[serde(with = "instant_serde")]
        started_at: std::time::Instant,
    },
    /// Finished successfully or with failure output.
    Completed(TeammateResult),
    /// Failed with an error message.
    Failed { error: String },
    /// Cancelled before completion.
    Cancelled,
}

/// Serde support for `std::time::Instant` — serializes as elapsed millis from
/// a reference point (not meaningful across processes, but useful for traces).
mod instant_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Instant;

    // We store a process-scoped reference instant for relative serialization.
    static REFERENCE: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

    pub fn serialize<S: Serializer>(instant: &Instant, s: S) -> Result<S::Ok, S::Error> {
        let millis = instant.duration_since(*REFERENCE).as_millis() as u64;
        millis.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Instant, D::Error> {
        let millis = u64::deserialize(d)?;
        Ok(*REFERENCE + std::time::Duration::from_millis(millis))
    }
}

// ---------------------------------------------------------------------------
// TeammateConfig
// ---------------------------------------------------------------------------

/// Configuration for an Agent Teams teammate spawned from a PlanStep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateConfig {
    /// Name for the teammate (used with `SendMessage({to: name})`).
    pub name: String,
    /// The plan step this teammate executes.
    pub step_id: Uuid,
    /// Model to use (opus/sonnet/haiku).
    pub model: String,
    /// Effort level.
    pub effort: EffortLevel,
    /// Whether this teammate should run in a worktree (isolated git context).
    pub isolated: bool,
    /// Whether to run in the background.
    pub background: bool,
    /// The prompt/task description for this teammate.
    pub prompt: String,
}

impl TeammateConfig {
    /// Build a teammate config from a plan step and routing decision.
    pub fn from_step(step: &PlanStep, model: &str, effort: EffortLevel) -> Self {
        let isolated = step.modifies_files();
        let name = format!("oco-{}", step.name);

        Self {
            name,
            step_id: step.id,
            model: model.to_string(),
            effort,
            isolated,
            background: true,
            prompt: step.description.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// TeammateResult
// ---------------------------------------------------------------------------

/// Result from a completed teammate execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateResult {
    pub step_id: Uuid,
    pub success: bool,
    pub output: String,
    pub tokens_used: u32,
}

// ---------------------------------------------------------------------------
// Internal state (behind RwLock)
// ---------------------------------------------------------------------------

/// Teammate entry combining config and status.
#[derive(Debug, Clone)]
struct TeammateEntry {
    config: TeammateConfig,
    status: TeammateStatus,
}

#[derive(Debug, Default)]
struct ExecutorInner {
    teammates: HashMap<Uuid, TeammateEntry>,
}

// ---------------------------------------------------------------------------
// AgentTeamsExecutor — async-safe with Arc<RwLock<...>>
// ---------------------------------------------------------------------------

/// Tracks active teammates and their results.
///
/// All methods take `&self` and are async-safe (interior mutability via
/// `tokio::sync::RwLock`). Values are returned by clone, not reference.
#[derive(Debug, Clone)]
pub struct AgentTeamsExecutor {
    inner: Arc<RwLock<ExecutorInner>>,
}

impl Default for AgentTeamsExecutor {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ExecutorInner::default())),
        }
    }
}

impl AgentTeamsExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a teammate for execution. Returns error if already registered.
    pub async fn register(&self, config: TeammateConfig) -> Result<(), AgentTeamsError> {
        let mut inner = self.inner.write().await;
        if inner.teammates.contains_key(&config.step_id) {
            return Err(AgentTeamsError::AlreadyRegistered(config.step_id));
        }
        let step_id = config.step_id;
        inner.teammates.insert(
            step_id,
            TeammateEntry {
                config,
                status: TeammateStatus::Pending,
            },
        );
        Ok(())
    }

    /// Mark a teammate as running.
    pub async fn mark_running(&self, step_id: &Uuid) -> Result<(), AgentTeamsError> {
        let mut inner = self.inner.write().await;
        let entry = inner
            .teammates
            .get_mut(step_id)
            .ok_or(AgentTeamsError::NotFound(*step_id))?;
        if matches!(entry.status, TeammateStatus::Completed(_)) {
            return Err(AgentTeamsError::AlreadyCompleted(*step_id));
        }
        entry.status = TeammateStatus::Running {
            started_at: std::time::Instant::now(),
        };
        Ok(())
    }

    /// Record that a teammate has completed.
    pub async fn record_result(&self, result: TeammateResult) -> Result<(), AgentTeamsError> {
        let mut inner = self.inner.write().await;
        let entry = inner
            .teammates
            .get_mut(&result.step_id)
            .ok_or(AgentTeamsError::NotFound(result.step_id))?;
        if matches!(entry.status, TeammateStatus::Completed(_)) {
            return Err(AgentTeamsError::AlreadyCompleted(result.step_id));
        }
        entry.status = TeammateStatus::Completed(result);
        Ok(())
    }

    /// Record that a teammate has failed.
    pub async fn record_failure(
        &self,
        step_id: &Uuid,
        error: String,
    ) -> Result<(), AgentTeamsError> {
        let mut inner = self.inner.write().await;
        let entry = inner
            .teammates
            .get_mut(step_id)
            .ok_or(AgentTeamsError::NotFound(*step_id))?;
        if matches!(entry.status, TeammateStatus::Completed(_)) {
            return Err(AgentTeamsError::AlreadyCompleted(*step_id));
        }
        entry.status = TeammateStatus::Failed { error };
        Ok(())
    }

    /// Cancel a teammate.
    pub async fn cancel(&self, step_id: &Uuid) -> Result<(), AgentTeamsError> {
        let mut inner = self.inner.write().await;
        let entry = inner
            .teammates
            .get_mut(step_id)
            .ok_or(AgentTeamsError::NotFound(*step_id))?;
        if matches!(entry.status, TeammateStatus::Completed(_)) {
            return Err(AgentTeamsError::AlreadyCompleted(*step_id));
        }
        entry.status = TeammateStatus::Cancelled;
        Ok(())
    }

    /// Get the result for a completed step (cloned).
    pub async fn get_result(&self, step_id: &Uuid) -> Option<TeammateResult> {
        let inner = self.inner.read().await;
        inner.teammates.get(step_id).and_then(|e| {
            if let TeammateStatus::Completed(ref r) = e.status {
                Some(r.clone())
            } else {
                None
            }
        })
    }

    /// Get the status of a teammate (cloned).
    pub async fn get_status(&self, step_id: &Uuid) -> Option<TeammateStatus> {
        let inner = self.inner.read().await;
        inner.teammates.get(step_id).map(|e| e.status.clone())
    }

    /// Check if a step is currently running.
    pub async fn is_active(&self, step_id: &Uuid) -> bool {
        let inner = self.inner.read().await;
        inner.teammates.get(step_id).is_some_and(|e| {
            matches!(
                e.status,
                TeammateStatus::Pending | TeammateStatus::Running { .. }
            )
        })
    }

    /// Number of currently active teammates (Pending or Running).
    pub async fn active_count(&self) -> usize {
        let inner = self.inner.read().await;
        inner
            .teammates
            .values()
            .filter(|e| {
                matches!(
                    e.status,
                    TeammateStatus::Pending | TeammateStatus::Running { .. }
                )
            })
            .count()
    }

    /// Number of completed teammates.
    pub async fn completed_count(&self) -> usize {
        let inner = self.inner.read().await;
        inner
            .teammates
            .values()
            .filter(|e| matches!(e.status, TeammateStatus::Completed(_)))
            .count()
    }

    /// All active teammate configs (cloned, for status display).
    pub async fn active_teammates(&self) -> Vec<TeammateConfig> {
        let inner = self.inner.read().await;
        inner
            .teammates
            .values()
            .filter(|e| {
                matches!(
                    e.status,
                    TeammateStatus::Pending | TeammateStatus::Running { .. }
                )
            })
            .map(|e| e.config.clone())
            .collect()
    }

    /// Generate the Claude Code Agent tool parameters for a teammate.
    ///
    /// Returns a JSON value that maps to the Agent tool's parameters.
    /// This is what the GraphRunner would pass to `claude` CLI or the
    /// Agent tool when spawning a teammate.
    pub async fn agent_tool_params(&self, step_id: &Uuid) -> Option<serde_json::Value> {
        let inner = self.inner.read().await;
        inner
            .teammates
            .get(step_id)
            .filter(|e| {
                matches!(
                    e.status,
                    TeammateStatus::Pending | TeammateStatus::Running { .. }
                )
            })
            .map(|entry| {
                let config = &entry.config;
                let mut params = serde_json::json!({
                    "prompt": config.prompt,
                    "description": format!("Execute step: {}", config.name),
                    "name": config.name,
                    "model": config.model,
                    "effort": config.effort,
                    "run_in_background": config.background,
                });

                if config.isolated {
                    params["isolation"] = serde_json::json!("worktree");
                }

                params
            })
    }

    /// Cleanup: remove all tracking state.
    pub async fn teardown(&self) {
        let mut inner = self.inner.write().await;
        inner.teammates.clear();
    }
}

// ---------------------------------------------------------------------------
// PlanStep extension -- determine if a step modifies files
// ---------------------------------------------------------------------------

/// Extension trait for PlanStep to check if it modifies files.
trait PlanStepExt {
    fn modifies_files(&self) -> bool;
}

impl PlanStepExt for PlanStep {
    /// Safe default: if the role is NOT read_only, assume it writes files
    /// and needs worktree isolation. Only skip isolation for explicitly
    /// read_only roles.
    fn modifies_files(&self) -> bool {
        !self.agent_role.read_only
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

    #[tokio::test]
    async fn teammate_config_from_coder_step() {
        let step = make_step("implement-auth", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::Medium);

        assert_eq!(config.name, "oco-implement-auth");
        assert_eq!(config.model, "sonnet");
        assert_eq!(config.effort, EffortLevel::Medium);
        assert!(
            config.isolated,
            "non-read-only role should use worktree isolation"
        );
        assert!(config.background);
    }

    #[tokio::test]
    async fn teammate_config_from_non_read_only_role_is_isolated() {
        // Any non-read-only role (even unknown ones) should default to isolated
        let step = make_step("search-code", "explorer");
        let config = TeammateConfig::from_step(&step, "haiku", EffortLevel::Low);

        assert!(
            config.isolated,
            "non-read-only role defaults to isolated (safe default)"
        );
    }

    #[tokio::test]
    async fn teammate_config_from_read_only_role() {
        let step =
            make_step("review", "reviewer").with_role(AgentRole::new("reviewer").read_only());
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::High);

        assert!(!config.isolated);
    }

    #[tokio::test]
    async fn executor_register_and_complete() {
        let exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::Medium);
        let step_id = config.step_id;

        exec.register(config).await.unwrap();
        assert_eq!(exec.active_count().await, 1);
        assert!(exec.is_active(&step_id).await);

        exec.record_result(TeammateResult {
            step_id,
            success: true,
            output: "done".into(),
            tokens_used: 1000,
        })
        .await
        .unwrap();

        assert_eq!(exec.active_count().await, 0);
        assert_eq!(exec.completed_count().await, 1);
        assert!(exec.get_result(&step_id).await.unwrap().success);
    }

    #[tokio::test]
    async fn register_duplicate_returns_error() {
        let exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::Medium);
        let config2 = config.clone();

        exec.register(config).await.unwrap();
        let err = exec.register(config2).await.unwrap_err();
        assert!(matches!(err, AgentTeamsError::AlreadyRegistered(_)));
    }

    #[tokio::test]
    async fn record_result_unknown_step_returns_error() {
        let exec = AgentTeamsExecutor::new();
        let err = exec
            .record_result(TeammateResult {
                step_id: Uuid::new_v4(),
                success: true,
                output: "done".into(),
                tokens_used: 0,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AgentTeamsError::NotFound(_)));
    }

    #[tokio::test]
    async fn record_result_already_completed_returns_error() {
        let exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::Medium);
        let step_id = config.step_id;

        exec.register(config).await.unwrap();
        exec.record_result(TeammateResult {
            step_id,
            success: true,
            output: "done".into(),
            tokens_used: 100,
        })
        .await
        .unwrap();

        let err = exec
            .record_result(TeammateResult {
                step_id,
                success: false,
                output: "again".into(),
                tokens_used: 50,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AgentTeamsError::AlreadyCompleted(_)));
    }

    #[tokio::test]
    async fn agent_tool_params_includes_worktree_and_effort() {
        let exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "opus", EffortLevel::High);
        let step_id = config.step_id;

        exec.register(config).await.unwrap();
        let params = exec.agent_tool_params(&step_id).await.unwrap();

        assert_eq!(params["model"], "opus");
        assert_eq!(params["effort"], "high");
        assert_eq!(params["isolation"], "worktree");
        assert_eq!(params["run_in_background"], true);
    }

    #[tokio::test]
    async fn agent_tool_params_no_worktree_for_read_only() {
        let exec = AgentTeamsExecutor::new();
        let step =
            make_step("search", "explorer").with_role(AgentRole::new("explorer").read_only());
        let config = TeammateConfig::from_step(&step, "haiku", EffortLevel::Low);
        let step_id = config.step_id;

        exec.register(config).await.unwrap();
        let params = exec.agent_tool_params(&step_id).await.unwrap();

        assert!(params.get("isolation").is_none());
        assert_eq!(params["effort"], "low");
    }

    #[tokio::test]
    async fn teardown_clears_all() {
        let exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        exec.register(TeammateConfig::from_step(
            &step,
            "sonnet",
            EffortLevel::Medium,
        ))
        .await
        .unwrap();

        exec.teardown().await;
        assert_eq!(exec.active_count().await, 0);
        assert_eq!(exec.completed_count().await, 0);
    }

    #[tokio::test]
    async fn status_transitions() {
        let exec = AgentTeamsExecutor::new();
        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::Medium);
        let step_id = config.step_id;

        exec.register(config).await.unwrap();
        assert!(matches!(
            exec.get_status(&step_id).await.unwrap(),
            TeammateStatus::Pending
        ));

        exec.mark_running(&step_id).await.unwrap();
        assert!(matches!(
            exec.get_status(&step_id).await.unwrap(),
            TeammateStatus::Running { .. }
        ));

        exec.record_result(TeammateResult {
            step_id,
            success: true,
            output: "done".into(),
            tokens_used: 500,
        })
        .await
        .unwrap();
        assert!(matches!(
            exec.get_status(&step_id).await.unwrap(),
            TeammateStatus::Completed(_)
        ));
    }

    #[tokio::test]
    async fn failure_and_cancel_transitions() {
        let exec = AgentTeamsExecutor::new();

        // Test failure
        let step1 = make_step("fail-step", "coder");
        let config1 = TeammateConfig::from_step(&step1, "sonnet", EffortLevel::Medium);
        let id1 = config1.step_id;
        exec.register(config1).await.unwrap();
        exec.record_failure(&id1, "oops".into()).await.unwrap();
        assert!(matches!(
            exec.get_status(&id1).await.unwrap(),
            TeammateStatus::Failed { .. }
        ));

        // Test cancel
        let step2 = make_step("cancel-step", "coder");
        let config2 = TeammateConfig::from_step(&step2, "sonnet", EffortLevel::Medium);
        let id2 = config2.step_id;
        exec.register(config2).await.unwrap();
        exec.cancel(&id2).await.unwrap();
        assert!(matches!(
            exec.get_status(&id2).await.unwrap(),
            TeammateStatus::Cancelled
        ));
    }

    #[tokio::test]
    async fn effort_level_parsing() {
        assert_eq!("low".parse::<EffortLevel>().unwrap(), EffortLevel::Low);
        assert_eq!(
            "medium".parse::<EffortLevel>().unwrap(),
            EffortLevel::Medium
        );
        assert_eq!("high".parse::<EffortLevel>().unwrap(), EffortLevel::High);
        assert!("ultra".parse::<EffortLevel>().is_err());
    }

    #[tokio::test]
    async fn executor_is_clone_and_send() {
        let exec = AgentTeamsExecutor::new();
        let exec2 = exec.clone();

        let step = make_step("impl", "coder");
        let config = TeammateConfig::from_step(&step, "sonnet", EffortLevel::Medium);
        let step_id = config.step_id;

        exec.register(config).await.unwrap();

        // Both handles see the same state
        assert!(exec2.is_active(&step_id).await);
    }
}
