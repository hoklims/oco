//! GraphRunner — executes an `ExecutionPlan` DAG with parallel step support.
//!
//! For Medium+ tasks, the `OrchestrationLoop` delegates to the GraphRunner
//! instead of the flat action loop. The GraphRunner:
//!
//! 1. Finds ready steps (dependencies all completed).
//! 2. Executes them — inline steps run in the main loop, subagent/teammate/mcp
//!    steps are dispatched to their respective executors.
//! 3. Enforces verify gates after implementation steps.
//! 4. Triggers replanning on verification failure (max 3 attempts).
//! 5. Emits `OrchestrationEvent` variants for each step lifecycle change.
//!
//! **Parallel execution**: steps at the same DAG depth with no mutual deps
//! run concurrently via `tokio::join!` / `FuturesUnordered`.

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};
use uuid::Uuid;

use oco_planner::{Planner, PlanningContext};
use oco_shared_types::{ExecutionPlan, OrchestrationEvent, PlanStep, StepStatus};

use crate::error::OrchestratorError;

/// Maximum replan attempts for a single failed step before aborting.
const MAX_REPLAN_ATTEMPTS: u32 = 3;

/// Result of executing a single plan step.
#[derive(Debug)]
pub struct StepResult {
    pub step_id: Uuid,
    pub success: bool,
    pub output: String,
    pub duration_ms: u64,
    pub tokens_used: u32,
}

/// Trait for executing individual plan steps. Abstracted for testability.
///
/// In production, the real implementation wraps `OrchestratorRuntime` and
/// dispatches based on `StepExecution` mode. In tests, a stub returns
/// predetermined results.
#[async_trait::async_trait]
pub trait StepExecutor: Send + Sync {
    /// Execute a single plan step with the given context.
    async fn execute_step(
        &self,
        step: &PlanStep,
        context: &[String],
    ) -> Result<StepResult, OrchestratorError>;

    /// Run verification (tests/build/lint) for a step.
    async fn verify_step(&self, step: &PlanStep) -> Result<StepResult, OrchestratorError>;
}

/// Stub executor for testing — returns configurable results.
pub struct StubStepExecutor {
    /// Default result for all steps. Override per step_name via `overrides`.
    pub default_success: bool,
    /// Per-step overrides: step name → (success, output).
    pub overrides: std::collections::HashMap<String, (bool, String)>,
}

impl StubStepExecutor {
    pub fn all_pass() -> Self {
        Self {
            default_success: true,
            overrides: std::collections::HashMap::new(),
        }
    }

    pub fn with_failure(mut self, step_name: &str, error: &str) -> Self {
        self.overrides
            .insert(step_name.into(), (false, error.into()));
        self
    }
}

#[async_trait::async_trait]
impl StepExecutor for StubStepExecutor {
    async fn execute_step(
        &self,
        step: &PlanStep,
        _context: &[String],
    ) -> Result<StepResult, OrchestratorError> {
        let (success, output) = self
            .overrides
            .get(&step.name)
            .cloned()
            .unwrap_or_else(|| {
                (
                    self.default_success,
                    format!("Executed step: {}", step.name),
                )
            });

        Ok(StepResult {
            step_id: step.id,
            success,
            output,
            duration_ms: 50,
            tokens_used: step.estimated_tokens,
        })
    }

    async fn verify_step(&self, step: &PlanStep) -> Result<StepResult, OrchestratorError> {
        // Verification follows the same override logic
        let verify_key = format!("verify:{}", step.name);
        let (success, output) = self
            .overrides
            .get(&verify_key)
            .cloned()
            .unwrap_or_else(|| (self.default_success, "Verification passed".into()));

        Ok(StepResult {
            step_id: step.id,
            success,
            output,
            duration_ms: 100,
            tokens_used: 500,
        })
    }
}

/// The DAG execution engine.
pub struct GraphRunner {
    executor: Arc<dyn StepExecutor>,
    planner: Arc<dyn Planner>,
    event_tx: Option<UnboundedSender<OrchestrationEvent>>,
    /// Total token budget for this execution.
    token_budget: u64,
    /// Tokens consumed so far.
    tokens_used: u64,
}

impl GraphRunner {
    pub fn new(
        executor: Arc<dyn StepExecutor>,
        planner: Arc<dyn Planner>,
    ) -> Self {
        Self {
            executor,
            planner,
            event_tx: None,
            token_budget: 100_000,
            tokens_used: 0,
        }
    }

    pub fn with_event_channel(mut self, tx: UnboundedSender<OrchestrationEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub fn with_budget(mut self, budget: u64) -> Self {
        self.token_budget = budget;
        self
    }

    /// Execute the full plan, returning the completed (or failed) plan.
    pub async fn execute(
        &mut self,
        mut plan: ExecutionPlan,
        planning_context: &PlanningContext,
    ) -> Result<ExecutionPlan, OrchestratorError> {
        let total_steps = plan.steps.len();
        info!(plan_id = %plan.id, steps = total_steps, "starting graph execution");

        self.emit_plan_generated(&plan);

        let mut replan_count: u32 = 0;

        let mut last_completed_count = 0usize;

        loop {
            // Budget check
            if self.tokens_used >= self.token_budget {
                warn!("token budget exhausted during graph execution");
                break;
            }

            let ready = plan.ready_steps();

            if ready.is_empty() {
                if plan.is_complete() {
                    info!(plan_id = %plan.id, "plan completed successfully");
                    break;
                }
                if plan.has_failures() {
                    // Try replanning
                    if replan_count >= MAX_REPLAN_ATTEMPTS {
                        warn!("max replan attempts reached, aborting");
                        break;
                    }
                    match self.try_replan(&mut plan, planning_context).await {
                        Ok(true) => {
                            replan_count += 1;
                            continue;
                        }
                        Ok(false) => {
                            warn!("replan produced no new steps, aborting");
                            break;
                        }
                        Err(e) => {
                            warn!(error = %e, "replan failed");
                            break;
                        }
                    }
                }
                // Deadlock: no ready steps, not complete, no failures
                warn!("deadlock detected: no ready steps and plan not complete");
                break;
            }

            // No-progress guard: if completed count hasn't changed, we're stuck
            let current_completed = plan
                .steps
                .iter()
                .filter(|s| s.is_terminal())
                .count();
            if current_completed == last_completed_count && last_completed_count > 0 {
                warn!("no progress detected: terminal step count unchanged");
                break;
            }
            last_completed_count = current_completed;

            // Pre-reserve budget for the batch (fix #9: preventive, not reactive)
            let ready_ids: Vec<Uuid> = ready.iter().map(|s| s.id).collect();
            let batch_estimated: u64 = ready_ids
                .iter()
                .filter_map(|id| plan.get_step(*id))
                .map(|s| s.estimated_tokens as u64)
                .sum();
            let remaining = self.token_budget.saturating_sub(self.tokens_used);
            if batch_estimated > remaining * 2 {
                // Batch would exceed 2x remaining budget — trim to affordable steps
                let affordable: Vec<Uuid> = ready_ids
                    .iter()
                    .copied()
                    .scan(0u64, |acc, id| {
                        if let Some(step) = plan.get_step(id) {
                            *acc += step.estimated_tokens as u64;
                            if *acc <= remaining {
                                Some(Some(id))
                            } else {
                                Some(None)
                            }
                        } else {
                            Some(None)
                        }
                    })
                    .flatten()
                    .collect();
                if affordable.is_empty() {
                    warn!("no steps affordable within remaining budget");
                    break;
                }
                debug!(
                    trimmed = ready_ids.len() - affordable.len(),
                    "budget-trimmed parallel batch"
                );
                let results = self.execute_parallel(&plan, &affordable).await;
                self.process_results(&mut plan, results, replan_count).await;
                continue;
            }

            let results = self.execute_parallel(&plan, &ready_ids).await;
            self.process_results(&mut plan, results, replan_count).await;
        }

        Ok(plan)
    }

    /// Process step results: update statuses, run verify gates, emit events.
    async fn process_results(
        &mut self,
        plan: &mut ExecutionPlan,
        results: Vec<StepResult>,
        replan_count: u32,
    ) {
        for result in results {
            let Some(step) = plan.get_step_mut(result.step_id) else {
                warn!(step_id = %result.step_id, "step not found in plan during result processing");
                continue;
            };

            self.tokens_used += result.tokens_used as u64;

            if result.success {
                step.output = Some(result.output.clone());

                if step.verify_after {
                    let verify_result = self.executor.verify_step(step).await;
                    match verify_result {
                        Ok(vr) if vr.success => {
                            let step_name = step.name.clone();
                            step.status = StepStatus::Completed;
                            self.emit_step_completed(result.step_id, &step_name, true, result.duration_ms);
                        }
                        Ok(vr) => {
                            let step_name = step.name.clone();
                            step.status = StepStatus::Failed { reason: vr.output.clone() };
                            self.emit_step_completed(result.step_id, &step_name, false, result.duration_ms);
                            self.emit_verify_gate_failed(result.step_id, &step_name, &vr.output, replan_count);
                        }
                        Err(e) => {
                            let step_name = step.name.clone();
                            step.status = StepStatus::Failed { reason: e.to_string() };
                            self.emit_step_completed(result.step_id, &step_name, false, result.duration_ms);
                        }
                    }
                } else {
                    let step_name = step.name.clone();
                    step.status = StepStatus::Completed;
                    self.emit_step_completed(result.step_id, &step_name, true, result.duration_ms);
                }
            } else {
                let step_name = step.name.clone();
                step.status = StepStatus::Failed { reason: result.output.clone() };
                self.emit_step_completed(result.step_id, &step_name, false, result.duration_ms);
            }
        }
    }

    /// Execute multiple steps in parallel.
    async fn execute_parallel(
        &self,
        plan: &ExecutionPlan,
        step_ids: &[Uuid],
    ) -> Vec<StepResult> {
        if step_ids.len() == 1 {
            // Single step — no need for join overhead
            let step = plan.get_step(step_ids[0]).expect("step must exist");
            self.emit_step_started(step);
            match self.executor.execute_step(step, &[]).await {
                Ok(r) => vec![r],
                Err(e) => vec![StepResult {
                    step_id: step_ids[0],
                    success: false,
                    output: e.to_string(),
                    duration_ms: 0,
                    tokens_used: 0,
                }],
            }
        } else {
            // Multiple steps — run in parallel
            debug!(count = step_ids.len(), "executing parallel steps");
            let mut handles = Vec::with_capacity(step_ids.len());

            for &id in step_ids {
                let step = plan.get_step(id).expect("step must exist").clone();
                self.emit_step_started(&step);
                let executor = self.executor.clone();
                handles.push(tokio::spawn(async move {
                    match executor.execute_step(&step, &[]).await {
                        Ok(r) => r,
                        Err(e) => StepResult {
                            step_id: id,
                            success: false,
                            output: e.to_string(),
                            duration_ms: 0,
                            tokens_used: 0,
                        },
                    }
                }));
            }

            let mut results = Vec::with_capacity(handles.len());
            for (i, handle) in handles.into_iter().enumerate() {
                match handle.await {
                    Ok(r) => results.push(r),
                    Err(e) => {
                        // Fix #8: JoinError (panic/cancel) → produce a Failed StepResult
                        // so the step doesn't "disappear" from the state machine.
                        warn!(error = %e, "step task panicked or was cancelled");
                        let failed_id = step_ids.get(i).copied().unwrap_or_default();
                        results.push(StepResult {
                            step_id: failed_id,
                            success: false,
                            output: format!("task panicked: {e}"),
                            duration_ms: 0,
                            tokens_used: 0,
                        });
                    }
                }
            }
            results
        }
    }

    /// Attempt to replan after a failure. Returns true if new steps were added.
    async fn try_replan(
        &self,
        plan: &mut ExecutionPlan,
        context: &PlanningContext,
    ) -> Result<bool, OrchestratorError> {
        let failed = plan
            .steps
            .iter()
            .find(|s| matches!(s.status, StepStatus::Failed { .. }))
            .cloned();

        let Some(failed_step) = failed else {
            return Ok(false);
        };

        let error_context = match &failed_step.status {
            StepStatus::Failed { reason } => reason.clone(),
            _ => "unknown failure".into(),
        };

        info!(
            step = %failed_step.name,
            error = %error_context,
            "attempting replan"
        );

        let new_plan = self
            .planner
            .replan(plan, &failed_step, &error_context, context)
            .await
            .map_err(|e| OrchestratorError::PlanningFailed(e.to_string()))?;

        // Count new steps (those that aren't Completed or Replanned)
        let new_step_count = new_plan
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Pending)
            .count();

        if new_step_count == 0 {
            return Ok(false);
        }

        // Replace the plan
        *plan = new_plan;

        self.emit_replan_triggered(plan);
        Ok(true)
    }

    // -- Event emission helpers --

    fn emit_plan_generated(&self, plan: &ExecutionPlan) {
        if let Some(ref tx) = self.event_tx {
            // Use existing StepCompleted variant with synthetic data for now.
            // TODO(#15): add PlanGenerated event variant
            let _ = tx.send(OrchestrationEvent::StepCompleted {
                step: 0,
                action: oco_shared_types::OrchestratorAction::Plan {
                    request: format!(
                        "Generated plan with {} steps, {} parallel groups",
                        plan.steps.len(),
                        plan.parallel_groups().len()
                    ),
                },
                reason: format!("strategy: {:?}", plan.strategy),
                duration_ms: 0,
                budget_snapshot: oco_shared_types::BudgetSnapshot {
                    tokens_used: self.tokens_used,
                    tokens_remaining: self.token_budget.saturating_sub(self.tokens_used),
                    tool_calls_used: 0,
                    tool_calls_remaining: 0,
                    retrievals_used: 0,
                    verify_cycles_used: 0,
                    elapsed_secs: 0,
                },
                knowledge_confidence: 0.5,
                success: true,
            });
        }
    }

    fn emit_step_started(&self, step: &PlanStep) {
        debug!(
            step_id = %step.id,
            step_name = %step.name,
            execution = ?step.execution,
            "step started"
        );
    }

    fn emit_step_completed(
        &self,
        step_id: Uuid,
        step_name: &str,
        success: bool,
        duration_ms: u64,
    ) {
        if success {
            info!(step_name, duration_ms, "step completed");
        } else {
            warn!(step_name, duration_ms, "step failed");
        }

        if let Some(ref tx) = self.event_tx {
            let action = if success {
                oco_shared_types::OrchestratorAction::Delegate {
                    step_id,
                    agent_role: oco_shared_types::AgentRole::new(step_name),
                    context: vec![],
                }
            } else {
                oco_shared_types::OrchestratorAction::Replan {
                    failed_step_id: step_id,
                    error_context: "step failed".into(),
                }
            };

            let _ = tx.send(OrchestrationEvent::StepCompleted {
                step: 0, // TODO: track global step counter
                action,
                reason: format!("step {step_name}: {}", if success { "ok" } else { "failed" }),
                duration_ms,
                budget_snapshot: oco_shared_types::BudgetSnapshot {
                    tokens_used: self.tokens_used,
                    tokens_remaining: self.token_budget.saturating_sub(self.tokens_used),
                    tool_calls_used: 0,
                    tool_calls_remaining: 0,
                    retrievals_used: 0,
                    verify_cycles_used: 0,
                    elapsed_secs: 0,
                },
                knowledge_confidence: 0.5,
                success,
            });
        }
    }

    fn emit_verify_gate_failed(
        &self,
        _step_id: Uuid,
        step_name: &str,
        _failures: &str,
        replan_attempt: u32,
    ) {
        warn!(
            step_name,
            replan_attempt,
            "verify gate failed"
        );
        // TODO(#15): emit a dedicated VerifyGateFailed event
    }

    fn emit_replan_triggered(&self, plan: &ExecutionPlan) {
        info!(
            plan_id = %plan.id,
            new_pending = plan.steps.iter().filter(|s| s.status == StepStatus::Pending).count(),
            "replan triggered"
        );
    }
}

// -- Error extension --

impl OrchestratorError {
    /// Convenience constructor for planning failures.
    pub fn planning_failed(msg: impl Into<String>) -> Self {
        Self::PlanningFailed(msg.into())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oco_planner::{DirectPlanner, PlanningContext};
    use oco_shared_types::{PlanStep, PlanStrategy, TaskCategory, TaskComplexity};

    fn make_plan(steps: Vec<PlanStep>) -> ExecutionPlan {
        ExecutionPlan::new(steps, PlanStrategy::Direct)
    }

    fn ctx() -> PlanningContext {
        PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature)
    }

    // -- Basic execution --

    #[tokio::test]
    async fn single_step_executes() {
        let step = PlanStep::new("do-it", "Execute the task");
        let plan = make_plan(vec![step]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.is_complete());
        assert!(!result.has_failures());
        assert_eq!(result.steps[0].status, StepStatus::Completed);
    }

    #[tokio::test]
    async fn linear_chain_executes_in_order() {
        let a = PlanStep::new("first", "Step 1");
        let b = PlanStep::new("second", "Step 2").with_depends_on(vec![a.id]);
        let c = PlanStep::new("third", "Step 3").with_depends_on(vec![b.id]);
        let plan = make_plan(vec![a, b, c]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.is_complete());
        assert!(result.steps.iter().all(|s| s.status == StepStatus::Completed));
    }

    #[tokio::test]
    async fn parallel_steps_execute() {
        let root = PlanStep::new("root", "Setup");
        let a = PlanStep::new("branch-a", "Parallel A").with_depends_on(vec![root.id]);
        let b = PlanStep::new("branch-b", "Parallel B").with_depends_on(vec![root.id]);
        let merge = PlanStep::new("merge", "Merge").with_depends_on(vec![a.id, b.id]);
        let plan = make_plan(vec![root, a, b, merge]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.is_complete());
        assert_eq!(
            result.steps.iter().filter(|s| s.status == StepStatus::Completed).count(),
            4
        );
    }

    // -- Verify gate --

    #[tokio::test]
    async fn verify_gate_passes() {
        let step = PlanStep::new("implement", "Write code").with_verify();
        let plan = make_plan(vec![step]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.is_complete());
        assert!(!result.has_failures());
    }

    #[tokio::test]
    async fn verify_gate_fails_triggers_replan() {
        let step = PlanStep::new("implement", "Write code").with_verify();
        let plan = make_plan(vec![step]);

        // Verification fails
        let executor = Arc::new(
            StubStepExecutor::all_pass()
                .with_failure("verify:implement", "2 tests failing"),
        );
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // After replan, there should be new steps
        assert!(result.steps.len() >= 1);
        // The original step should be Failed or Replanned
        assert!(result.steps.iter().any(|s| {
            matches!(s.status, StepStatus::Failed { .. } | StepStatus::Replanned)
        }));
    }

    // -- Step failure --

    #[tokio::test]
    async fn step_failure_marks_failed() {
        let step = PlanStep::new("broken", "This will fail");
        let plan = make_plan(vec![step]);

        let executor = Arc::new(
            StubStepExecutor::all_pass().with_failure("broken", "runtime error"),
        );
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.has_failures());
    }

    // -- Budget exhaustion --

    #[tokio::test]
    async fn budget_exhaustion_stops_execution() {
        let a = PlanStep::new("big", "Expensive step").with_estimated_tokens(10_000);
        let b = PlanStep::new("after", "After").with_depends_on(vec![a.id]);
        let plan = make_plan(vec![a, b]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        // Very tight budget — only enough for first step
        let mut runner = GraphRunner::new(executor, planner).with_budget(5_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // First step should complete, second should still be pending
        assert!(result.steps.iter().any(|s| s.status == StepStatus::Completed));
        assert!(result.steps.iter().any(|s| s.status == StepStatus::Pending));
    }

    // -- Event emission --

    #[tokio::test]
    async fn events_are_emitted() {
        let step = PlanStep::new("task", "Do it");
        let plan = make_plan(vec![step]);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner)
            .with_event_channel(tx)
            .with_budget(50_000);

        runner.execute(plan, &ctx()).await.unwrap();

        // Should have received at least 2 events: plan generated + step completed
        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert!(events.len() >= 2, "expected >= 2 events, got {}", events.len());
    }

    // -- Diamond DAG --

    #[tokio::test]
    async fn diamond_dag_executes() {
        //   a
        //  / \
        // b   c
        //  \ /
        //   d
        let a = PlanStep::new("a", "root");
        let b = PlanStep::new("b", "left").with_depends_on(vec![a.id]);
        let c = PlanStep::new("c", "right").with_depends_on(vec![a.id]);
        let d = PlanStep::new("d", "merge").with_depends_on(vec![b.id, c.id]);
        let plan = make_plan(vec![a, b, c, d]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.is_complete());
        assert!(!result.has_failures());
    }

    // -- Output capture --

    #[tokio::test]
    async fn step_output_captured() {
        let step = PlanStep::new("task", "Execute");
        let plan = make_plan(vec![step]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.steps[0].output.is_some());
        assert!(result.steps[0].output.as_ref().unwrap().contains("task"));
    }
}
