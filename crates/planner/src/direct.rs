//! Direct planner — single-step plan for Trivial/Low tasks. No LLM call.

use async_trait::async_trait;
use oco_shared_types::{
    AgentRole, ExecutionPlan, PlanStep, PlanStrategy, StepExecution, TaskCategory, TaskComplexity,
};

use crate::Planner;
use crate::context::PlanningContext;
use crate::error::PlannerError;

/// Planner for simple tasks: returns a single-step direct plan.
/// No LLM call, deterministic, instant.
pub struct DirectPlanner;

impl DirectPlanner {
    /// Determine if this planner should handle the task (Trivial or Low complexity).
    pub fn should_handle(complexity: TaskComplexity) -> bool {
        matches!(complexity, TaskComplexity::Trivial | TaskComplexity::Low)
    }
}

#[async_trait]
impl Planner for DirectPlanner {
    async fn plan(
        &self,
        request: &str,
        context: &PlanningContext,
    ) -> Result<ExecutionPlan, PlannerError> {
        let (role, tools, verify) = role_for_category(context.task_category);

        // Cap token estimate to a reasonable maximum for single-step plans.
        // Using budget/2 raw produces 500K+ values that trigger GraphRunner's
        // budget pre-reservation guard, silently dropping the step (fix #43).
        const MAX_SINGLE_STEP_ESTIMATE: u64 = 50_000;
        let estimated = (context.budget.max_total_tokens / 2).min(MAX_SINGLE_STEP_ESTIMATE);

        let step = PlanStep::new("execute", request)
            .with_role(role)
            .with_tools(tools)
            .with_execution(StepExecution::Inline)
            .with_estimated_tokens(estimated as u32);

        let step = if verify { step.with_verify() } else { step };

        Ok(ExecutionPlan::direct(step))
    }

    async fn replan(
        &self,
        _original: &ExecutionPlan,
        failed_step: &PlanStep,
        error_context: &str,
        context: &PlanningContext,
    ) -> Result<ExecutionPlan, PlannerError> {
        // For trivial tasks, replanning is just retrying with error context appended.
        let description = format!(
            "{}\n\nPrevious attempt failed: {}",
            failed_step.description, error_context
        );
        let (role, tools, verify) = role_for_category(context.task_category);

        const MAX_SINGLE_STEP_ESTIMATE: u64 = 50_000;
        let estimated = (context.budget.max_total_tokens / 2).min(MAX_SINGLE_STEP_ESTIMATE);

        let step = PlanStep::new("retry", &description)
            .with_role(role)
            .with_tools(tools)
            .with_execution(StepExecution::Inline)
            .with_estimated_tokens(estimated as u32);

        let step = if verify { step.with_verify() } else { step };

        let mut plan = ExecutionPlan::new(
            vec![step],
            PlanStrategy::Replanned {
                original_plan_id: _original.id,
                failed_step_id: failed_step.id,
            },
        );
        // Direct replan is still a direct plan
        plan.strategy = PlanStrategy::Replanned {
            original_plan_id: _original.id,
            failed_step_id: failed_step.id,
        };

        Ok(plan)
    }
}

/// Determine the agent role, tools, and verify flag based on task category.
fn role_for_category(category: TaskCategory) -> (AgentRole, Vec<String>, bool) {
    match category {
        TaskCategory::Bug => (
            AgentRole::new("debugger").with_capabilities(vec![
                "code_search".into(),
                "file_edit".into(),
                "shell_exec".into(),
            ]),
            vec![], // all tools
            true,   // verify after fix
        ),
        TaskCategory::Refactor => (
            AgentRole::new("refactorer")
                .with_capabilities(vec!["code_search".into(), "file_edit".into()]),
            vec![],
            true,
        ),
        TaskCategory::NewFeature => (
            AgentRole::new("implementer").with_capabilities(vec![
                "code_search".into(),
                "file_edit".into(),
                "shell_exec".into(),
            ]),
            vec![],
            true,
        ),
        TaskCategory::Explanation => (
            AgentRole::new("explainer")
                .with_capabilities(vec!["code_search".into()])
                .read_only(),
            vec!["search".into(), "file_read".into()],
            false,
        ),
        TaskCategory::Review => (
            AgentRole::new("reviewer")
                .with_capabilities(vec!["code_review".into()])
                .read_only(),
            vec!["search".into(), "file_read".into()],
            false,
        ),
        TaskCategory::Security => (
            AgentRole::new("security-reviewer")
                .with_capabilities(vec!["security_scan".into(), "code_review".into()])
                .read_only()
                .with_model("opus"),
            vec!["search".into(), "file_read".into()],
            false,
        ),
        TaskCategory::Testing => (
            AgentRole::new("tester")
                .with_capabilities(vec!["file_edit".into(), "shell_exec".into()]),
            vec![],
            true,
        ),
        TaskCategory::Frontend => (
            AgentRole::new("frontend-dev")
                .with_capabilities(vec!["file_edit".into(), "code_search".into()]),
            vec![],
            true,
        ),
        TaskCategory::DevOps => (
            AgentRole::new("devops")
                .with_capabilities(vec!["shell_exec".into(), "file_edit".into()]),
            vec![],
            true,
        ),
        TaskCategory::General => (AgentRole::new("general"), vec![], false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn direct_plan_single_step() {
        let planner = DirectPlanner;
        let ctx = PlanningContext::minimal(TaskComplexity::Trivial, TaskCategory::Explanation);
        let plan = planner.plan("explain what a mutex is", &ctx).await.unwrap();

        assert_eq!(plan.steps.len(), 1);
        assert!(matches!(plan.strategy, PlanStrategy::Direct));
        assert_eq!(plan.steps[0].name, "execute");
        assert!(plan.steps[0].agent_role.read_only);
        assert!(!plan.steps[0].verify_after);
    }

    #[tokio::test]
    async fn direct_plan_bug_has_verify() {
        let planner = DirectPlanner;
        let ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::Bug);
        let plan = planner.plan("fix the null pointer", &ctx).await.unwrap();

        assert_eq!(plan.steps.len(), 1);
        assert!(plan.steps[0].verify_after);
        assert_eq!(plan.steps[0].agent_role.name, "debugger");
    }

    #[tokio::test]
    async fn direct_plan_security_uses_opus() {
        let planner = DirectPlanner;
        let ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::Security);
        let plan = planner.plan("check for XSS", &ctx).await.unwrap();

        assert_eq!(
            plan.steps[0].agent_role.preferred_model.as_deref(),
            Some("opus")
        );
        assert!(plan.steps[0].agent_role.read_only);
    }

    #[tokio::test]
    async fn direct_replan_includes_error_context() {
        let planner = DirectPlanner;
        let ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::Bug);
        let original = planner.plan("fix the bug", &ctx).await.unwrap();

        let replan = planner
            .replan(
                &original,
                &original.steps[0],
                "test failed: assertion error",
                &ctx,
            )
            .await
            .unwrap();

        assert!(matches!(replan.strategy, PlanStrategy::Replanned { .. }));
        assert!(replan.steps[0].description.contains("assertion error"));
    }

    #[tokio::test]
    async fn direct_plan_caps_estimated_tokens() {
        let planner = DirectPlanner;
        // Use a large budget that would produce an unrealistic estimate without the cap
        let mut ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::General);
        ctx.budget.max_total_tokens = 1_000_000;
        let plan = planner.plan("explain something", &ctx).await.unwrap();

        // Should be capped at 50K, not 500K (budget/2)
        assert_eq!(plan.steps[0].estimated_tokens, 50_000);
    }

    #[tokio::test]
    async fn direct_plan_uses_half_budget_when_small() {
        let planner = DirectPlanner;
        let mut ctx = PlanningContext::minimal(TaskComplexity::Trivial, TaskCategory::General);
        ctx.budget.max_total_tokens = 20_000;
        let plan = planner.plan("quick question", &ctx).await.unwrap();

        // Budget/2 = 10K < 50K cap, so use the natural estimate
        assert_eq!(plan.steps[0].estimated_tokens, 10_000);
    }

    #[test]
    fn should_handle_trivial_and_low() {
        assert!(DirectPlanner::should_handle(TaskComplexity::Trivial));
        assert!(DirectPlanner::should_handle(TaskComplexity::Low));
        assert!(!DirectPlanner::should_handle(TaskComplexity::Medium));
        assert!(!DirectPlanner::should_handle(TaskComplexity::High));
        assert!(!DirectPlanner::should_handle(TaskComplexity::Critical));
    }
}
