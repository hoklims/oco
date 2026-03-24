use chrono::{DateTime, Utc};
use oco_shared_types::{
    ActionCandidate, AssembledContext, BudgetSnapshot, DecisionTrace, Observation,
    OrchestratorAction, Session, TaskCategory, TaskComplexity, VerificationState, WorkingMemory,
};
use std::collections::VecDeque;
use uuid::Uuid;

/// Full orchestration state for a single session.
#[derive(Debug, Clone)]
pub struct OrchestrationState {
    pub session: Session,
    /// History of actions taken.
    pub action_history: Vec<OrchestratorAction>,
    /// Recent observations (bounded ring buffer).
    pub observations: VecDeque<Observation>,
    /// Maximum observations to keep in memory.
    pub max_observations: usize,
    /// Current context window.
    pub current_context: Option<AssembledContext>,
    /// Decision traces for this session.
    pub traces: Vec<DecisionTrace>,
    /// Assessed task complexity.
    pub task_complexity: TaskComplexity,
    /// Current knowledge confidence estimate.
    pub knowledge_confidence: f64,
    /// Whether retrieval has been performed at least once.
    pub has_retrieved: bool,
    /// Count of consecutive errors.
    pub error_streak: u32,
    /// Start time for duration tracking.
    pub started_at: DateTime<Utc>,
    /// v2: Verification state — tracks modifications and verification freshness.
    pub verification: VerificationState,
    /// v2: Working memory — structured findings, hypotheses, facts.
    pub memory: WorkingMemory,
}

impl OrchestrationState {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            action_history: Vec::new(),
            observations: VecDeque::with_capacity(50),
            max_observations: 50,
            current_context: None,
            traces: Vec::new(),
            task_complexity: TaskComplexity::Medium,
            knowledge_confidence: 0.5,
            has_retrieved: false,
            error_streak: 0,
            started_at: Utc::now(),
            verification: VerificationState::default(),
            memory: WorkingMemory::default(),
        }
    }

    pub fn push_observation(&mut self, obs: Observation) {
        if self.observations.len() >= self.max_observations {
            self.observations.pop_front();
        }
        self.observations.push_back(obs);
    }

    pub fn push_action(&mut self, action: OrchestratorAction) {
        self.action_history.push(action);
        self.session.increment_step();
    }

    pub fn record_trace(
        &mut self,
        action: &OrchestratorAction,
        reason: String,
        duration_ms: u64,
        alternatives: Vec<ActionCandidate>,
    ) {
        let budget = &self.session.budget;
        let trace = DecisionTrace {
            id: Uuid::new_v4(),
            session_id: self.session.id,
            step: self.session.step_count,
            timestamp: Utc::now(),
            duration_ms,
            action: action.clone(),
            reason,
            complexity: self.task_complexity,
            knowledge_confidence: self.knowledge_confidence,
            budget_snapshot: BudgetSnapshot {
                tokens_used: budget.tokens_used,
                tokens_remaining: budget.remaining_tokens(),
                tool_calls_used: budget.tool_calls_used,
                tool_calls_remaining: budget.remaining_tool_calls(),
                retrievals_used: budget.retrievals_used,
                verify_cycles_used: budget.verify_cycles_used,
                elapsed_secs: (Utc::now() - self.started_at).num_seconds() as u64,
            },
            context_utilization: self
                .current_context
                .as_ref()
                .map(|c| c.utilization())
                .unwrap_or(0.0),
            alternatives_considered: alternatives,
        };
        self.traces.push(trace);
    }

    pub fn should_stop(&self) -> Option<oco_shared_types::StopReason> {
        if !self.session.is_within_budget() {
            if self.session.step_count >= self.session.max_steps {
                return Some(oco_shared_types::StopReason::MaxStepsReached);
            }
            return Some(oco_shared_types::StopReason::BudgetExhausted);
        }
        if self.error_streak >= 3 {
            return Some(oco_shared_types::StopReason::Error {
                message: "Too many consecutive errors".into(),
            });
        }
        None
    }

    pub fn elapsed_secs(&self) -> u64 {
        (Utc::now() - self.started_at).num_seconds() as u64
    }

    /// Classify the task category from the user request.
    pub fn task_category(&self) -> TaskCategory {
        oco_policy_engine::classifier::TaskClassifier::classify_category(&self.session.user_request)
    }
}
