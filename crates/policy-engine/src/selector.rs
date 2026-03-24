use oco_shared_types::{
    Budget, Observation, ObservationKind, ObservationSource, OrchestratorAction, RetrievalSource,
    RiskLevel, StopReason, TaskCategory, TaskComplexity, VerificationStrategy,
};
use serde::{Deserialize, Serialize};

use crate::budget::{BudgetEnforcer, BudgetStatus};

/// Snapshot of the orchestrator state used for policy decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyState {
    /// Current step number in the orchestration loop.
    pub current_step: u32,
    /// Maximum steps allowed.
    pub max_steps: u32,
    /// Current budget.
    pub budget: Budget,
    /// Recent observations (most recent last).
    pub recent_observations: Vec<Observation>,
    /// Classified complexity of the current task.
    pub task_complexity: TaskComplexity,
    /// Estimated knowledge confidence (0.0 to 1.0).
    pub knowledge_confidence: f64,
    /// Whether context retrieval has been performed.
    pub has_retrieved_context: bool,
    /// Whether any tool has been called.
    pub has_called_tools: bool,
    /// Whether verification has been done this cycle.
    pub has_verified: bool,
    /// Whether the task involves write/destructive operations.
    pub is_write_task: bool,
    /// Number of consecutive errors in recent observations.
    pub consecutive_error_count: u32,
    /// Whether the most recent tool output needs verification.
    pub pending_verification: bool,
    /// v2: Repo risk level (affects verification strictness).
    #[serde(default)]
    pub risk_level: RiskLevel,
    /// v2: Whether working memory has unresolved errors.
    #[serde(default)]
    pub has_memory_errors: bool,
    /// v2: Number of active working memory entries.
    #[serde(default)]
    pub memory_active_count: usize,
    /// v2: Task category for skill recommendation routing.
    #[serde(default)]
    pub task_category: TaskCategory,
}

/// A scored action alternative considered during selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredAlternative {
    pub action_type: String,
    pub score: f64,
    pub reason: String,
}

/// The result of action selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDecision {
    /// The selected action to take.
    pub action: OrchestratorAction,
    /// Confidence score for this decision (0.0 to 1.0).
    pub score: f64,
    /// Human-readable reason for this selection.
    pub reason: String,
    /// Other actions that were considered with their scores.
    pub alternatives: Vec<ScoredAlternative>,
    /// Optional skill recommendation based on task category and complexity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_recommendation: Option<String>,
}

/// Trait for action selection policies.
pub trait ActionSelector: Send + Sync {
    fn select_action(&self, state: &PolicyState) -> ActionDecision;
}

/// Default rule-based action selector.
///
/// Decision priority:
/// 1. Budget exhausted -> Stop
/// 2. Max errors -> Stop with error
/// 3. No context retrieved yet and task needs it -> Retrieve
/// 4. Pending verification (especially for write tasks) -> Verify
/// 5. Budget critical -> Respond immediately
/// 6. High confidence and enough context -> Respond
/// 7. Otherwise -> Retrieve or ToolCall based on task type
pub struct DefaultActionSelector {
    enforcer: BudgetEnforcer,
}

impl DefaultActionSelector {
    pub fn new(enforcer: BudgetEnforcer) -> Self {
        Self { enforcer }
    }

    /// Determine if the task likely needs context retrieval.
    fn task_needs_retrieval(complexity: TaskComplexity) -> bool {
        matches!(
            complexity,
            TaskComplexity::Medium | TaskComplexity::High | TaskComplexity::Critical
        )
    }

    /// Determine if the latest observation looks like a completed response.
    fn looks_complete(observations: &[Observation]) -> bool {
        if let Some(last) = observations.last() {
            matches!(last.source, ObservationSource::LlmResponse)
                && matches!(last.kind, ObservationKind::Text { .. })
        } else {
            false
        }
    }

    /// Check if the last tool output had failures that need verification.
    fn last_tool_had_issues(observations: &[Observation]) -> bool {
        observations.iter().rev().take(3).any(|o| {
            matches!(
                &o.kind,
                ObservationKind::VerificationResult { passed: false, .. }
            ) || matches!(
                &o.kind,
                ObservationKind::Error {
                    recoverable: true,
                    ..
                }
            )
        })
    }

    fn score_retrieve(state: &PolicyState) -> (f64, String) {
        let mut score: f64 = 0.0;
        let mut reasons = Vec::new();

        // Trivial/Low tasks with adequate confidence should NOT retrieve
        if state.task_complexity <= TaskComplexity::Low && state.knowledge_confidence >= 0.5 {
            reasons.push("trivial/low task with adequate confidence, retrieval unnecessary");
            return (0.0, reasons.join("; "));
        }

        if !state.has_retrieved_context && Self::task_needs_retrieval(state.task_complexity) {
            score += 0.8;
            reasons.push("no context retrieved yet for complex task");
        }

        if state.knowledge_confidence < 0.5 && !state.has_retrieved_context {
            score += 0.3;
            reasons.push("low knowledge confidence");
        }

        if state.consecutive_error_count > 0 && state.has_called_tools {
            score += 0.2;
            reasons.push("errors suggest missing context");
        }

        // Diminishing returns: penalize repeated retrievals past 3
        let retrievals_used = state.budget.retrievals_used;
        if retrievals_used > 3 {
            let penalty = 0.15 * (retrievals_used - 3) as f64;
            score -= penalty;
            reasons.push("diminishing returns on repeated retrievals");
        }

        // Budget awareness: if retrieval budget > 50% used, reduce score
        let retrieval_utilization = if state.budget.max_retrievals > 0 {
            retrievals_used as f64 / state.budget.max_retrievals as f64
        } else {
            1.0
        };
        if retrieval_utilization > 0.5 {
            let budget_penalty = 0.3 * (retrieval_utilization - 0.5) / 0.5;
            score -= budget_penalty;
            reasons.push("retrieval budget partially consumed");
        }

        // After retrieval has happened, lower the base urge to retrieve again
        if state.has_retrieved_context {
            score -= 0.3;
            reasons.push("context already retrieved");
        }

        let reason = if reasons.is_empty() {
            "retrieval not strongly indicated".to_string()
        } else {
            reasons.join("; ")
        };
        (score.clamp(0.0, 1.0), reason)
    }

    fn score_verify(state: &PolicyState) -> (f64, String) {
        let mut score: f64 = 0.0;
        let mut reasons = Vec::new();

        if state.pending_verification {
            score += 0.7;
            reasons.push("pending verification from tool output");
        }

        if state.is_write_task && state.has_called_tools && !state.has_verified {
            score += 0.6;
            reasons.push("write task needs verification after tool call");
        }

        if Self::last_tool_had_issues(&state.recent_observations) {
            score += 0.4;
            reasons.push("recent tool output had issues");
        }

        // v2: Higher risk level increases verification urgency.
        match state.risk_level {
            RiskLevel::High => {
                if state.has_called_tools && !state.has_verified {
                    score += 0.3;
                    reasons.push("high risk repo requires verification");
                }
            }
            RiskLevel::Critical => {
                if state.has_called_tools && !state.has_verified {
                    score += 0.5;
                    reasons.push("critical risk repo demands verification");
                }
            }
            _ => {}
        }

        // v2: Working memory errors boost verify urgency.
        if state.has_memory_errors && !state.has_verified {
            score += 0.2;
            reasons.push("working memory has unresolved errors");
        }

        let reason = if reasons.is_empty() {
            "verification not needed".to_string()
        } else {
            reasons.join("; ")
        };
        (score.min(1.0), reason)
    }

    fn score_respond(state: &PolicyState) -> (f64, String) {
        let mut score: f64 = 0.0;
        let mut reasons = Vec::new();

        // Trivial tasks with adequate confidence should respond immediately
        if state.task_complexity == TaskComplexity::Trivial && state.knowledge_confidence >= 0.5 {
            score += 0.9;
            reasons.push("trivial task with sufficient confidence, respond immediately");
        }

        if state.knowledge_confidence > 0.8
            && (state.has_retrieved_context || !Self::task_needs_retrieval(state.task_complexity))
        {
            score += 0.7;
            reasons.push("high confidence with sufficient context");
        }

        if state.task_complexity <= TaskComplexity::Low && state.knowledge_confidence > 0.6 {
            score += 0.5;
            reasons.push("simple task with adequate confidence");
        }

        // Big boost after retrieval: context is available, lean towards responding
        if state.has_retrieved_context {
            score += 0.4;
            reasons.push("context retrieved, ready to synthesize response");
        }

        // If we've verified and everything passed, time to respond
        if state.has_verified
            && !Self::last_tool_had_issues(&state.recent_observations)
            && state.has_called_tools
        {
            score += 0.6;
            reasons.push("verification passed, ready to respond");
        }

        // v2: Unresolved memory errors reduce respond confidence.
        if state.has_memory_errors {
            score -= 0.2;
            reasons.push("working memory has unresolved errors, reducing respond urgency");
        }

        // v2: High/Critical risk without verification penalizes respond.
        if matches!(state.risk_level, RiskLevel::High | RiskLevel::Critical)
            && state.has_called_tools
            && !state.has_verified
        {
            score -= 0.3;
            reasons.push("high risk repo without verification, deferring response");
        }

        let reason = if reasons.is_empty() {
            "not enough information to respond confidently".to_string()
        } else {
            reasons.join("; ")
        };
        (score.clamp(0.0, 1.0), reason)
    }

    fn score_tool_call(state: &PolicyState) -> (f64, String) {
        let mut score: f64 = 0.0;
        let mut reasons = Vec::new();

        if state.has_retrieved_context
            && !state.has_called_tools
            && state.task_complexity >= TaskComplexity::Medium
        {
            score += 0.6;
            reasons.push("context retrieved, task needs tool execution");
        }

        if state.task_complexity >= TaskComplexity::High && state.consecutive_error_count == 0 {
            score += 0.2;
            reasons.push("complex task may benefit from tool use");
        }

        let reason = if reasons.is_empty() {
            "tool call not indicated".to_string()
        } else {
            reasons.join("; ")
        };
        (score.min(1.0), reason)
    }

    /// Determine a skill recommendation based on task category and complexity.
    fn recommend_skill(category: TaskCategory, complexity: TaskComplexity) -> Option<String> {
        match (category, complexity) {
            (TaskCategory::Bug, TaskComplexity::High | TaskComplexity::Critical) => {
                Some("oco-investigate-bug".to_string())
            }
            (TaskCategory::Bug, TaskComplexity::Medium) => {
                Some("oco-trace-stack".to_string())
            }
            (TaskCategory::Refactor, TaskComplexity::Medium | TaskComplexity::High | TaskComplexity::Critical) => {
                Some("oco-safe-refactor".to_string())
            }
            (TaskCategory::Security, _) => Some("security-review".to_string()),
            (TaskCategory::Frontend, TaskComplexity::Medium | TaskComplexity::High | TaskComplexity::Critical) => {
                Some("ultimate-design-system".to_string())
            }
            (TaskCategory::Testing, _) => Some("test-driven-development".to_string()),
            (TaskCategory::Review, _) => Some("code-review".to_string()),
            _ => None,
        }
    }

    fn score_stop(state: &PolicyState) -> (f64, String) {
        let mut score: f64 = 0.0;
        let mut reasons = Vec::new();

        if Self::looks_complete(&state.recent_observations) && state.has_verified {
            score += 0.8;
            reasons.push("task appears complete with verification");
        }

        if state.current_step >= state.max_steps.saturating_sub(1) {
            score += 0.9;
            reasons.push("approaching max steps");
        }

        let reason = if reasons.is_empty() {
            "task not yet complete".to_string()
        } else {
            reasons.join("; ")
        };
        (score.min(1.0), reason)
    }
}

impl ActionSelector for DefaultActionSelector {
    fn select_action(&self, state: &PolicyState) -> ActionDecision {
        let budget_report = self.enforcer.check(&state.budget);

        // Hard stops: budget exhausted or too many errors
        let skill = Self::recommend_skill(state.task_category, state.task_complexity);

        if budget_report.status == BudgetStatus::Exhausted {
            return ActionDecision {
                action: OrchestratorAction::Stop {
                    reason: StopReason::BudgetExhausted,
                },
                score: 1.0,
                reason: format!(
                    "budget exhausted (limiting: {})",
                    budget_report.limiting_factors.join(", ")
                ),
                alternatives: vec![],
                skill_recommendation: None,
            };
        }

        if state.consecutive_error_count >= 3 {
            return ActionDecision {
                action: OrchestratorAction::Stop {
                    reason: StopReason::Error {
                        message: format!(
                            "{} consecutive errors, stopping to avoid loops",
                            state.consecutive_error_count
                        ),
                    },
                },
                score: 1.0,
                reason: "too many consecutive errors".to_string(),
                alternatives: vec![],
                skill_recommendation: None,
            };
        }

        if state.current_step >= state.max_steps {
            return ActionDecision {
                action: OrchestratorAction::Stop {
                    reason: StopReason::MaxStepsReached,
                },
                score: 1.0,
                reason: "max steps reached".to_string(),
                alternatives: vec![],
                skill_recommendation: None,
            };
        }

        // Budget critical: wrap up quickly
        if budget_report.status >= BudgetStatus::Critical {
            let (respond_score, respond_reason) = Self::score_respond(state);
            return ActionDecision {
                action: OrchestratorAction::Respond {
                    content: String::new(), // Content to be filled by LLM
                },
                score: respond_score.max(0.8),
                reason: format!(
                    "budget critical ({}), forcing response: {}",
                    budget_report.limiting_factors.join(", "),
                    respond_reason
                ),
                alternatives: vec![ScoredAlternative {
                    action_type: "stop".to_string(),
                    score: 0.7,
                    reason: "could also stop to conserve budget".to_string(),
                }],
                skill_recommendation: skill.clone(),
            };
        }

        // Normal scoring: compute scores for all actions
        let (retrieve_score, retrieve_reason) = Self::score_retrieve(state);
        let (verify_score, verify_reason) = Self::score_verify(state);
        let (respond_score, respond_reason) = Self::score_respond(state);
        let (tool_score, tool_reason) = Self::score_tool_call(state);
        let (stop_score, stop_reason) = Self::score_stop(state);

        let mut candidates: Vec<(f64, &str, String)> = vec![
            (retrieve_score, "retrieve", retrieve_reason),
            (verify_score, "verify", verify_reason),
            (respond_score, "respond", respond_reason),
            (tool_score, "tool_call", tool_reason),
            (stop_score, "stop", stop_reason),
        ];

        // Sort by score descending
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let (best_score, best_type, best_reason) = candidates.remove(0);

        let alternatives: Vec<ScoredAlternative> = candidates
            .iter()
            .filter(|(score, _, _)| *score > 0.0)
            .map(|(score, action_type, reason)| ScoredAlternative {
                action_type: action_type.to_string(),
                score: *score,
                reason: reason.clone(),
            })
            .collect();

        let action = match best_type {
            "retrieve" => OrchestratorAction::Retrieve {
                query: String::new(), // To be filled by orchestrator
                sources: vec![RetrievalSource::CodeSearch, RetrievalSource::SemanticSearch],
                max_results: match state.task_complexity {
                    TaskComplexity::Trivial | TaskComplexity::Low => 5,
                    TaskComplexity::Medium => 10,
                    TaskComplexity::High => 15,
                    TaskComplexity::Critical => 20,
                },
            },
            "verify" => OrchestratorAction::Verify {
                strategy: if state.is_write_task {
                    VerificationStrategy::Build
                } else {
                    VerificationStrategy::RunTests
                },
                target: None,
            },
            "respond" => OrchestratorAction::Respond {
                content: String::new(),
            },
            "tool_call" => OrchestratorAction::ToolCall {
                tool_name: String::new(), // To be filled by orchestrator
                arguments: serde_json::Value::Null,
            },
            "stop" => OrchestratorAction::Stop {
                reason: StopReason::TaskComplete,
            },
            _ => unreachable!("unknown action type"),
        };

        ActionDecision {
            action,
            score: best_score,
            reason: best_reason,
            alternatives,
            skill_recommendation: skill,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> PolicyState {
        PolicyState {
            current_step: 0,
            max_steps: 25,
            budget: Budget::default(),
            recent_observations: vec![],
            task_complexity: TaskComplexity::Medium,
            knowledge_confidence: 0.5,
            has_retrieved_context: false,
            has_called_tools: false,
            has_verified: false,
            is_write_task: false,
            consecutive_error_count: 0,
            pending_verification: false,
            risk_level: RiskLevel::Standard,
            has_memory_errors: false,
            memory_active_count: 0,
            task_category: TaskCategory::General,
        }
    }

    #[test]
    fn first_step_complex_task_retrieves() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let state = default_state();
        let decision = selector.select_action(&state);
        assert!(
            matches!(decision.action, OrchestratorAction::Retrieve { .. }),
            "expected Retrieve, got {:?}",
            decision.action
        );
    }

    #[test]
    fn trivial_task_high_confidence_responds() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_complexity = TaskComplexity::Trivial;
        state.knowledge_confidence = 0.9;
        let decision = selector.select_action(&state);
        assert!(
            matches!(decision.action, OrchestratorAction::Respond { .. }),
            "expected Respond, got {:?}",
            decision.action
        );
    }

    #[test]
    fn exhausted_budget_stops() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.budget.tokens_used = state.budget.max_total_tokens;
        let decision = selector.select_action(&state);
        assert!(matches!(
            decision.action,
            OrchestratorAction::Stop {
                reason: StopReason::BudgetExhausted
            }
        ));
    }

    #[test]
    fn consecutive_errors_stop() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.consecutive_error_count = 3;
        let decision = selector.select_action(&state);
        assert!(matches!(
            decision.action,
            OrchestratorAction::Stop {
                reason: StopReason::Error { .. }
            }
        ));
    }

    #[test]
    fn pending_verification_verifies() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.has_retrieved_context = true;
        state.has_called_tools = true;
        state.pending_verification = true;
        state.is_write_task = true;
        let decision = selector.select_action(&state);
        assert!(
            matches!(decision.action, OrchestratorAction::Verify { .. }),
            "expected Verify, got {:?}",
            decision.action
        );
    }

    #[test]
    fn max_steps_stops() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.current_step = 25;
        let decision = selector.select_action(&state);
        assert!(matches!(
            decision.action,
            OrchestratorAction::Stop {
                reason: StopReason::MaxStepsReached
            }
        ));
    }

    #[test]
    fn decision_has_alternatives() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        // Set up a state where multiple actions score > 0:
        // has context but hasn't called tools yet, medium complexity, moderate confidence
        state.has_retrieved_context = true;
        state.knowledge_confidence = 0.85;
        state.task_complexity = TaskComplexity::Medium;
        let decision = selector.select_action(&state);
        // Should have considered multiple alternatives
        assert!(
            !decision.alternatives.is_empty(),
            "expected alternatives to be populated, got action: {:?}",
            decision.action
        );
    }

    #[test]
    fn critical_risk_boosts_verify_score() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.has_retrieved_context = true;
        state.has_called_tools = true;
        state.knowledge_confidence = 0.7;
        state.is_write_task = true;

        // Standard risk: verify score baseline
        state.risk_level = RiskLevel::Standard;
        let decision_standard = selector.select_action(&state);
        let verify_score_standard = decision_standard
            .alternatives
            .iter()
            .find(|a| a.action_type == "verify")
            .map(|a| a.score)
            .unwrap_or(decision_standard.score);

        // Critical risk: verify score should be higher
        state.risk_level = RiskLevel::Critical;
        let decision_critical = selector.select_action(&state);

        // With critical risk, verify should either be the selected action
        // or have a higher score than at standard risk
        let is_verify = matches!(decision_critical.action, OrchestratorAction::Verify { .. });
        let verify_score_critical = if is_verify {
            decision_critical.score
        } else {
            decision_critical
                .alternatives
                .iter()
                .find(|a| a.action_type == "verify")
                .map(|a| a.score)
                .unwrap_or(0.0)
        };
        assert!(
            verify_score_critical > verify_score_standard || is_verify,
            "critical risk should boost verify score"
        );
    }

    // --- Skill recommendation tests ---

    #[test]
    fn skill_bug_high_recommends_investigate() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_category = TaskCategory::Bug;
        state.task_complexity = TaskComplexity::High;
        let decision = selector.select_action(&state);
        assert_eq!(
            decision.skill_recommendation.as_deref(),
            Some("oco-investigate-bug")
        );
    }

    #[test]
    fn skill_bug_medium_recommends_trace() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_category = TaskCategory::Bug;
        state.task_complexity = TaskComplexity::Medium;
        let decision = selector.select_action(&state);
        assert_eq!(
            decision.skill_recommendation.as_deref(),
            Some("oco-trace-stack")
        );
    }

    #[test]
    fn skill_refactor_high_recommends_safe_refactor() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_category = TaskCategory::Refactor;
        state.task_complexity = TaskComplexity::High;
        let decision = selector.select_action(&state);
        assert_eq!(
            decision.skill_recommendation.as_deref(),
            Some("oco-safe-refactor")
        );
    }

    #[test]
    fn skill_security_any_complexity_recommends_review() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_category = TaskCategory::Security;
        state.task_complexity = TaskComplexity::Low;
        let decision = selector.select_action(&state);
        assert_eq!(
            decision.skill_recommendation.as_deref(),
            Some("security-review")
        );
    }

    #[test]
    fn skill_frontend_medium_recommends_design_system() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_category = TaskCategory::Frontend;
        state.task_complexity = TaskComplexity::Medium;
        let decision = selector.select_action(&state);
        assert_eq!(
            decision.skill_recommendation.as_deref(),
            Some("ultimate-design-system")
        );
    }

    #[test]
    fn skill_testing_recommends_tdd() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.task_category = TaskCategory::Testing;
        let decision = selector.select_action(&state);
        assert_eq!(
            decision.skill_recommendation.as_deref(),
            Some("test-driven-development")
        );
    }

    #[test]
    fn skill_general_no_recommendation() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let state = default_state();
        let decision = selector.select_action(&state);
        assert_eq!(decision.skill_recommendation, None);
    }

    #[test]
    fn memory_errors_reduce_respond_score() {
        let selector = DefaultActionSelector::new(BudgetEnforcer::new());
        let mut state = default_state();
        state.has_retrieved_context = true;
        state.knowledge_confidence = 0.85;
        state.has_memory_errors = true;
        // With memory errors, respond score should be lower
        let decision_with_errors = selector.select_action(&state);

        state.has_memory_errors = false;
        let decision_without_errors = selector.select_action(&state);

        // The score with errors should be lower than without
        assert!(
            decision_with_errors.score <= decision_without_errors.score,
            "memory errors should reduce decision score"
        );
    }
}
