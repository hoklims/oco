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
use oco_shared_types::{
    CheckResult, ExecutionPlan, OrchestrationEvent, PlanStep, StepStatus, StepSummary, TeamSummary,
};

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
    pub tokens_used: u64,
}

/// Cooperative cancellation token for step executors (fix #23).
///
/// Cloned and shared between the GraphRunner and spawned step tasks.
/// Check `is_cancelled()` periodically in long-running operations.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Signal cancellation.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Check if cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Constraints for a single step execution (fix #23).
#[derive(Debug, Clone)]
pub struct StepConstraints {
    /// Maximum tokens this step is allowed to consume.
    pub token_budget: u32,
    /// Cooperative cancellation token.
    pub cancel: CancellationToken,
}

impl StepConstraints {
    pub fn new(token_budget: u32) -> Self {
        Self {
            token_budget,
            cancel: CancellationToken::new(),
        }
    }
}

/// Trait for executing individual plan steps. Abstracted for testability.
///
/// In production, the real implementation wraps `OrchestratorRuntime` and
/// dispatches based on `StepExecution` mode. In tests, a stub returns
/// predetermined results.
#[async_trait::async_trait]
pub trait StepExecutor: Send + Sync {
    /// Execute a single plan step with the given context and constraints.
    async fn execute_step(
        &self,
        step: &PlanStep,
        context: &[String],
        constraints: &StepConstraints,
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
        _constraints: &StepConstraints,
    ) -> Result<StepResult, OrchestratorError> {
        let (success, output) = self.overrides.get(&step.name).cloned().unwrap_or_else(|| {
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
            tokens_used: step.estimated_tokens as u64,
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
    /// Agent Teams executor for Teammate steps (#44).
    agent_teams: Option<crate::agent_teams::AgentTeamsExecutor>,
    /// External cancellation token (signaled by session manager on stop hook).
    cancel: Option<CancellationToken>,
}

impl GraphRunner {
    pub fn new(executor: Arc<dyn StepExecutor>, planner: Arc<dyn Planner>) -> Self {
        Self {
            executor,
            planner,
            event_tx: None,
            token_budget: 100_000,
            tokens_used: 0,
            agent_teams: None,
            cancel: None,
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

    /// Set an external cancellation token for cooperative shutdown.
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Enable Agent Teams execution for Teammate steps (#44).
    pub fn with_agent_teams(mut self) -> Self {
        self.agent_teams = Some(crate::agent_teams::AgentTeamsExecutor::new());
        self
    }

    /// Total tokens consumed during execution.
    pub fn tokens_used(&self) -> u64 {
        self.tokens_used
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
            // External cancellation check
            if self.cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
                warn!("graph execution cancelled by external signal");
                break;
            }

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
                    match self
                        .try_replan(&mut plan, planning_context, replan_count)
                        .await
                    {
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
            let current_completed = plan.steps.iter().filter(|s| s.is_terminal()).count();
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
                    warn!("no steps affordable within remaining budget — all steps skipped");
                    if let Some(ref tx) = self.event_tx {
                        let _ = tx.send(OrchestrationEvent::BudgetWarning {
                            resource: format!(
                                "all {} ready steps skipped: estimated {}T > remaining {}T",
                                ready_ids.len(),
                                batch_estimated,
                                remaining
                            ),
                            utilization: if self.token_budget > 0 {
                                self.tokens_used as f64 / self.token_budget as f64
                            } else {
                                1.0
                            },
                        });
                    }
                    break;
                }
                let trimmed_count = ready_ids.len() - affordable.len();
                debug!(trimmed = trimmed_count, "budget-trimmed parallel batch");
                if trimmed_count > 0
                    && let Some(ref tx) = self.event_tx
                {
                    let _ = tx.send(OrchestrationEvent::BudgetWarning {
                        resource: format!(
                            "{} of {} steps trimmed from batch: estimated {}T > 2x remaining {}T",
                            trimmed_count,
                            ready_ids.len(),
                            batch_estimated,
                            remaining
                        ),
                        utilization: if self.token_budget > 0 {
                            self.tokens_used as f64 / self.token_budget as f64
                        } else {
                            1.0
                        },
                    });
                }
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

            self.tokens_used += result.tokens_used;

            if result.success {
                step.output = Some(result.output.clone());

                // Validate step contract before allowing completion (#61 wiring)
                if let Some(ref contract) = step.contract {
                    let validation = contract.validate_outputs(&result.output);
                    if let oco_shared_types::ContractValidation::Violated { missing_fields } =
                        validation
                    {
                        let step_name = step.name.clone();
                        warn!(
                            step = %step_name,
                            missing = ?missing_fields,
                            "step contract violated — required outputs missing"
                        );
                        step.status = StepStatus::Failed {
                            reason: format!(
                                "contract violated: missing {}",
                                missing_fields.join(", ")
                            ),
                        };
                        self.emit_step_completed(
                            result.step_id,
                            &step_name,
                            false,
                            result.duration_ms,
                            result.tokens_used,
                        );
                        continue;
                    }
                }

                if step.verify_after {
                    let verify_result = self.executor.verify_step(step).await;
                    match verify_result {
                        Ok(vr) if vr.success => {
                            self.tokens_used += vr.tokens_used;
                            let step_name = step.name.clone();
                            step.status = StepStatus::Completed;
                            self.emit_step_completed(
                                result.step_id,
                                &step_name,
                                true,
                                result.duration_ms,
                                result.tokens_used,
                            );
                            self.emit_verify_gate_result(
                                result.step_id,
                                &step_name,
                                &vr.output,
                                true,
                                false,
                            );
                        }
                        Ok(vr) => {
                            self.tokens_used += vr.tokens_used;
                            let step_name = step.name.clone();
                            step.status = StepStatus::Failed {
                                reason: vr.output.clone(),
                            };
                            self.emit_step_completed(
                                result.step_id,
                                &step_name,
                                false,
                                result.duration_ms,
                                result.tokens_used,
                            );
                            self.emit_verify_gate_result(
                                result.step_id,
                                &step_name,
                                &vr.output,
                                false,
                                replan_count < MAX_REPLAN_ATTEMPTS,
                            );
                        }
                        Err(e) => {
                            let step_name = step.name.clone();
                            step.status = StepStatus::Failed {
                                reason: e.to_string(),
                            };
                            self.emit_step_completed(
                                result.step_id,
                                &step_name,
                                false,
                                result.duration_ms,
                                result.tokens_used,
                            );
                        }
                    }
                } else {
                    let step_name = step.name.clone();
                    step.status = StepStatus::Completed;
                    self.emit_step_completed(
                        result.step_id,
                        &step_name,
                        true,
                        result.duration_ms,
                        result.tokens_used,
                    );
                }
            } else {
                let step_name = step.name.clone();
                step.status = StepStatus::Failed {
                    reason: result.output.clone(),
                };
                self.emit_step_completed(
                    result.step_id,
                    &step_name,
                    false,
                    result.duration_ms,
                    result.tokens_used,
                );
            }
        }

        // Emit progress after processing all results in a batch
        self.emit_progress(plan);
    }

    /// Execute multiple steps in parallel.
    async fn execute_parallel(&self, plan: &ExecutionPlan, step_ids: &[Uuid]) -> Vec<StepResult> {
        if step_ids.len() == 1 {
            // Single step — no need for join overhead
            let step = plan.get_step(step_ids[0]).expect("step must exist");
            self.emit_step_started(step);

            // ADR-008: if the step has a sub-plan, execute it recursively
            if step.sub_plan.is_some() {
                return vec![self.execute_sub_plan(step).await];
            }

            let constraints = StepConstraints::new(step.estimated_tokens);
            match self.executor.execute_step(step, &[], &constraints).await {
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
            // Multiple steps — run in parallel (sub-plan steps run inline)
            debug!(count = step_ids.len(), "executing parallel steps");
            let mut handles: Vec<(Uuid, tokio::task::JoinHandle<StepResult>)> = Vec::with_capacity(step_ids.len());
            let mut results_inline: Vec<StepResult> = Vec::new();

            for &id in step_ids {
                let step = plan.get_step(id).expect("step must exist").clone();
                self.emit_step_started(&step);

                // ADR-008: steps with sub_plan need &self, so execute them inline
                if step.sub_plan.is_some() {
                    results_inline.push(self.execute_sub_plan(&step).await);
                    continue;
                }

                let executor = self.executor.clone();
                let constraints = StepConstraints::new(step.estimated_tokens);
                handles.push((id, tokio::spawn(async move {
                    match executor.execute_step(&step, &[], &constraints).await {
                        Ok(r) => r,
                        Err(e) => StepResult {
                            step_id: id,
                            success: false,
                            output: e.to_string(),
                            duration_ms: 0,
                            tokens_used: 0,
                        },
                    }
                })));
            }

            let mut results = results_inline;
            for (id, handle) in handles {
                match handle.await {
                    Ok(r) => results.push(r),
                    Err(e) => {
                        // Fix #8: JoinError (panic/cancel) → produce a Failed StepResult
                        warn!(error = %e, "step task panicked or was cancelled");
                        results.push(StepResult {
                            step_id: id,
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

    /// Execute a sub-plan for a parent step (ADR-008).
    /// Walks the sub-plan's DAG, executing ready sub-steps sequentially,
    /// emitting SubPlanStarted/SubStepProgress/SubPlanCompleted events.
    /// Returns an aggregated StepResult for the parent step.
    async fn execute_sub_plan(&self, parent: &PlanStep) -> StepResult {
        let start = std::time::Instant::now();
        let mut sub_plan = *parent.sub_plan.as_ref().unwrap().clone();
        let mut total_tokens: u64 = 0;
        let mut all_success = true;

        // Emit SubPlanStarted
        self.emit_sub_plan_started(parent, &sub_plan);

        // Walk sub-plan DAG until complete
        loop {
            // Cancellation check
            if let Some(ref cancel) = self.cancel
                && cancel.is_cancelled()
            {
                all_success = false;
                break;
            }

            let ready: Vec<Uuid> = sub_plan.ready_steps().iter().map(|s| s.id).collect();
            if ready.is_empty() {
                if !sub_plan.is_complete() {
                    all_success = false;
                }
                break;
            }

            for sub_step_id in &ready {
                // Clone step data before mutating sub_plan
                let sub_step = sub_plan.get_step(*sub_step_id).unwrap().clone();
                let sub_name = sub_step.name.clone();
                let est_tokens = sub_step.estimated_tokens;

                // Emit progress: running
                self.emit_sub_step_progress(parent.id, *sub_step_id, &sub_name, "running");

                // Mark in-progress
                if let Some(s) = sub_plan.get_step_mut(*sub_step_id) {
                    s.status = StepStatus::InProgress;
                }

                // Execute the sub-step
                let constraints = StepConstraints::new(est_tokens);
                let result = self
                    .executor
                    .execute_step(&sub_step, &[], &constraints)
                    .await;

                match result {
                    Ok(r) => {
                        total_tokens += r.tokens_used;
                        let status = if r.success { "passed" } else { "failed" };
                        self.emit_sub_step_progress(parent.id, *sub_step_id, &sub_name, status);
                        if let Some(s) = sub_plan.get_step_mut(*sub_step_id) {
                            s.status = if r.success {
                                StepStatus::Completed
                            } else {
                                all_success = false;
                                StepStatus::Failed {
                                    reason: r.output.clone(),
                                }
                            };
                            s.output = Some(r.output);
                        }
                    }
                    Err(e) => {
                        all_success = false;
                        self.emit_sub_step_progress(parent.id, *sub_step_id, &sub_name, "failed");
                        if let Some(s) = sub_plan.get_step_mut(*sub_step_id) {
                            s.status = StepStatus::Failed {
                                reason: e.to_string(),
                            };
                        }
                    }
                }
            }
        }

        // Emit SubPlanCompleted
        self.emit_sub_plan_completed(parent, all_success);

        StepResult {
            step_id: parent.id,
            success: all_success,
            output: if all_success {
                "sub-plan completed successfully".into()
            } else {
                "sub-plan had failures".into()
            },
            duration_ms: start.elapsed().as_millis() as u64,
            tokens_used: total_tokens,
        }
    }

    /// Attempt to replan after a failure. Returns true if new steps were added.
    async fn try_replan(
        &mut self,
        plan: &mut ExecutionPlan,
        context: &PlanningContext,
        replan_count: u32,
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

        // Budget pre-check: ensure we have at least 15% remaining for replan + new steps
        let remaining = self.token_budget.saturating_sub(self.tokens_used);
        let min_required = self.token_budget / 7; // ~15%
        if remaining < min_required {
            warn!(
                remaining,
                min_required, "insufficient budget for replan, skipping"
            );
            return Ok(false);
        }

        info!(
            step = %failed_step.name,
            error = %error_context,
            "attempting replan"
        );

        let failed_step_name = failed_step.name.clone();
        let old_plan_snapshot = plan.clone();

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

        self.emit_replan_triggered(
            &failed_step_name,
            replan_count + 1,
            &old_plan_snapshot,
            &new_plan,
        );

        // Replace the plan and emit updated plan overview
        *plan = new_plan;
        self.emit_plan_generated(plan);

        Ok(true)
    }

    // -- Event emission helpers --

    fn emit_plan_generated(&self, plan: &ExecutionPlan) {
        if let Some(ref tx) = self.event_tx {
            let execution_mode_str = |step: &PlanStep| -> String {
                match &step.execution {
                    oco_shared_types::StepExecution::Inline => "inline".into(),
                    oco_shared_types::StepExecution::Subagent { model } => {
                        format!("subagent({})", model.as_deref().unwrap_or("default"))
                    }
                    oco_shared_types::StepExecution::Teammate { team_name } => {
                        format!("teammate({team_name})")
                    }
                    oco_shared_types::StepExecution::McpTool { server, tool } => {
                        format!("mcp({server}/{tool})")
                    }
                }
            };

            let steps: Vec<StepSummary> = plan
                .steps
                .iter()
                .filter(|s| s.status != StepStatus::Replanned)
                .map(|s| StepSummary {
                    id: s.id,
                    name: s.name.clone(),
                    description: s.description.clone(),
                    role: s.agent_role.name.clone(),
                    execution_mode: execution_mode_str(s),
                    depends_on: s.depends_on.clone(),
                    verify_after: s.verify_after,
                    estimated_tokens: s.estimated_tokens,
                    preferred_model: s.agent_role.preferred_model.clone(),
                })
                .collect();

            let team = plan.team.as_ref().map(|t| TeamSummary {
                name: t.name.clone(),
                topology: format!("{:?}", t.communication),
                member_count: t.members.len(),
            });

            let _ = tx.send(OrchestrationEvent::PlanGenerated {
                plan_id: plan.id,
                step_count: steps.len(),
                parallel_group_count: plan.parallel_groups().len(),
                critical_path_length: plan.critical_path_length(),
                estimated_total_tokens: plan.estimated_total_tokens(),
                strategy: format!("{:?}", plan.strategy),
                team,
                steps,
            });
        }
    }

    fn emit_step_started(&self, step: &PlanStep) {
        let mode = match &step.execution {
            oco_shared_types::StepExecution::Inline => "inline",
            oco_shared_types::StepExecution::Subagent { .. } => "subagent",
            oco_shared_types::StepExecution::Teammate { .. } => "teammate",
            oco_shared_types::StepExecution::McpTool { .. } => "mcp_tool",
        };
        debug!(
            step_id = %step.id,
            step_name = %step.name,
            execution = ?step.execution,
            "step started"
        );
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(OrchestrationEvent::PlanStepStarted {
                step_id: step.id,
                step_name: step.name.clone(),
                role: step.agent_role.name.clone(),
                execution_mode: mode.into(),
            });
        }
    }

    fn emit_step_completed(
        &self,
        step_id: Uuid,
        step_name: &str,
        success: bool,
        duration_ms: u64,
        tokens_used: u64,
    ) {
        if success {
            info!(step_name, duration_ms, "step completed");
        } else {
            warn!(step_name, duration_ms, "step failed");
        }

        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(OrchestrationEvent::PlanStepCompleted {
                step_id,
                step_name: step_name.into(),
                success,
                duration_ms,
                tokens_used,
            });
        }
    }

    fn emit_progress(&self, plan: &ExecutionPlan) {
        if let Some(ref tx) = self.event_tx {
            let completed = plan
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::Completed)
                .count();
            let total = plan
                .steps
                .iter()
                .filter(|s| s.status != StepStatus::Replanned)
                .count();
            let active: Vec<(Uuid, String)> = plan
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::InProgress)
                .map(|s| (s.id, s.name.clone()))
                .collect();
            let budget_used_pct = if self.token_budget > 0 {
                self.tokens_used as f32 / self.token_budget as f32 * 100.0
            } else {
                0.0
            };

            let _ = tx.send(OrchestrationEvent::PlanProgress {
                completed,
                total,
                active_steps: active,
                budget_used_pct,
            });
        }
    }

    fn emit_verify_gate_result(
        &self,
        step_id: Uuid,
        step_name: &str,
        output: &str,
        passed: bool,
        replan_triggered: bool,
    ) {
        if let Some(ref tx) = self.event_tx {
            // Parse verification output into individual checks when possible.
            // For now, treat the entire output as a single check result.
            let checks = vec![CheckResult {
                check_type: "verification".into(),
                passed,
                summary: if output.len() > 200 {
                    format!("{}...", &output[..197])
                } else {
                    output.into()
                },
            }];

            let _ = tx.send(OrchestrationEvent::VerifyGateResult {
                step_id,
                step_name: step_name.into(),
                checks,
                overall_passed: passed,
                replan_triggered,
            });
        }
    }

    fn emit_replan_triggered(
        &self,
        failed_step_name: &str,
        replan_count: u32,
        old_plan: &ExecutionPlan,
        new_plan: &ExecutionPlan,
    ) {
        let preserved = old_plan
            .steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Completed | StepStatus::InProgress))
            .count();
        let removed = old_plan
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    StepStatus::Failed { .. } | StepStatus::Pending | StepStatus::Blocked
                )
            })
            .count();
        let added = new_plan
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Pending)
            .count();

        info!(
            plan_id = %new_plan.id,
            preserved,
            removed,
            added,
            "replan triggered"
        );

        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(OrchestrationEvent::ReplanTriggered {
                failed_step_name: failed_step_name.into(),
                attempt: replan_count,
                max_attempts: MAX_REPLAN_ATTEMPTS,
                steps_preserved: preserved,
                steps_removed: removed,
                steps_added: added,
            });
        }
    }

    // -- Sub-plan event emission (ADR-008) --

    fn emit_sub_plan_started(&self, parent: &PlanStep, sub_plan: &ExecutionPlan) {
        debug!(parent_step = %parent.name, sub_steps = sub_plan.steps.len(), "sub-plan started");
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(OrchestrationEvent::SubPlanStarted {
                parent_step_id: parent.id,
                parent_step_name: parent.name.clone(),
                sub_steps: sub_plan
                    .steps
                    .iter()
                    .map(|s| (s.id, s.name.clone()))
                    .collect(),
            });
        }
    }

    fn emit_sub_step_progress(
        &self,
        parent_step_id: Uuid,
        sub_step_id: Uuid,
        sub_step_name: &str,
        status: &str,
    ) {
        debug!(parent = %parent_step_id, sub_step = %sub_step_name, status, "sub-step progress");
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(OrchestrationEvent::SubStepProgress {
                parent_step_id,
                sub_step_id,
                sub_step_name: sub_step_name.into(),
                status: status.into(),
            });
        }
    }

    fn emit_sub_plan_completed(&self, parent: &PlanStep, success: bool) {
        debug!(parent_step = %parent.name, success, "sub-plan completed");
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(OrchestrationEvent::SubPlanCompleted {
                parent_step_id: parent.id,
                parent_step_name: parent.name.clone(),
                success,
            });
        }
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
    use oco_shared_types::{PlanStep, PlanStrategy, StepExecution, TaskCategory, TaskComplexity};

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
        assert!(
            result
                .steps
                .iter()
                .all(|s| s.status == StepStatus::Completed)
        );
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
            result
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::Completed)
                .count(),
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
            StubStepExecutor::all_pass().with_failure("verify:implement", "2 tests failing"),
        );
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // After replan, there should be new steps
        assert!(!result.steps.is_empty());
        // The original step should be Failed or Replanned
        assert!(
            result
                .steps
                .iter()
                .any(|s| { matches!(s.status, StepStatus::Failed { .. } | StepStatus::Replanned) })
        );
    }

    // -- Step failure --

    #[tokio::test]
    async fn step_failure_marks_failed() {
        let step = PlanStep::new("broken", "This will fail");
        let plan = make_plan(vec![step]);

        let executor =
            Arc::new(StubStepExecutor::all_pass().with_failure("broken", "runtime error"));
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
        assert!(
            result
                .steps
                .iter()
                .any(|s| s.status == StepStatus::Completed)
        );
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
        assert!(
            events.len() >= 2,
            "expected >= 2 events, got {}",
            events.len()
        );
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

    // -- CancellationToken (fix #23) --

    #[test]
    fn cancellation_token_works() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn step_constraints_carries_budget() {
        let c = StepConstraints::new(5000);
        assert_eq!(c.token_budget, 5000);
        assert!(!c.cancel.is_cancelled());
    }

    #[tokio::test]
    async fn external_cancel_stops_graph_execution() {
        // Setup: 3 sequential steps (a → b → c)
        let a = PlanStep::new("a", "first");
        let b = PlanStep::new("b", "second").with_depends_on(vec![a.id]);
        let c = PlanStep::new("c", "third").with_depends_on(vec![b.id]);
        let plan = make_plan(vec![a, b, c]);

        // Create a cancel token and signal it immediately
        let token = CancellationToken::new();
        token.cancel();

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner)
            .with_budget(50_000)
            .with_cancellation(token);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // Plan should NOT be complete — cancelled before any step ran
        assert!(!result.is_complete());
    }

    #[tokio::test]
    async fn cancel_mid_execution_stops_remaining_steps() {
        // Setup: a → b (sequential)
        let a = PlanStep::new("a", "first");
        let b = PlanStep::new("b", "second").with_depends_on(vec![a.id]);
        let plan = make_plan(vec![a.clone(), b.clone()]);

        let token = CancellationToken::new();

        // Use a custom executor that cancels after step "a" completes
        struct CancelAfterFirstExecutor {
            token: CancellationToken,
        }
        #[async_trait::async_trait]
        impl StepExecutor for CancelAfterFirstExecutor {
            async fn execute_step(
                &self,
                step: &PlanStep,
                _context: &[String],
                _constraints: &StepConstraints,
            ) -> Result<StepResult, OrchestratorError> {
                let result = StepResult {
                    step_id: step.id,
                    success: true,
                    output: format!("done: {}", step.name),
                    duration_ms: 10,
                    tokens_used: 100,
                };
                // Cancel after executing step "a"
                if step.name == "a" {
                    self.token.cancel();
                }
                Ok(result)
            }

            async fn verify_step(&self, step: &PlanStep) -> Result<StepResult, OrchestratorError> {
                Ok(StepResult {
                    step_id: step.id,
                    success: true,
                    output: "verified".into(),
                    duration_ms: 10,
                    tokens_used: 50,
                })
            }
        }

        let executor = Arc::new(CancelAfterFirstExecutor {
            token: token.clone(),
        });
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner)
            .with_budget(50_000)
            .with_cancellation(token);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // Step "a" should have completed, step "b" should NOT
        let step_a = result.get_step(a.id).unwrap();
        assert!(step_a.is_terminal(), "step a should have completed");
        let step_b = result.get_step(b.id).unwrap();
        assert!(!step_b.is_terminal(), "step b should not have run");
    }

    // -- Toxic scenarios (fix #24) --

    #[tokio::test]
    async fn panicking_executor_produces_failed_result() {
        /// Executor that panics only on steps named "panic-step".
        struct SelectivePanicExecutor;

        #[async_trait::async_trait]
        impl StepExecutor for SelectivePanicExecutor {
            async fn execute_step(
                &self,
                step: &PlanStep,
                _context: &[String],
                _constraints: &StepConstraints,
            ) -> Result<StepResult, OrchestratorError> {
                if step.name == "panic-step" {
                    panic!("executor panic!");
                }
                Ok(StepResult {
                    step_id: step.id,
                    success: true,
                    output: format!("Executed: {}", step.name),
                    duration_ms: 10,
                    tokens_used: 100,
                })
            }
            async fn verify_step(&self, _step: &PlanStep) -> Result<StepResult, OrchestratorError> {
                Ok(StepResult {
                    step_id: Uuid::new_v4(),
                    success: true,
                    output: "ok".into(),
                    duration_ms: 0,
                    tokens_used: 0,
                })
            }
        }

        // Both steps depend on root → they run in parallel via tokio::spawn
        // so the panic is caught by JoinError handler (not propagated to test thread)
        let root = PlanStep::new("root", "Setup");
        let a = PlanStep::new("panic-step", "Will panic").with_depends_on(vec![root.id]);
        let b = PlanStep::new("normal-step", "Normal").with_depends_on(vec![root.id]);
        let plan = make_plan(vec![root, a, b]);

        let executor = Arc::new(SelectivePanicExecutor);
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        // Should not crash — panic is caught by JoinError handler in parallel branch
        let result = runner.execute(plan, &ctx()).await.unwrap();
        // The panic step should be marked as failed with "panicked" in reason
        assert!(result.steps.iter().any(|s| {
            matches!(&s.status, StepStatus::Failed { reason } if reason.contains("panic"))
        }));
        // The normal step should have completed
        assert!(
            result
                .steps
                .iter()
                .any(|s| s.name == "normal-step" && s.status == StepStatus::Completed)
        );
    }

    #[tokio::test]
    async fn deadlock_detection_breaks_loop() {
        // Create a plan where after root completes, remaining steps have unresolvable deps
        let root = PlanStep::new("root", "Setup");
        let ghost_dep = Uuid::new_v4(); // doesn't exist in plan
        let stuck =
            PlanStep::new("stuck", "Blocked forever").with_depends_on(vec![root.id, ghost_dep]);
        // Note: validate() would catch this, but GraphRunner should still handle it
        let plan = make_plan(vec![root, stuck]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // Root should complete, stuck should remain pending (deadlock detected)
        assert!(
            result
                .steps
                .iter()
                .any(|s| s.name == "root" && s.status == StepStatus::Completed)
        );
        assert!(
            result
                .steps
                .iter()
                .any(|s| s.name == "stuck" && s.status == StepStatus::Pending)
        );
    }

    #[tokio::test]
    async fn verify_fail_then_replan_then_abort() {
        // Step with verify that fails → triggers replan → DirectPlanner can't help → abort
        let step = PlanStep::new("implement", "Write code").with_verify();
        let plan = make_plan(vec![step]);

        let executor = Arc::new(
            StubStepExecutor::all_pass().with_failure("verify:implement", "all tests fail"),
        );
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // Should eventually stop (max replan attempts or no new steps)
        assert!(
            result
                .steps
                .iter()
                .any(|s| { matches!(s.status, StepStatus::Failed { .. } | StepStatus::Replanned) })
        );
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

    // -- Verify gate token accounting --

    #[tokio::test]
    async fn verify_gate_tokens_counted_in_budget() {
        let step = PlanStep::new("impl", "Implement feature")
            .with_verify()
            .with_estimated_tokens(1000);
        let plan = make_plan(vec![step]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let _result = runner.execute(plan, &ctx()).await.unwrap();
        // Step tokens (1000) + verify gate tokens (500 from StubStepExecutor)
        assert_eq!(runner.tokens_used(), 1500);
    }

    #[tokio::test]
    async fn verify_gate_failure_tokens_counted_in_budget() {
        let step = PlanStep::new("impl", "Implement feature")
            .with_verify()
            .with_estimated_tokens(1000);
        let plan = make_plan(vec![step]);

        let executor =
            Arc::new(StubStepExecutor::all_pass().with_failure("verify:impl", "tests failed"));
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert!(result.has_failures());
        // Step tokens (1000) + failed verify gate tokens (500)
        assert_eq!(runner.tokens_used(), 1500);
    }

    // -- Sub-plan execution (ADR-008) --

    #[tokio::test]
    async fn sub_plan_executes_sub_steps() {
        let sub_a = PlanStep::new("sub-scan", "Scan files");
        let mut sub_b = PlanStep::new("sub-summarize", "Summarize");
        sub_b.depends_on = vec![sub_a.id];
        let sub_plan = ExecutionPlan::new(vec![sub_a, sub_b], PlanStrategy::Direct);

        let parent = PlanStep::new("delegate", "Delegate to subagent")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(sub_plan);
        let plan = make_plan(vec![parent]);

        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].status, StepStatus::Completed);
    }

    #[tokio::test]
    async fn sub_plan_emits_events() {
        let sub_step = PlanStep::new("sub-task", "Do sub-work");
        let sub_plan = ExecutionPlan::new(vec![sub_step], PlanStrategy::Direct);

        let parent = PlanStep::new("parent", "Parent step")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(sub_plan);
        let plan = make_plan(vec![parent]);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let executor = Arc::new(StubStepExecutor::all_pass());
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner)
            .with_budget(50_000)
            .with_event_channel(tx);

        let _result = runner.execute(plan, &ctx()).await.unwrap();

        let mut found_sub_started = false;
        let mut found_sub_progress = false;
        let mut found_sub_completed = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                OrchestrationEvent::SubPlanStarted { .. } => found_sub_started = true,
                OrchestrationEvent::SubStepProgress { .. } => found_sub_progress = true,
                OrchestrationEvent::SubPlanCompleted { success, .. } => {
                    found_sub_completed = true;
                    assert!(success);
                }
                _ => {}
            }
        }
        assert!(found_sub_started, "should emit SubPlanStarted");
        assert!(found_sub_progress, "should emit SubStepProgress");
        assert!(found_sub_completed, "should emit SubPlanCompleted");
    }

    #[tokio::test]
    async fn sub_plan_failure_propagates() {
        let sub_step = PlanStep::new("sub-failing", "Will fail");
        let sub_plan = ExecutionPlan::new(vec![sub_step.clone()], PlanStrategy::Direct);

        let parent = PlanStep::new("parent", "Parent step")
            .with_execution(StepExecution::Subagent { model: None })
            .with_sub_plan(sub_plan);
        let plan = make_plan(vec![parent]);

        let executor =
            Arc::new(StubStepExecutor::all_pass().with_failure("sub-failing", "sub-step error"));
        let planner = Arc::new(DirectPlanner);
        let mut runner = GraphRunner::new(executor, planner).with_budget(50_000);

        let result = runner.execute(plan, &ctx()).await.unwrap();
        // Parent step should be marked as failed because sub-plan failed
        assert!(result.has_failures());
    }
}
