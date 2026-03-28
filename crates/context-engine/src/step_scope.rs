//! Step-scoped context filtering for plan-based orchestration.
//!
//! When executing an `ExecutionPlan`, each step needs a focused context window:
//! - Only tools/capabilities relevant to the step's role
//! - Outputs from completed dependency steps (their discoveries)
//! - Shared memory entries readable by this step's agent
//! - Error context from failed previous attempts (Manus pattern: keep errors visible)
//!
//! This module extends `ContextBuilder` with step-aware filtering.

use oco_shared_types::{
    ContextItem, ContextPriority, ContextSource, ExecutionPlan, PlanStep, SharedMemoryBus,
    StepStatus,
};

use crate::builder::ContextBuilder;
use crate::estimator::TokenEstimator;

/// Extension methods for building step-scoped context.
pub struct StepContextBuilder;

impl StepContextBuilder {
    /// Build a context window scoped to a specific plan step.
    ///
    /// The resulting context includes:
    /// 1. System prompt (always)
    /// 2. User request (always)
    /// 3. Step description + role instructions
    /// 4. Outputs from completed dependency steps
    /// 5. Error context from failed steps (kept visible — Manus pattern)
    /// 6. Shared memory entries (Agent Teams mailbox)
    pub fn build_for_step(
        step: &PlanStep,
        plan: &ExecutionPlan,
        user_request: &str,
        shared_memory: Option<&SharedMemoryBus>,
        agent_id: Option<oco_shared_types::AgentId>,
        budget_tokens: u32,
        current_step: u32,
    ) -> ContextBuilder {
        let mut builder = ContextBuilder::new(budget_tokens)
            .with_staleness(current_step, 8)
            .with_user_request(user_request);

        // Step instructions — high priority
        let step_prompt = build_step_prompt(step);
        builder = builder.with_pinned(vec![ContextItem {
            key: format!("__step_{}__", step.id),
            label: format!("Step: {}", step.name),
            content: step_prompt,
            token_estimate: 0, // re-estimated by builder
            priority: ContextPriority::System,
            source: ContextSource::UserRequest,
            pinned: true,
            relevance: 1.0,
            added_at: chrono::Utc::now(),
            added_at_step: current_step,
        }]);

        // Dependency outputs — important context from completed predecessors
        let dep_items = collect_dependency_outputs(step, plan, current_step);
        if !dep_items.is_empty() {
            builder = builder.with_retrieved_items(dep_items);
        }

        // Error context from failed steps — keep visible (Manus pattern)
        let error_items = collect_error_context(plan, current_step);
        if !error_items.is_empty() {
            builder = builder.with_retrieved_items(error_items);
        }

        // Shared memory entries (Agent Teams mailbox)
        if let (Some(bus), Some(aid)) = (shared_memory, agent_id) {
            let memory_items = collect_shared_memory(bus, aid, current_step);
            if !memory_items.is_empty() {
                builder = builder.with_retrieved_items(memory_items);
            }
        }

        builder
    }

    /// Compute token budget for a step proportional to its estimated cost.
    /// Each step gets a share of the total budget proportional to its token estimate.
    pub fn budget_for_step(step: &PlanStep, plan: &ExecutionPlan, total_budget: u32) -> u32 {
        let total_estimated = plan.estimated_total_tokens().max(1);
        let step_share = step.estimated_tokens as f64 / total_estimated as f64; // both promoted to f64
        // At least 10% of total, at most 80%
        let share = step_share.clamp(0.1, 0.8);
        (total_budget as f64 * share) as u32
    }
}

/// Build the step-specific system prompt.
fn build_step_prompt(step: &PlanStep) -> String {
    let mut prompt = format!("## Current Step: {}\n\n{}\n", step.name, step.description);

    // Role instructions
    prompt.push_str(&format!("\n### Role: {}\n", step.agent_role.name));

    if !step.agent_role.required_capabilities.is_empty() {
        prompt.push_str(&format!(
            "Required capabilities: {}\n",
            step.agent_role.required_capabilities.join(", ")
        ));
    }

    if step.agent_role.read_only {
        prompt.push_str("**READ ONLY** — do not modify any files.\n");
    }

    if !step.allowed_tools.is_empty() {
        prompt.push_str(&format!(
            "Allowed tools: {}\n",
            step.allowed_tools.join(", ")
        ));
    }

    if step.verify_after {
        prompt.push_str("**Verification required** after this step completes.\n");
    }

    prompt
}

/// Collect outputs from completed dependency steps.
fn collect_dependency_outputs(
    step: &PlanStep,
    plan: &ExecutionPlan,
    current_step: u32,
) -> Vec<ContextItem> {
    let mut items = Vec::new();

    for dep_id in &step.depends_on {
        let Some(dep) = plan.get_step(*dep_id) else {
            continue;
        };
        if dep.status != StepStatus::Completed {
            continue;
        }
        let Some(ref output) = dep.output else {
            continue;
        };
        items.push(ContextItem {
            key: format!("dep_output:{}", dep.id),
            label: format!("Output from step '{}'", dep.name),
            content: output.clone(),
            token_estimate: TokenEstimator::estimate_tokens(output),
            priority: ContextPriority::High,
            source: ContextSource::ToolOutput {
                tool_name: format!("step:{}", dep.name),
            },
            pinned: false,
            relevance: 0.9,
            added_at: chrono::Utc::now(),
            added_at_step: current_step,
        });
    }

    items
}

/// Collect error context from failed steps (Manus pattern: errors = signal).
fn collect_error_context(plan: &ExecutionPlan, current_step: u32) -> Vec<ContextItem> {
    let mut items = Vec::new();

    for step in &plan.steps {
        if let StepStatus::Failed { ref reason } = step.status {
            items.push(ContextItem {
                key: format!("error:{}", step.id),
                label: format!("FAILED: step '{}'", step.name),
                content: format!(
                    "Step '{}' failed with error:\n{}\n\nDo NOT repeat the same approach.",
                    step.name, reason
                ),
                token_estimate: TokenEstimator::estimate_tokens(reason),
                priority: ContextPriority::High, // errors are high priority
                source: ContextSource::VerificationResult,
                pinned: false,
                relevance: 0.95, // errors are extremely relevant
                added_at: chrono::Utc::now(),
                added_at_step: current_step,
            });
        }
    }

    items
}

/// Collect shared memory entries readable by this agent (Agent Teams mailbox).
fn collect_shared_memory(
    bus: &SharedMemoryBus,
    agent_id: oco_shared_types::AgentId,
    current_step: u32,
) -> Vec<ContextItem> {
    bus.get_for_agent(agent_id)
        .into_iter()
        .map(|entry| {
            let content = match &entry.value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            ContextItem {
                key: format!("shared_memory:{}", entry.key),
                label: format!("Shared: {}", entry.key),
                content,
                token_estimate: 0, // re-estimated by builder
                priority: ContextPriority::Medium,
                source: ContextSource::SessionSummary,
                pinned: false,
                relevance: 0.7,
                added_at: entry.created_at,
                added_at_step: current_step,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{AgentId, AgentRole, PlanStep, PlanStrategy, SharedMemoryEntry};

    /// Returns (plan, investigate_step_id, implement_step_id).
    fn simple_plan() -> (ExecutionPlan, uuid::Uuid, uuid::Uuid) {
        let mut a = PlanStep::new("investigate", "Search for code patterns")
            .with_role(AgentRole::new("explorer").read_only());
        a.status = StepStatus::Completed;
        a.output = Some("Found auth module in src/auth.rs with 3 endpoints".into());
        let a_id = a.id;

        let b = PlanStep::new("implement", "Write the feature")
            .with_depends_on(vec![a.id])
            .with_role(AgentRole::new("coder"))
            .with_verify();
        let b_id = b.id;

        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        (plan, a_id, b_id)
    }

    #[test]
    fn step_context_includes_step_prompt() {
        let (plan, _, b_id) = simple_plan();
        let step = plan.get_step(b_id).unwrap();

        let builder =
            StepContextBuilder::build_for_step(step, &plan, "add JWT auth", None, None, 10_000, 1);
        let ctx = builder.build();

        // Should have user request + step prompt + dependency output
        assert!(ctx.items.len() >= 2);
        // Step prompt should mention the step name
        assert!(ctx.items.iter().any(|i| i.content.contains("implement")));
    }

    #[test]
    fn step_context_includes_dependency_outputs() {
        let (plan, _, b_id) = simple_plan();
        let step = plan.get_step(b_id).unwrap();

        let builder =
            StepContextBuilder::build_for_step(step, &plan, "add JWT auth", None, None, 10_000, 1);
        let ctx = builder.build();

        // Should include output from completed dependency
        assert!(ctx.items.iter().any(|i| i.content.contains("auth module")));
    }

    #[test]
    fn step_context_includes_error_context() {
        let mut a = PlanStep::new("first-attempt", "Try implementation");
        a.status = StepStatus::Failed {
            reason: "TypeError: undefined is not a function".into(),
        };

        let b = PlanStep::new("retry", "Fix and retry");
        let b_id = b.id;
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        let step = plan.get_step(b_id).unwrap();

        let builder =
            StepContextBuilder::build_for_step(step, &plan, "fix the bug", None, None, 10_000, 2);
        let ctx = builder.build();

        // Error should be in context
        assert!(ctx.items.iter().any(|i| i.content.contains("TypeError")));
        // And should warn against repeating
        assert!(ctx.items.iter().any(|i| i.content.contains("NOT repeat")));
    }

    #[test]
    fn step_context_includes_shared_memory() {
        let (plan, _, b_id) = simple_plan();
        let step = plan.get_step(b_id).unwrap();

        let agent = AgentId::new();
        let mut bus = SharedMemoryBus::new();
        let entry = SharedMemoryEntry::new(
            agent,
            "api_contract",
            serde_json::json!("POST /auth/login { email, password }"),
        )
        .shareable();
        bus.put(entry);

        let builder = StepContextBuilder::build_for_step(
            step,
            &plan,
            "add JWT auth",
            Some(&bus),
            Some(agent),
            10_000,
            1,
        );
        let ctx = builder.build();

        assert!(ctx.items.iter().any(|i| i.content.contains("/auth/login")));
    }

    #[test]
    fn step_context_read_only_annotation() {
        let step = PlanStep::new("review", "Review the code")
            .with_role(AgentRole::new("reviewer").read_only());
        let plan = ExecutionPlan::new(vec![step.clone()], PlanStrategy::Direct);

        let builder =
            StepContextBuilder::build_for_step(&step, &plan, "review auth", None, None, 10_000, 0);
        let ctx = builder.build();

        assert!(ctx.items.iter().any(|i| i.content.contains("READ ONLY")));
    }

    #[test]
    fn budget_for_step_proportional() {
        let a = PlanStep::new("small", "Quick task").with_estimated_tokens(1000);
        let b = PlanStep::new("big", "Heavy task").with_estimated_tokens(9000);
        let plan = ExecutionPlan::new(vec![a.clone(), b.clone()], PlanStrategy::Direct);

        let budget_a = StepContextBuilder::budget_for_step(&a, &plan, 50_000);
        let budget_b = StepContextBuilder::budget_for_step(&b, &plan, 50_000);

        // big should get more budget than small
        assert!(budget_b > budget_a);
        // Both should be within bounds
        assert!(budget_a >= 5_000); // min 10%
        assert!(budget_b <= 40_000); // max 80%
    }

    #[test]
    fn budget_for_step_clamped() {
        // Single step gets clamped to 80%
        let step = PlanStep::new("only", "Only step").with_estimated_tokens(10_000);
        let plan = ExecutionPlan::new(vec![step.clone()], PlanStrategy::Direct);

        let budget = StepContextBuilder::budget_for_step(&step, &plan, 50_000);
        assert!(budget <= 40_000); // 80% of 50k
    }

    #[test]
    fn no_dependency_outputs_for_pending_steps() {
        let a = PlanStep::new("pending", "Not done yet");
        // a is still Pending (not Completed)
        let b = PlanStep::new("after", "Depends on pending").with_depends_on(vec![a.id]);
        let plan = ExecutionPlan::new(vec![a, b.clone()], PlanStrategy::Direct);

        let builder = StepContextBuilder::build_for_step(&b, &plan, "task", None, None, 10_000, 0);
        let ctx = builder.build();

        // Should NOT have any dependency output (a is still pending)
        assert!(!ctx.items.iter().any(|i| i.key.starts_with("dep_output:")));
    }
}
