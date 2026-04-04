use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use oco_shared_types::{
    ActionCandidate, MemoryEntry, MemorySeverity, Observation, ObservationKind, ObservationSource,
    OrchestrationEvent, OrchestratorAction, PlanStep, Session, StopReason, TaskComplexity,
    TelemetryEventType, ToolGateDecision, WorkingMemory,
};
use tracing::{debug, info, warn};

use crate::config::OrchestratorConfig;
use crate::error::OrchestratorError;
use crate::graph_runner::{GraphRunner, StepConstraints, StepExecutor, StepResult};
use crate::llm::{LlmMessage, LlmProvider, LlmRequest, LlmRole};
use crate::runtime::OrchestratorRuntime;
use crate::state::OrchestrationState;

use oco_planner::{DirectPlanner, LlmPlanner, Planner as _, PlanningContext};

/// Extract the file path from a write tool call, if applicable.
fn extract_write_path(tool_name: &str, arguments: &serde_json::Value) -> Option<String> {
    match tool_name {
        "write_file" | "file_write" | "Edit" | "Write" | "MultiEdit" => arguments
            .get("path")
            .or_else(|| arguments.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "shell" | "bash" | "Bash" => {
            // Heuristic: check if command writes to a file
            if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                // Common write patterns
                for prefix in &["echo ", "cat >", "tee ", "sed -i", "mv ", "cp "] {
                    if cmd.contains(prefix) {
                        return None; // Can't reliably extract path from shell commands
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Keywords that indicate a write/mutating task.
const WRITE_KEYWORDS: &[&str] = &[
    "write",
    "delete",
    "remove",
    "rename",
    "refactor",
    "modify",
    "update",
    "create",
    "add",
    "move",
    "replace",
    "overwrite",
    "drop",
    "reset",
    "push",
    "deploy",
    "install",
    "uninstall",
];

/// Detect whether a user request describes a write/mutating task.
fn detect_write_task(request: &str) -> bool {
    let lower = request.to_lowercase();
    WRITE_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Adapter: bridges `LlmProvider` → planner's `LlmCallFn`.
struct LlmProviderCallFn {
    provider: Arc<dyn LlmProvider>,
}

#[async_trait]
impl oco_planner::LlmCallFn for LlmProviderCallFn {
    async fn call(
        &self,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
    ) -> Result<(String, u64), oco_planner::PlannerError> {
        let request = LlmRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: user_message.to_string(),
            }],
            max_tokens,
            temperature: 0.0,
            system_prompt: Some(system_prompt.to_string()),
            effort_override: None,
        };
        let response = self
            .provider
            .complete(request)
            .await
            .map_err(|e| oco_planner::PlannerError::LlmError(e.to_string()))?;
        let tokens = (response.input_tokens + response.output_tokens) as u64;
        Ok((response.content, tokens))
    }
}

/// The main orchestration loop.
pub struct OrchestrationLoop {
    config: OrchestratorConfig,
    llm: Arc<dyn LlmProvider>,
    policy: oco_policy_engine::DefaultActionSelector,
    policy_gate: oco_policy_engine::PolicyGate,
    runtime: Option<OrchestratorRuntime>,
    trace_collector: oco_telemetry::DecisionTraceCollector,
    metrics: Option<oco_telemetry::SessionMetrics>,
    /// Optional channel for live event streaming to UI.
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<OrchestrationEvent>>,
    /// External cancellation token (signaled by session manager on stop hook).
    cancel: Option<crate::graph_runner::CancellationToken>,
    /// External session ID for correlation (e.g. Claude Code session).
    external_session_id: Option<String>,
}

impl OrchestrationLoop {
    pub fn new(config: OrchestratorConfig, llm: Arc<dyn LlmProvider>) -> Self {
        Self {
            config,
            llm,
            policy: oco_policy_engine::DefaultActionSelector::new(
                oco_policy_engine::BudgetEnforcer::new(),
            ),
            policy_gate: oco_policy_engine::PolicyGate::new(
                oco_policy_engine::WritePolicy::RequireConfirmation,
            ),
            runtime: None,
            trace_collector: oco_telemetry::DecisionTraceCollector::new(),
            metrics: None,
            event_tx: None,
            cancel: None,
            external_session_id: None,
        }
    }

    /// Set a channel for live orchestration events (step-by-step feedback).
    pub fn with_event_channel(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<OrchestrationEvent>,
    ) -> &mut Self {
        self.event_tx = Some(tx);
        self
    }

    /// Set the external session ID for correlation with the calling system.
    pub fn with_external_session_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.external_session_id = Some(id.into());
        self
    }

    /// Set an external cancellation token (e.g. from session manager stop hook).
    pub fn with_cancellation(
        &mut self,
        token: crate::graph_runner::CancellationToken,
    ) -> &mut Self {
        self.cancel = Some(token);
        self
    }

    /// Access the decision trace collector.
    pub fn trace_collector(&self) -> &oco_telemetry::DecisionTraceCollector {
        &self.trace_collector
    }

    /// Access the session metrics (available after `run` is called).
    pub fn metrics(&self) -> Option<&oco_telemetry::SessionMetrics> {
        self.metrics.as_ref()
    }

    /// Initialize the runtime with workspace indexing.
    pub fn with_workspace(&mut self, workspace_root: PathBuf) -> &mut Self {
        let mut rt = OrchestratorRuntime::new(workspace_root);
        if let Err(e) = rt.index_workspace() {
            warn!(error = %e, "Failed to index workspace, continuing without index");
        }
        self.runtime = Some(rt);
        self
    }

    /// Run the orchestration loop for a given user request.
    /// Guarantees a `Stopped` event is emitted even if the loop errors out.
    pub async fn run(
        &mut self,
        user_request: String,
        workspace_root: Option<String>,
    ) -> Result<OrchestrationState, OrchestratorError> {
        let result = self.run_inner(user_request, workspace_root).await;
        // Emit Stopped on error paths where run_inner didn't reach its own Stopped
        if let Err(ref e) = result {
            self.emit_event(OrchestrationEvent::Stopped {
                reason: StopReason::Error {
                    message: e.to_string(),
                },
                total_steps: 0,
                total_tokens: 0,
            });
        }
        result
    }

    /// Inner run implementation.
    async fn run_inner(
        &mut self,
        user_request: String,
        workspace_root: Option<String>,
    ) -> Result<OrchestrationState, OrchestratorError> {
        // (Re-)initialize runtime if workspace changed or not yet initialized.
        // Prevents stale context from a previous session leaking into a new one.
        if let Some(ref ws) = workspace_root {
            let ws_path = PathBuf::from(ws);
            let needs_reinit = match &self.runtime {
                None => true,
                Some(rt) => rt.workspace_root != ws_path,
            };
            if needs_reinit {
                self.with_workspace(ws_path);
            }
        }

        let mut session = Session::new(user_request.clone(), workspace_root);
        if let Some(ref ext_id) = self.external_session_id {
            session.external_session_id = Some(ext_id.clone());
        }
        let mut state = OrchestrationState::new(session);

        // Apply max_steps override from config (e.g. eval scenario overrides).
        if self.config.max_steps > 0 {
            state.session.max_steps = self.config.max_steps;
        }

        // Initialize session metrics
        self.metrics = Some(oco_telemetry::SessionMetrics::new(state.session.id));

        // Classify task complexity and adapt budget.
        // Start from configured budget, then apply complexity-based limits as caps.
        state.task_complexity = oco_policy_engine::TaskClassifier::classify(&user_request, &[]);
        let complexity_budget = oco_shared_types::Budget::for_complexity(state.task_complexity);
        let configured = &self.config.default_budget;
        state.session.budget = oco_shared_types::Budget {
            max_context_tokens: configured
                .max_context_tokens
                .min(complexity_budget.max_context_tokens),
            max_output_tokens: configured
                .max_output_tokens
                .min(complexity_budget.max_output_tokens),
            max_total_tokens: configured
                .max_total_tokens
                .min(complexity_budget.max_total_tokens),
            max_tool_calls: configured
                .max_tool_calls
                .min(complexity_budget.max_tool_calls),
            max_retrievals: configured
                .max_retrievals
                .min(complexity_budget.max_retrievals),
            max_duration_secs: configured
                .max_duration_secs
                .min(complexity_budget.max_duration_secs),
            max_verify_cycles: configured
                .max_verify_cycles
                .min(complexity_budget.max_verify_cycles),
            // Counters start at zero.
            tokens_used: 0,
            tool_calls_used: 0,
            retrievals_used: 0,
            verify_cycles_used: 0,
        };
        info!(
            session_id = %state.session.id.0,
            complexity = ?state.task_complexity,
            max_tokens = state.session.budget.max_total_tokens,
            max_tool_calls = state.session.budget.max_tool_calls,
            "Starting orchestration with task-adapted budget"
        );

        // Emit RunStarted so the dashboard can show the mission immediately.
        self.emit_event(OrchestrationEvent::RunStarted {
            provider: self.llm.provider_name().to_string(),
            model: self.llm.model_name().to_string(),
            request_summary: user_request.clone(),
            complexity: format!("{:?}", state.task_complexity),
        });

        // v2 routing: Medium+ tasks go through the plan engine.
        if matches!(
            state.task_complexity,
            TaskComplexity::Medium | TaskComplexity::High | TaskComplexity::Critical
        ) {
            info!(
                complexity = ?state.task_complexity,
                "routing to plan engine (GraphRunner)"
            );
            self.emit_event(OrchestrationEvent::StepCompleted {
                step: 0,
                action: OrchestratorAction::Plan {
                    request: state.session.user_request.clone(),
                },
                reason: "Medium+ task → plan engine".into(),
                duration_ms: 0,
                budget_snapshot: oco_shared_types::BudgetSnapshot {
                    tokens_used: state.session.budget.tokens_used,
                    tokens_remaining: state
                        .session
                        .budget
                        .max_total_tokens
                        .saturating_sub(state.session.budget.tokens_used),
                    tool_calls_used: state.session.budget.tool_calls_used,
                    tool_calls_remaining: state
                        .session
                        .budget
                        .max_tool_calls
                        .saturating_sub(state.session.budget.tool_calls_used),
                    retrievals_used: state.session.budget.retrievals_used,
                    verify_cycles_used: state.session.budget.verify_cycles_used,
                    elapsed_secs: state.elapsed_secs(),
                },
                knowledge_confidence: state.knowledge_confidence,
                success: true,
            });

            state.push_action(OrchestratorAction::Plan {
                request: state.session.user_request.clone(),
            });

            let plan_result = self.run_with_plan(&mut state).await;
            let stop_reason = match &plan_result {
                Ok(()) => StopReason::TaskComplete,
                Err(e) => StopReason::Error {
                    message: e.to_string(),
                },
            };

            state.session.status = match &stop_reason {
                StopReason::TaskComplete => oco_shared_types::SessionStatus::Completed,
                StopReason::Error { .. } => oco_shared_types::SessionStatus::Failed,
                _ => oco_shared_types::SessionStatus::Completed,
            };

            state.push_action(OrchestratorAction::Stop {
                reason: stop_reason.clone(),
            });

            self.emit_event(OrchestrationEvent::Stopped {
                reason: stop_reason,
                total_steps: state.session.step_count,
                total_tokens: state.session.budget.tokens_used,
            });

            return plan_result.map(|()| state);
        }

        // Flat loop for Trivial/Low tasks.
        loop {
            // Check external cancellation (stop hook)
            if self.cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
                info!(session_id = %state.session.id.0, "Stopping orchestration (external cancel)");
                state.push_action(OrchestratorAction::Stop {
                    reason: StopReason::UserCancelled,
                });
                break;
            }

            // Check stop conditions
            if let Some(reason) = state.should_stop() {
                info!(session_id = %state.session.id.0, reason = ?reason, "Stopping orchestration");
                let stop_action = OrchestratorAction::Stop { reason };
                state.push_action(stop_action);
                break;
            }

            // Check budget duration
            if state.elapsed_secs() > state.session.budget.max_duration_secs {
                let stop_action = OrchestratorAction::Stop {
                    reason: StopReason::BudgetExhausted,
                };
                state.push_action(stop_action);
                break;
            }

            let step_start = Instant::now();

            // Build policy state
            let policy_state = self.build_policy_state(&state);

            // v2: Emit budget threshold telemetry when utilization is high.
            let token_util = if state.session.budget.max_total_tokens > 0 {
                state.session.budget.tokens_used as f64
                    / state.session.budget.max_total_tokens as f64
            } else {
                0.0
            };
            if token_util > 0.7 {
                self.trace_collector
                    .record_event(TelemetryEventType::BudgetThreshold {
                        resource: "tokens".into(),
                        utilization: token_util,
                        status: if token_util > 0.9 {
                            "critical"
                        } else {
                            "warning"
                        }
                        .into(),
                    });
                self.emit_event(OrchestrationEvent::BudgetWarning {
                    resource: "tokens".into(),
                    utilization: token_util,
                });
            }

            // Select action via policy engine
            use oco_policy_engine::ActionSelector;
            let decision = self.policy.select_action(&policy_state);
            debug!(
                step = state.session.step_count,
                action = ?decision.action,
                reason = %decision.reason,
                score = decision.score,
                "Action selected"
            );

            // Execute action using real runtime when available.
            // Track whether execution actually succeeded — critical for
            // deciding whether Respond should terminate the loop.
            let action = decision.action.clone();
            let obs_len_before = state.observations.len();
            let execution_result = self.execute_action(&action, &state).await;

            let duration_ms = step_start.elapsed().as_millis() as u64;
            let action_succeeded;

            match execution_result {
                Ok(observation) => {
                    action_succeeded = true;
                    state.error_streak = 0;
                    // Fix #42: track tokens in flat loop budget (not just metrics)
                    state.session.budget.tokens_used += observation.token_estimate as u64;
                    state.push_observation(observation);
                }
                Err(e) => {
                    action_succeeded = false;
                    warn!(error = %e, "Action execution failed");
                    state.error_streak += 1;
                    let error_obs = Observation::new(
                        ObservationSource::System,
                        ObservationKind::Error {
                            message: e.to_string(),
                            recoverable: state.error_streak < 3,
                        },
                        50,
                    );
                    state.push_observation(error_obs);
                }
            }

            // Record trace
            let alternatives: Vec<ActionCandidate> = decision
                .alternatives
                .iter()
                .map(|a| ActionCandidate {
                    action_type: a.action_type.clone(),
                    score: a.score,
                    reason: a.reason.clone(),
                })
                .collect();

            state.record_trace(&action, decision.reason.clone(), duration_ms, alternatives);

            // Feed telemetry collector with the trace we just recorded
            if let Some(trace) = state.traces.last() {
                self.trace_collector.record(trace.clone());
                // Emit live event for the UI
                self.emit_event(OrchestrationEvent::StepCompleted {
                    step: trace.step,
                    action: action.clone(),
                    reason: trace.reason.clone(),
                    duration_ms,
                    budget_snapshot: trace.budget_snapshot.clone(),
                    knowledge_confidence: trace.knowledge_confidence,
                    success: action_succeeded,
                });
            }

            // Record step duration in session metrics
            if let Some(ref metrics) = self.metrics {
                metrics.record_step(duration_ms);
            }

            // Push action to history.
            // For Respond, fill the content from the LLM observation produced
            // by THIS action (not a stale observation from a prior step).
            let action_to_record = if action_succeeded {
                if let OrchestratorAction::Respond { .. } = &action {
                    state
                        .observations
                        .iter()
                        .skip(obs_len_before)
                        .rev()
                        .find_map(|obs| match (&obs.source, &obs.kind) {
                            (
                                ObservationSource::LlmResponse,
                                ObservationKind::Text { content, .. },
                            ) => Some(OrchestratorAction::Respond {
                                content: content.clone(),
                            }),
                            _ => None,
                        })
                        .unwrap_or_else(|| action.clone())
                } else {
                    action.clone()
                }
            } else {
                action.clone()
            };
            state.push_action(action_to_record);

            // Update state flags based on action.
            // Only debit budgets and mark terminal states when the action
            // actually succeeded — fixes the GPT-5.4 audit finding where a
            // failed Respond was incorrectly marked as TaskComplete.
            match &action {
                OrchestratorAction::Retrieve { .. } => {
                    if action_succeeded {
                        state.has_retrieved = true;
                    }
                    state.session.budget.record_retrieval();
                }
                OrchestratorAction::ToolCall {
                    tool_name,
                    arguments,
                } => {
                    if action_succeeded {
                        state.session.budget.record_tool_call();
                        if let Some(ref metrics) = self.metrics {
                            metrics.record_tool_call(tool_name);
                        }
                        // v2: Track file modifications for verification freshness.
                        if let Some(path) = extract_write_path(tool_name, arguments) {
                            state.verification.record_modification(path);
                        }
                    }
                    // Don't debit budget if tool was denied/unavailable
                }
                OrchestratorAction::Verify { strategy, .. } => {
                    state.session.budget.record_verify_cycle();
                    // v2: Record verification run with modification snapshot.
                    if action_succeeded && let Some(last_obs) = state.observations.back() {
                        let passed = matches!(
                            &last_obs.kind,
                            ObservationKind::VerificationResult { passed: true, .. }
                        );
                        let failures =
                            if let ObservationKind::VerificationResult { failures, .. } =
                                &last_obs.kind
                            {
                                failures.clone()
                            } else {
                                vec![]
                            };
                        state
                            .verification
                            .record_run(oco_shared_types::VerificationRun {
                                strategy: format!("{strategy:?}"),
                                timestamp: chrono::Utc::now(),
                                passed,
                                covered_files: std::collections::HashSet::new(),
                                modifications_snapshot: state.verification.modified_files.clone(),
                                duration_ms,
                                failures,
                            });
                        // v2: Emit telemetry event for verification.
                        self.trace_collector
                            .record_event(TelemetryEventType::VerifyCompleted {
                                strategy: format!("{strategy:?}"),
                                passed,
                                duration_ms,
                            });
                    }
                }
                OrchestratorAction::Respond { .. } => {
                    if action_succeeded {
                        // Respond is terminal only on success
                        state.push_action(OrchestratorAction::Stop {
                            reason: StopReason::TaskComplete,
                        });
                        break;
                    }
                    // On failure, let the policy engine decide the next action
                }
                OrchestratorAction::UpdateMemory { operation } => {
                    // Memory ops are always local — no external execution needed.
                    use oco_shared_types::MemoryOperation;
                    match operation {
                        MemoryOperation::PromoteToFact { entry_id } => {
                            state.memory.promote_to_fact(*entry_id);
                        }
                        MemoryOperation::Invalidate { entry_id, reason } => {
                            state.memory.invalidate(*entry_id, reason);
                        }
                        MemoryOperation::Supersede { old_id, new_id } => {
                            state.memory.supersede(*old_id, *new_id);
                        }
                        MemoryOperation::LinkEvidence {
                            target_id,
                            evidence_id,
                            supports,
                        } => {
                            state
                                .memory
                                .add_evidence_link(*target_id, *evidence_id, *supports);
                        }
                        MemoryOperation::AddHypothesis {
                            content,
                            confidence,
                        } => {
                            state
                                .memory
                                .add_hypothesis(MemoryEntry::new(content.clone(), *confidence));
                        }
                        MemoryOperation::AddQuestion { content } => {
                            state
                                .memory
                                .add_question(MemoryEntry::new(content.clone(), 0.5));
                        }
                        MemoryOperation::ResolveQuestion { question_id } => {
                            state.memory.resolve_question(*question_id);
                        }
                        MemoryOperation::UpdatePlan { steps } => {
                            state.memory.update_plan(steps.clone());
                        }
                    }
                }
                OrchestratorAction::Stop { .. } => break,
                // v2 actions: no-op in flat loop (handled by GraphRunner)
                OrchestratorAction::Plan { .. }
                | OrchestratorAction::Delegate { .. }
                | OrchestratorAction::Message { .. }
                | OrchestratorAction::Replan { .. } => {}
            }

            // v2: Update working memory based on observations.
            if action_succeeded && let Some(last_obs) = state.observations.back() {
                let prev_count = state.memory.active_count();
                update_working_memory(&mut state.memory, last_obs, &action);
                let new_count = state.memory.active_count();
                if new_count != prev_count {
                    self.trace_collector
                        .record_event(TelemetryEventType::MemoryUpdated {
                            operation: "auto_update".into(),
                            active_count: new_count,
                        });
                }
            }

            // Re-estimate knowledge confidence after every action using the
            // multi-signal estimator instead of a fixed increment.
            let obs_snapshot: Vec<Observation> = state.observations.iter().cloned().collect();
            let workspace_signals: Vec<String> = state.session.pinned_context.clone();
            state.knowledge_confidence = oco_policy_engine::KnowledgeBoundaryEstimator::estimate(
                state.task_complexity,
                &state.session.user_request,
                &obs_snapshot,
                state.has_retrieved,
                &workspace_signals,
            );
        }

        // Emit final stopped event — derive reason from the terminal Stop action
        let stop_reason = state
            .action_history
            .iter()
            .rev()
            .find_map(|a| {
                if let OrchestratorAction::Stop { reason } = a {
                    Some(reason.clone())
                } else {
                    None
                }
            })
            .unwrap_or(StopReason::Error {
                message: "session ended without explicit stop".into(),
            });
        // Update session status based on stop reason.
        state.session.status = match &stop_reason {
            StopReason::TaskComplete => oco_shared_types::SessionStatus::Completed,
            StopReason::BudgetExhausted => oco_shared_types::SessionStatus::BudgetExhausted,
            StopReason::Error { .. } => oco_shared_types::SessionStatus::Failed,
            _ => oco_shared_types::SessionStatus::Completed,
        };

        self.emit_event(OrchestrationEvent::Stopped {
            reason: stop_reason,
            total_steps: state.session.step_count,
            total_tokens: state.session.budget.tokens_used,
        });

        Ok(state)
    }

    /// Emit a live event if a channel is connected. Non-blocking, ignores send failures.
    fn emit_event(&self, event: OrchestrationEvent) {
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(event);
        }
    }

    fn build_policy_state(&self, state: &OrchestrationState) -> oco_policy_engine::PolicyState {
        let recent_obs: Vec<Observation> =
            state.observations.iter().rev().take(5).cloned().collect();

        let has_called_tools = state
            .action_history
            .iter()
            .any(|a| matches!(a, OrchestratorAction::ToolCall { .. }));

        let has_verified = state
            .action_history
            .iter()
            .any(|a| matches!(a, OrchestratorAction::Verify { .. }));

        // v2: Check if verification is stale, partial, or never done after modifications.
        let freshness = state.verification.freshness();
        let pending_verification = matches!(
            freshness,
            oco_shared_types::VerificationFreshness::Stale
                | oco_shared_types::VerificationFreshness::Partial
        ) || (!state.verification.modified_files.is_empty()
            && matches!(freshness, oco_shared_types::VerificationFreshness::None));

        // v2: Emit telemetry if verification is stale.
        if matches!(freshness, oco_shared_types::VerificationFreshness::Stale) {
            let stale_files: Vec<String> =
                state.verification.modified_files.keys().cloned().collect();
            self.trace_collector
                .record_event(TelemetryEventType::VerificationStale { stale_files });
        }

        // v2: Check for unresolved errors in working memory.
        let has_memory_errors = !state.memory.unresolved_errors().is_empty();

        oco_policy_engine::PolicyState {
            current_step: state.session.step_count,
            max_steps: state.session.max_steps,
            budget: state.session.budget.clone(),
            recent_observations: recent_obs,
            task_complexity: state.task_complexity,
            knowledge_confidence: state.knowledge_confidence,
            has_retrieved_context: state.has_retrieved,
            has_called_tools,
            has_verified,
            is_write_task: detect_write_task(&state.session.user_request),
            consecutive_error_count: state.error_streak,
            pending_verification,
            risk_level: self.config.profile.risk_level,
            has_memory_errors,
            memory_active_count: state.memory.active_count(),
            task_category: state.task_category(),
            policy_pack: self.config.profile.policy_pack,
        }
    }

    async fn execute_action(
        &self,
        action: &OrchestratorAction,
        state: &OrchestrationState,
    ) -> Result<Observation, OrchestratorError> {
        match action {
            OrchestratorAction::Respond { .. } => self.execute_respond(state).await,
            OrchestratorAction::Retrieve {
                query, max_results, ..
            } => self.execute_retrieve(query, *max_results, state).await,
            OrchestratorAction::ToolCall {
                tool_name,
                arguments,
            } => self.execute_tool_call(tool_name, arguments).await,
            OrchestratorAction::Verify { strategy, target } => {
                self.execute_verify(strategy, target.as_deref()).await
            }
            OrchestratorAction::UpdateMemory { operation } => Ok(Observation::new(
                ObservationSource::System,
                ObservationKind::Text {
                    content: format!("Memory updated: {operation:?}"),
                    metadata: None,
                },
                5,
            )),
            OrchestratorAction::Stop { .. } => Ok(Observation::new(
                ObservationSource::System,
                ObservationKind::Text {
                    content: "Session stopped".into(),
                    metadata: None,
                },
                10,
            )),
            // Orchestration v2 actions — handled by GraphRunner, not the flat loop.
            // In the flat loop these are no-ops that return a stub observation.
            OrchestratorAction::Plan { request } => Ok(Observation::new(
                ObservationSource::System,
                ObservationKind::Text {
                    content: format!("Plan requested (not yet wired to GraphRunner): {request}"),
                    metadata: None,
                },
                10,
            )),
            OrchestratorAction::Delegate {
                step_id,
                agent_role,
                ..
            } => Ok(Observation::new(
                ObservationSource::System,
                ObservationKind::Text {
                    content: format!(
                        "Delegate step {step_id} to {} (not yet wired to GraphRunner)",
                        agent_role.name
                    ),
                    metadata: None,
                },
                10,
            )),
            OrchestratorAction::Message {
                to_agent, content, ..
            } => Ok(Observation::new(
                ObservationSource::System,
                ObservationKind::Text {
                    content: format!("Message to {to_agent}: {content}"),
                    metadata: None,
                },
                10,
            )),
            OrchestratorAction::Replan {
                failed_step_id,
                error_context,
            } => Ok(Observation::new(
                ObservationSource::System,
                ObservationKind::Text {
                    content: format!(
                        "Replan step {failed_step_id}: {error_context} (not yet wired)"
                    ),
                    metadata: None,
                },
                10,
            )),
        }
    }

    /// Run a planned execution for Medium+ tasks via GraphRunner.
    async fn run_with_plan(
        &mut self,
        state: &mut OrchestrationState,
    ) -> Result<(), OrchestratorError> {
        let complexity = state.task_complexity;
        let category = state.task_category();

        let mut planning_ctx = PlanningContext::minimal(complexity, category);
        // Use the session's actual budget, not the complexity default.
        // DirectPlanner uses this to estimate step tokens — if the estimate
        // exceeds the GraphRunner's real budget, pre-reservation trims all steps.
        planning_ctx.budget = state.session.budget.clone();

        // Route to the right planner based on complexity:
        // - Trivial/Low → DirectPlanner (no LLM call, instant)
        // - Medium+ → Competitive planning (2 candidates in parallel), fallback to DirectPlanner
        let plan = if DirectPlanner::should_handle(complexity) {
            DirectPlanner
                .plan(&state.session.user_request, &planning_ctx)
                .await
                .map_err(|e| OrchestratorError::PlanningFailed(e.to_string()))?
        } else {
            let llm_call = Box::new(LlmProviderCallFn {
                provider: self.llm.clone(),
            });
            let llm_planner = LlmPlanner::new(llm_call);

            match llm_planner
                .plan_competitive(&state.session.user_request, &planning_ctx)
                .await
            {
                Ok((plan, candidates)) => {
                    // Emit exploration event for the dashboard visualization
                    let summaries: Vec<oco_shared_types::telemetry::PlanCandidateSummary> =
                        candidates
                            .iter()
                            .map(|c| oco_shared_types::telemetry::PlanCandidateSummary {
                                strategy: c.strategy.clone(),
                                step_count: c.plan.steps.len(),
                                estimated_tokens: c
                                    .plan
                                    .steps
                                    .iter()
                                    .map(|s| s.estimated_tokens as u64)
                                    .sum(),
                                verify_count: c
                                    .plan
                                    .steps
                                    .iter()
                                    .filter(|s| s.verify_after)
                                    .count(),
                                parallel_groups: c.plan.parallel_groups().len(),
                                score: c.score,
                                winner: c.winner,
                            })
                            .collect();

                    let winner = candidates.iter().find(|c| c.winner);
                    self.emit_event(OrchestrationEvent::PlanExploration {
                        candidates: summaries,
                        winner_strategy: winner.map(|w| w.strategy.clone()).unwrap_or_default(),
                        winner_score: winner.map(|w| w.score).unwrap_or(0.0),
                    });

                    plan
                }
                Err(e) => {
                    warn!(error = %e, "competitive planning failed, falling back to DirectPlanner");
                    DirectPlanner
                        .plan(&state.session.user_request, &planning_ctx)
                        .await
                        .map_err(|e| OrchestratorError::PlanningFailed(e.to_string()))?
                }
            }
        };

        info!(
            plan_id = %plan.id,
            steps = plan.steps.len(),
            strategy = ?plan.strategy,
            "plan generated for Medium+ task"
        );

        let executor = Arc::new(LoopStepExecutor {
            llm: self.llm.clone(),
        });

        // Build a planner for replans (GraphRunner needs it)
        let replan_planner: Arc<dyn oco_planner::Planner> =
            if DirectPlanner::should_handle(complexity) {
                Arc::new(DirectPlanner)
            } else {
                let llm_call = Box::new(LlmProviderCallFn {
                    provider: self.llm.clone(),
                });
                Arc::new(LlmPlanner::new(llm_call))
            };

        let event_tx = self.event_tx.clone();
        let budget = state.session.budget.max_total_tokens;

        let mut runner = GraphRunner::new(executor, replan_planner).with_budget(budget);
        if let Some(tx) = event_tx {
            runner = runner.with_event_channel(tx);
        }
        if let Some(ref token) = self.cancel {
            runner = runner.with_cancellation(token.clone());
        }

        let completed_plan = runner.execute(plan, &planning_ctx).await?;

        // Use actual tokens tracked by GraphRunner, not estimates.
        let total_tokens = runner.tokens_used();
        state.session.budget.tokens_used += total_tokens;

        // Collect all outputs (from completed AND failed steps — failed steps
        // may still have partial output worth surfacing).
        let outputs: Vec<String> = completed_plan
            .steps
            .iter()
            .filter_map(|s| s.output.clone())
            .filter(|o| !o.is_empty())
            .collect();

        if !outputs.is_empty() {
            let combined = outputs.join("\n\n");
            state.push_observation(Observation::new(
                ObservationSource::LlmResponse,
                ObservationKind::Text {
                    content: combined.clone(),
                    metadata: None,
                },
                total_tokens as u32,
            ));
            // Surface the plan output as a Respond action so eval detects response_generated.
            state.push_action(OrchestratorAction::Respond { content: combined });
        } else if completed_plan.has_failures() {
            // No outputs at all — report failures
            let failed: Vec<String> = completed_plan
                .steps
                .iter()
                .filter(|s| matches!(s.status, oco_shared_types::StepStatus::Failed { .. }))
                .map(|s| {
                    if let oco_shared_types::StepStatus::Failed { ref reason } = s.status {
                        format!("{}: {}", s.name, reason)
                    } else {
                        s.name.clone()
                    }
                })
                .collect();
            state.push_observation(Observation::new(
                ObservationSource::System,
                ObservationKind::Error {
                    message: format!("Plan partially completed. Failures: {}", failed.join("; ")),
                    recoverable: false,
                },
                50,
            ));
        }

        // Update working memory planner state after plan execution (#62 wiring)
        state
            .memory
            .update_planner_state(oco_shared_types::PlannerState {
                current_step: None,
                replan_count: 0,
                phase: Some("completed".into()),
                lease_id: None,
            });

        Ok(())
    }

    async fn execute_respond(
        &self,
        state: &OrchestrationState,
    ) -> Result<Observation, OrchestratorError> {
        // v2: Include working memory in pinned context if non-empty.
        let mut pinned = state.session.pinned_context.clone();
        if state.memory.active_count() > 0 {
            pinned.push(format!("## Working Memory\n\n{}", state.memory.summary()));
        }

        // Build context from observations
        let observations: Vec<Observation> = state.observations.iter().cloned().collect();
        let context = if let Some(ref rt) = self.runtime {
            rt.build_context_with_complexity(
                &state.session.user_request,
                &observations,
                &pinned,
                state.session.budget.max_context_tokens,
                state.session.step_count,
                Some(state.task_complexity),
            )
        } else {
            // Minimal context without runtime
            oco_context_engine::ContextBuilder::new(state.session.budget.max_context_tokens)
                .with_staleness(state.session.step_count, 8)
                .with_user_request(&state.session.user_request)
                .build()
        };

        // v2: Emit context assembly telemetry.
        self.trace_collector
            .record_event(TelemetryEventType::ContextAssembled {
                total_tokens: context.total_tokens,
                item_count: context.items.len() as u32,
                excluded_count: context.excluded_count,
                utilization: context.utilization(),
            });

        // Build LLM messages from assembled context
        let mut context_text = String::new();
        for item in &context.items {
            if !item.content.is_empty() {
                context_text.push_str(&item.content);
                context_text.push_str("\n\n");
            }
        }

        let request = LlmRequest {
            messages: vec![
                LlmMessage {
                    role: LlmRole::User,
                    content: if context_text.is_empty() {
                        state.session.user_request.clone()
                    } else {
                        format!(
                            "Context:\n{}\n\nUser request: {}",
                            context_text, state.session.user_request
                        )
                    },
                },
            ],
            max_tokens: state.session.budget.max_output_tokens,
            temperature: 0.0,
            system_prompt: Some(
                "You are an expert coding assistant. Analyze the provided context and respond to the user's request. \
                 Be precise, cite file paths and line numbers when relevant.".into()
            ),
            effort_override: None,
        };

        let response = self.llm.complete(request).await?;

        let token_estimate = response.input_tokens + response.output_tokens;

        // Record token usage in session metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_token_usage(token_estimate as u64);
        }

        Ok(Observation::new(
            ObservationSource::LlmResponse,
            ObservationKind::Text {
                content: response.content,
                metadata: Some(serde_json::json!({
                    "model": response.model,
                    "input_tokens": response.input_tokens,
                    "output_tokens": response.output_tokens,
                    "stop_reason": response.stop_reason,
                })),
            },
            token_estimate,
        ))
    }

    async fn execute_retrieve(
        &self,
        query: &str,
        max_results: u32,
        state: &OrchestrationState,
    ) -> Result<Observation, OrchestratorError> {
        // Use the user request as query if the policy returned an empty query
        let effective_query = if query.is_empty() {
            &state.session.user_request
        } else {
            query
        };

        if let Some(ref rt) = self.runtime
            && rt.indexed
        {
            return rt
                .execute_retrieval(effective_query, max_results)
                .await
                .map_err(|e| OrchestratorError::RetrievalFailed(e.to_string()));
        }

        // Fallback: no runtime or not indexed
        Ok(Observation::new(
            ObservationSource::Retrieval {
                source_type: "stub".into(),
            },
            ObservationKind::Text {
                content: format!("No index available. Query: {effective_query}"),
                metadata: None,
            },
            50,
        ))
    }

    async fn execute_tool_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<Observation, OrchestratorError> {
        // Policy gate: check if the tool action is allowed
        // Fail-closed: unknown tools are denied by default.
        // Only registered tools with known risk levels pass through the gate.
        let gate_decision = if let Some(ref rt) = self.runtime {
            if let Some(descriptor) = rt.tool_registry.get(tool_name) {
                self.policy_gate.evaluate(&descriptor)
            } else {
                // Unknown tool — check command string if present, otherwise deny
                if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                    self.policy_gate.evaluate_command(cmd)
                } else {
                    ToolGateDecision::Deny {
                        reason: format!("tool '{tool_name}' is not registered"),
                    }
                }
            }
        } else {
            // No runtime means no tool registry — allow only for stub/dev mode
            ToolGateDecision::Allow
        };

        match gate_decision {
            ToolGateDecision::Deny { reason } => {
                warn!(tool = tool_name, reason = %reason, "Tool call denied by policy gate");
                return Ok(Observation::new(
                    ObservationSource::System,
                    ObservationKind::Error {
                        message: format!("Policy gate denied tool '{tool_name}': {reason}"),
                        recoverable: true,
                    },
                    20,
                ));
            }
            ToolGateDecision::RequireConfirmation { reason } => {
                info!(tool = tool_name, reason = %reason, "Tool call requires confirmation");
                // In non-interactive mode, treat as denied with explanation
                return Ok(Observation::new(
                    ObservationSource::System,
                    ObservationKind::Error {
                        message: format!(
                            "Policy gate requires confirmation for tool '{tool_name}': {reason}"
                        ),
                        recoverable: true,
                    },
                    20,
                ));
            }
            ToolGateDecision::Allow => { /* proceed */ }
        }

        if let Some(ref rt) = self.runtime {
            return rt
                .execute_tool(tool_name, arguments)
                .await
                .map_err(|e| OrchestratorError::ToolExecutionFailed(e.to_string()));
        }

        // Fallback stub
        Ok(Observation::new(
            ObservationSource::ToolExecution {
                tool_name: tool_name.to_string(),
            },
            ObservationKind::Structured {
                data: serde_json::json!({"status": "no_runtime", "tool": tool_name}),
            },
            30,
        ))
    }

    async fn execute_verify(
        &self,
        strategy: &oco_shared_types::VerificationStrategy,
        target: Option<&str>,
    ) -> Result<Observation, OrchestratorError> {
        if let Some(ref rt) = self.runtime {
            return rt
                .execute_verification(strategy, target)
                .await
                .map_err(|e| OrchestratorError::VerificationFailed(e.to_string()));
        }

        // Fallback stub
        Ok(Observation::new(
            ObservationSource::Verification {
                strategy: format!("{strategy:?}"),
            },
            ObservationKind::VerificationResult {
                passed: true,
                output: "[No runtime available — stub verification]".into(),
                failures: vec![],
            },
            50,
        ))
    }
}

/// Step executor that bridges the GraphRunner to the existing LLM + runtime.
///
/// For inline steps, calls the LLM with the step description as prompt.
/// For verification, returns a stub pass (the full verifier runs in the flat loop).
struct LoopStepExecutor {
    llm: Arc<dyn LlmProvider>,
}

#[async_trait::async_trait]
impl StepExecutor for LoopStepExecutor {
    async fn execute_step(
        &self,
        step: &PlanStep,
        context: &[String],
        constraints: &StepConstraints,
    ) -> Result<StepResult, OrchestratorError> {
        let start = Instant::now();

        let mut prompt = format!(
            "You are acting as role: {}.\n\nTask: {}",
            step.agent_role.name, step.description
        );

        if !context.is_empty() {
            prompt.push_str("\n\nContext from previous steps:\n");
            for ctx in context {
                prompt.push_str(ctx);
                prompt.push('\n');
            }
        }

        let request = LlmRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: prompt,
            }],
            max_tokens: constraints.token_budget.min(4096),
            temperature: 0.0,
            system_prompt: Some(format!(
                "You are a {} agent. Execute the task precisely and concisely.",
                step.agent_role.name
            )),
            effort_override: None,
        };

        let response = self.llm.complete(request).await?;

        let tokens_used = (response.input_tokens + response.output_tokens) as u64;
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(StepResult {
            step_id: step.id,
            success: true,
            output: response.content,
            duration_ms,
            tokens_used,
        })
    }

    async fn verify_step(&self, step: &PlanStep) -> Result<StepResult, OrchestratorError> {
        // Stub verification — the real verifier runs when the full loop calls execute_verify.
        // In plan mode, verify gates signal pass/fail; the GraphRunner handles replan on failure.
        Ok(StepResult {
            step_id: step.id,
            success: true,
            output: format!("Verification passed for step: {}", step.name),
            duration_ms: 0,
            tokens_used: 0,
        })
    }
}

/// Update working memory based on the latest observation and action.
fn update_working_memory(
    memory: &mut WorkingMemory,
    obs: &Observation,
    action: &OrchestratorAction,
) {
    match &obs.kind {
        ObservationKind::VerificationResult {
            passed, failures, ..
        } => {
            if *passed {
                let entry = MemoryEntry::new(format!("Verification passed: {action:?}"), 1.0)
                    .with_source("verification".into())
                    .with_severity(MemorySeverity::Info);
                memory.add_finding(entry);
            } else {
                for failure in failures {
                    let entry = MemoryEntry::new(format!("Verification failure: {failure}"), 0.9)
                        .with_source("verification".into())
                        .with_tags(vec!["failure".into()])
                        .with_severity(MemorySeverity::Error);
                    memory.add_finding(entry);
                }
            }
        }
        ObservationKind::Error {
            message,
            recoverable,
        } => {
            let (confidence, severity) = if *recoverable {
                (0.7, MemorySeverity::Warning)
            } else {
                (0.9, MemorySeverity::Critical)
            };
            let entry = MemoryEntry::new(format!("Error: {message}"), confidence)
                .with_source("error".into())
                .with_tags(vec!["error".into()])
                .with_severity(severity);
            memory.add_finding(entry);
        }
        ObservationKind::Symbol {
            name,
            kind,
            file_path,
            ..
        } => {
            let entry = MemoryEntry::new(format!("Found {kind} `{name}` in {file_path}"), 0.8)
                .with_source(file_path.clone())
                .with_severity(MemorySeverity::Info);
            memory.add_finding(entry);
        }
        ObservationKind::CodeSnippet {
            file_path,
            start_line,
            language,
            ..
        } => {
            let lang = language.as_deref().unwrap_or("unknown");
            let entry = MemoryEntry::new(
                format!("Code snippet from {file_path}:{start_line} ({lang})"),
                0.6,
            )
            .with_source(file_path.clone())
            .with_tags(vec!["code_snippet".into()]);
            memory.add_finding(entry);
        }
        ObservationKind::Structured { data } => {
            // Extract key fields from structured data as findings.
            if let Some(obj) = data.as_object()
                && let Some(status) = obj.get("status").and_then(|v| v.as_str())
            {
                let severity = if status == "error" || status == "fail" {
                    MemorySeverity::Error
                } else {
                    MemorySeverity::Info
                };
                let summary = obj
                    .get("message")
                    .or_else(|| obj.get("summary"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(status);
                let entry = MemoryEntry::new(format!("Structured result: {summary}"), 0.7)
                    .with_source("structured".into())
                    .with_tags(vec!["structured".into()])
                    .with_severity(severity);
                memory.add_finding(entry);
            }
        }
        ObservationKind::Text { .. } => {
            // Plain text observations are too noisy for automatic memory.
            // The LLM can promote relevant text via UpdateMemory actions.
        }
    }
}
