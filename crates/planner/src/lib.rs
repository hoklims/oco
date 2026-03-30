//! Planner crate — generates emergent `ExecutionPlan`s from user requests.
//!
//! Each plan is unique: the planner analyzes the task, repo profile, and available
//! capabilities to produce a DAG of steps with appropriate agent roles, tools, and
//! execution strategies. No templates — the structure emerges from the context.
//!
//! Two implementations:
//! - **`DirectPlanner`** — for Trivial/Low tasks: single-step, no LLM call.
//! - **`LlmPlanner`** — for Medium+ tasks: calls LLM to generate a structured DAG.

mod context;
mod direct;
mod error;
mod llm_planner;
mod prompt;
pub mod risk_analysis;

pub use context::PlanningContext;
pub use direct::DirectPlanner;
pub use error::PlannerError;
pub use llm_planner::{LlmCallFn, LlmPlanner, PlanCandidate};
pub use prompt::PlanBias;
pub use risk_analysis::{FailurePreview, Risk, analyze_risks};

use async_trait::async_trait;
use oco_shared_types::{ExecutionPlan, PlanStep};

/// Trait for plan generation. Implementations decide how to decompose a task.
#[async_trait]
pub trait Planner: Send + Sync {
    /// Generate an execution plan for the given request.
    async fn plan(
        &self,
        request: &str,
        context: &PlanningContext,
    ) -> Result<ExecutionPlan, PlannerError>;

    /// Replan after a step failure: generate a replacement sub-plan.
    /// Preserves completed steps, replaces the failed step and its dependents.
    async fn replan(
        &self,
        original: &ExecutionPlan,
        failed_step: &PlanStep,
        error_context: &str,
        context: &PlanningContext,
    ) -> Result<ExecutionPlan, PlannerError>;
}
