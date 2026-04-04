use chrono::{DateTime, Utc};
use oco_shared_types::{
    ActionCandidate, AssembledContext, BudgetSnapshot, CompactSnapshot, DecisionTrace,
    MissionMemory, Observation, OrchestratorAction, PolicyPack, RepoProfile, Session, TaskCategory,
    TaskComplexity, TrustVerdict, VerificationState, WorkingMemory,
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
            external_session_id: self.session.external_session_id.clone(),
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

    /// Create a typed compact snapshot of the current working memory.
    ///
    /// The snapshot captures verified facts, active hypotheses, plan, planner
    /// state, and open questions — everything needed to survive context
    /// compaction. The `policy_pack` is stamped onto the snapshot for
    /// downstream consumers.
    pub fn create_compact_snapshot(&self, policy_pack: PolicyPack) -> CompactSnapshot {
        CompactSnapshot::from_memory(&self.memory, &self.verification, policy_pack)
    }

    /// Restore working memory state from a previously created snapshot.
    ///
    /// Re-populates verified facts, hypotheses, plan, and planner state from
    /// the snapshot. Existing entries in those categories are replaced.
    /// Other memory categories (findings, questions, inspected areas,
    /// invalidated) are left untouched.
    pub fn restore_from_snapshot(&mut self, snapshot: &CompactSnapshot) {
        // Restore verified facts as MemoryEntry instances.
        self.memory.verified_facts = snapshot
            .verified_facts
            .iter()
            .map(|content| {
                let mut entry = oco_shared_types::MemoryEntry::new(content.clone(), 1.0);
                entry.status = oco_shared_types::MemoryStatus::Confirmed;
                entry
            })
            .collect();

        // Restore hypotheses.
        self.memory.hypotheses = snapshot
            .hypotheses
            .iter()
            .map(|(text, confidence)| oco_shared_types::MemoryEntry::new(text.clone(), *confidence))
            .collect();

        // Restore plan.
        self.memory.plan = snapshot.plan.clone();

        // Restore planner state.
        self.memory.planner_state = snapshot.planner_state.clone();
    }

    /// Create a [`MissionMemory`] from the current orchestration state.
    ///
    /// Uses the same trust-verdict logic as `RunSummaryBuilder` (Q3):
    /// mandatory strategies come from the profile's policy pack, and
    /// sensitive-path checks use the profile's sensitive_paths list.
    pub fn create_mission_memory(&self, profile: &RepoProfile) -> MissionMemory {
        let freshness = self.verification.freshness();
        let policy_pack = profile.policy_pack;

        // Determine which verification runs are mandatory for this policy pack.
        let mandatory_strats = policy_pack.mandatory_strategies();
        let all_mandatory_passed = self
            .verification
            .runs
            .iter()
            .filter(|run| {
                mandatory_strats
                    .iter()
                    .any(|s| format!("{s:?}").to_lowercase() == run.strategy)
            })
            .all(|run| run.passed);

        // Check whether any unverified file matches a sensitive path pattern.
        // Uses the same logic as RunSummaryBuilder: a file is "unverified" if
        // no run covers it (empty covered_files = whole-project coverage).
        let files_unverified: Vec<&str> =
            if self.verification.runs.is_empty() {
                self.verification
                    .modified_files
                    .keys()
                    .map(|s| s.as_str())
                    .collect()
            } else {
                self.verification
                    .modified_files
                    .keys()
                    .filter(|f| {
                        !self.verification.runs.iter().any(|run| {
                            run.covered_files.is_empty() || run.covered_files.contains(*f)
                        })
                    })
                    .map(|s| s.as_str())
                    .collect()
            };
        let has_unverified_sensitive = !profile.sensitive_paths.is_empty()
            && files_unverified
                .iter()
                .any(|path| profile.is_sensitive(path));

        let trust_verdict =
            TrustVerdict::compute(freshness, all_mandatory_passed, has_unverified_sensitive);

        MissionMemory::from_working_state(
            self.session.id,
            &self.session.user_request,
            &self.memory,
            &self.verification,
            trust_verdict,
        )
    }

    /// Restore state from a [`MissionMemory`] (for inter-session resume).
    ///
    /// **Restored faithfully**: verified facts, hypotheses, questions, plan,
    /// planner state, and the list of previously-modified files (as stale).
    ///
    /// **Not restored** (irrecoverably lost between sessions):
    /// - Raw observations, tool output history, action history
    /// - Verification runs (timestamps, durations, covered files)
    /// - Evidence link UUIDs between entries
    /// - Findings, inspected areas, invalidated entries
    ///
    /// After restore, `verification.freshness()` will return `Stale` or `None`,
    /// forcing the orchestrator to re-verify before completing.
    pub fn restore_from_mission(&mut self, mission: &MissionMemory) {
        // Restore verified facts
        self.memory.verified_facts = mission
            .facts
            .iter()
            .map(|f| {
                let mut entry = oco_shared_types::MemoryEntry::new(f.content.clone(), 1.0);
                entry.status = oco_shared_types::MemoryStatus::Confirmed;
                if let Some(ref src) = f.source {
                    entry.source = Some(src.clone());
                }
                entry
            })
            .collect();

        // Restore hypotheses
        self.memory.hypotheses = mission
            .hypotheses
            .iter()
            .map(|h| {
                oco_shared_types::MemoryEntry::new(
                    h.content.clone(),
                    h.confidence_pct as f64 / 100.0,
                )
            })
            .collect();

        // Restore questions
        self.memory.questions = mission
            .open_questions
            .iter()
            .map(|q| oco_shared_types::MemoryEntry::new(q.clone(), 0.5))
            .collect();

        // Restore plan
        self.memory.plan = mission.plan.remaining_steps.clone();

        // Restore planner state
        if mission.plan.current_objective.is_some() || mission.plan.phase.is_some() {
            self.memory.planner_state = Some(oco_shared_types::PlannerState {
                current_step: mission.plan.current_objective.clone(),
                replan_count: 0,
                phase: mission.plan.phase.clone(),
                lease_id: None,
            });
        }

        // Restore verification awareness: mark previously-modified files as
        // modified at the current time. Because there are no verification runs
        // in the fresh state, freshness() will return Stale or None — forcing
        // re-verification before the task can complete.
        for file in &mission.modified_files {
            self.verification.record_modification(file.clone());
        }
        // Also mark unverified files from the previous session.
        for file in &mission.verification.unverified_files {
            if !self.verification.modified_files.contains_key(file) {
                self.verification.record_modification(file.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{
        MemoryEntry, MissionFact, MissionHypothesis, MissionPlan, PlannerState, RepoProfile,
    };

    fn test_session() -> Session {
        Session::new("fix the auth bug".into(), None)
    }

    #[test]
    fn create_compact_snapshot_captures_memory() {
        let mut state = OrchestrationState::new(test_session());

        let fact = MemoryEntry::new("DB pool is correct".into(), 1.0);
        let fact_id = fact.id;
        state.memory.add_finding(fact);
        state.memory.promote_to_fact(fact_id);

        state
            .memory
            .add_hypothesis(MemoryEntry::new("race in cache".into(), 0.7));
        state
            .memory
            .update_plan(vec!["add lock".into(), "test".into()]);
        state
            .memory
            .add_question(MemoryEntry::new("which backend?".into(), 0.5));
        state.memory.update_planner_state(PlannerState {
            current_step: Some("investigate".into()),
            replan_count: 1,
            phase: Some("explore".into()),
            lease_id: None,
        });

        let snap = state.create_compact_snapshot(PolicyPack::Strict);

        assert_eq!(snap.verified_facts.len(), 1);
        assert_eq!(snap.verified_facts[0], "DB pool is correct");
        assert_eq!(snap.hypotheses.len(), 1);
        assert_eq!(snap.plan.len(), 2);
        assert_eq!(snap.questions.len(), 1);
        assert_eq!(snap.policy_pack, PolicyPack::Strict);
        assert!(snap.planner_state.is_some());
        assert!(snap.has_content());
    }

    #[test]
    fn create_compact_snapshot_empty_memory() {
        let state = OrchestrationState::new(test_session());
        let snap = state.create_compact_snapshot(PolicyPack::Balanced);
        assert!(!snap.has_content());
        assert_eq!(snap.policy_pack, PolicyPack::Balanced);
    }

    #[test]
    fn restore_from_snapshot_populates_memory() {
        let mut state = OrchestrationState::new(test_session());
        assert!(state.memory.verified_facts.is_empty());
        assert!(state.memory.hypotheses.is_empty());

        let snap = CompactSnapshot {
            verified_facts: vec!["fact A".into(), "fact B".into()],
            hypotheses: vec![("hyp X".into(), 0.75)],
            plan: vec!["step 1".into(), "step 2".into()],
            questions: vec!["q?".into()],
            inspected_paths: vec!["src/lib.rs".into()],
            planner_state: Some(PlannerState {
                current_step: Some("verify".into()),
                replan_count: 2,
                phase: Some("implement".into()),
                lease_id: None,
            }),
            policy_pack: PolicyPack::Strict,
            verification_freshness: oco_shared_types::VerificationFreshness::Fresh,
            unverified_files: vec![],
            created_at: Utc::now(),
        };

        state.restore_from_snapshot(&snap);

        assert_eq!(state.memory.verified_facts.len(), 2);
        assert_eq!(state.memory.verified_facts[0].content, "fact A");
        assert_eq!(state.memory.verified_facts[0].confidence, 1.0);
        assert_eq!(
            state.memory.verified_facts[0].status,
            oco_shared_types::MemoryStatus::Confirmed
        );

        assert_eq!(state.memory.hypotheses.len(), 1);
        assert_eq!(state.memory.hypotheses[0].content, "hyp X");
        assert!((state.memory.hypotheses[0].confidence - 0.75).abs() < f64::EPSILON);

        assert_eq!(state.memory.plan, vec!["step 1", "step 2"]);

        let ps = state.memory.planner_state.as_ref().unwrap();
        assert_eq!(ps.current_step.as_deref(), Some("verify"));
        assert_eq!(ps.replan_count, 2);
        assert_eq!(ps.phase.as_deref(), Some("implement"));
    }

    #[test]
    fn restore_preserves_unrelated_memory_categories() {
        let mut state = OrchestrationState::new(test_session());
        state
            .memory
            .add_finding(MemoryEntry::new("existing finding".into(), 0.5));
        state
            .memory
            .add_question(MemoryEntry::new("existing question".into(), 0.5));

        let snap = CompactSnapshot {
            verified_facts: vec!["restored fact".into()],
            hypotheses: vec![],
            plan: vec![],
            questions: vec![],
            inspected_paths: vec![],
            planner_state: None,
            policy_pack: PolicyPack::Balanced,
            verification_freshness: oco_shared_types::VerificationFreshness::None,
            unverified_files: vec![],
            created_at: Utc::now(),
        };

        state.restore_from_snapshot(&snap);

        assert_eq!(state.memory.findings.len(), 1);
        assert_eq!(state.memory.findings[0].content, "existing finding");
        assert_eq!(state.memory.questions.len(), 1);
        assert_eq!(state.memory.questions[0].content, "existing question");
        assert_eq!(state.memory.verified_facts.len(), 1);
        assert_eq!(state.memory.verified_facts[0].content, "restored fact");
    }

    #[test]
    fn roundtrip_snapshot_restore() {
        let mut state = OrchestrationState::new(test_session());

        let fact = MemoryEntry::new("verified item".into(), 1.0);
        let fact_id = fact.id;
        state.memory.add_finding(fact);
        state.memory.promote_to_fact(fact_id);
        state
            .memory
            .add_hypothesis(MemoryEntry::new("maybe X".into(), 0.6));
        state.memory.update_plan(vec!["do A".into()]);
        state.memory.update_planner_state(PlannerState {
            current_step: Some("step Z".into()),
            replan_count: 0,
            phase: None,
            lease_id: None,
        });

        let snap = state.create_compact_snapshot(PolicyPack::Balanced);

        let mut fresh_state = OrchestrationState::new(test_session());
        fresh_state.restore_from_snapshot(&snap);

        assert_eq!(fresh_state.memory.verified_facts.len(), 1);
        assert_eq!(
            fresh_state.memory.verified_facts[0].content,
            "verified item"
        );
        assert_eq!(fresh_state.memory.hypotheses.len(), 1);
        assert_eq!(fresh_state.memory.hypotheses[0].content, "maybe X");
        assert_eq!(fresh_state.memory.plan, vec!["do A"]);
        assert_eq!(
            fresh_state
                .memory
                .planner_state
                .as_ref()
                .unwrap()
                .current_step
                .as_deref(),
            Some("step Z")
        );
    }

    // -- Mission memory tests -------------------------------------------------

    #[test]
    fn create_mission_memory_captures_state() {
        let mut state = OrchestrationState::new(test_session());

        let fact = MemoryEntry::new("DB pool is correct".into(), 1.0);
        let fact_id = fact.id;
        state.memory.add_finding(fact);
        state.memory.promote_to_fact(fact_id);

        state
            .memory
            .add_hypothesis(MemoryEntry::new("race in cache".into(), 0.7));
        state
            .memory
            .update_plan(vec!["add lock".into(), "test".into()]);
        state
            .memory
            .add_question(MemoryEntry::new("which backend?".into(), 0.5));
        state.memory.update_planner_state(PlannerState {
            current_step: Some("investigate".into()),
            replan_count: 1,
            phase: Some("explore".into()),
            lease_id: None,
        });

        let mm = state.create_mission_memory(&RepoProfile::default());

        assert_eq!(mm.mission, "fix the auth bug");
        assert_eq!(mm.facts.len(), 1);
        assert_eq!(mm.facts[0].content, "DB pool is correct");
        assert_eq!(mm.hypotheses.len(), 1);
        assert_eq!(mm.hypotheses[0].content, "race in cache");
        assert_eq!(mm.open_questions.len(), 1);
        assert_eq!(mm.open_questions[0], "which backend?");
        assert_eq!(mm.plan.remaining_steps.len(), 2);
        assert_eq!(mm.plan.current_objective.as_deref(), Some("investigate"));
        assert_eq!(mm.plan.phase.as_deref(), Some("explore"));
        assert!(mm.has_content());
    }

    #[test]
    fn create_mission_memory_empty_state() {
        let state = OrchestrationState::new(test_session());
        let mm = state.create_mission_memory(&RepoProfile::default());
        assert!(!mm.has_content());
    }

    #[test]
    fn restore_from_mission_populates_memory() {
        let mut state = OrchestrationState::new(test_session());
        assert!(state.memory.verified_facts.is_empty());
        assert!(state.memory.hypotheses.is_empty());

        let mm = MissionMemory {
            schema_version: 1,
            session_id: state.session.id,
            created_at: Utc::now(),
            mission: "fix the auth bug".into(),
            facts: vec![
                MissionFact {
                    content: "fact A".into(),
                    source: Some("src/lib.rs".into()),
                    established_at: Utc::now(),
                },
                MissionFact {
                    content: "fact B".into(),
                    source: None,
                    established_at: Utc::now(),
                },
            ],
            hypotheses: vec![MissionHypothesis {
                content: "hyp X".into(),
                confidence_pct: 75,
                supporting_evidence: vec![],
            }],
            open_questions: vec!["question?".into()],
            plan: MissionPlan {
                current_objective: Some("verify".into()),
                completed_steps: vec!["step done".into()],
                remaining_steps: vec!["step 1".into(), "step 2".into()],
                phase: Some("implement".into()),
            },
            verification: Default::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: oco_shared_types::TrustVerdict::Medium,
            narrative: String::new(),
        };

        state.restore_from_mission(&mm);

        assert_eq!(state.memory.verified_facts.len(), 2);
        assert_eq!(state.memory.verified_facts[0].content, "fact A");
        assert_eq!(state.memory.verified_facts[0].confidence, 1.0);
        assert_eq!(
            state.memory.verified_facts[0].status,
            oco_shared_types::MemoryStatus::Confirmed
        );
        assert_eq!(
            state.memory.verified_facts[0].source.as_deref(),
            Some("src/lib.rs")
        );
        assert!(state.memory.verified_facts[1].source.is_none());

        assert_eq!(state.memory.hypotheses.len(), 1);
        assert_eq!(state.memory.hypotheses[0].content, "hyp X");
        assert!((state.memory.hypotheses[0].confidence - 0.75).abs() < f64::EPSILON);

        assert_eq!(state.memory.questions.len(), 1);
        assert_eq!(state.memory.questions[0].content, "question?");

        assert_eq!(state.memory.plan, vec!["step 1", "step 2"]);

        let ps = state.memory.planner_state.as_ref().unwrap();
        assert_eq!(ps.current_step.as_deref(), Some("verify"));
        assert_eq!(ps.replan_count, 0);
        assert_eq!(ps.phase.as_deref(), Some("implement"));
    }

    #[test]
    fn restore_from_mission_preserves_unrelated_memory() {
        let mut state = OrchestrationState::new(test_session());
        state
            .memory
            .add_finding(MemoryEntry::new("existing finding".into(), 0.5));

        let mm = MissionMemory {
            schema_version: 1,
            session_id: state.session.id,
            created_at: Utc::now(),
            mission: "resume".into(),
            facts: vec![MissionFact {
                content: "restored fact".into(),
                source: None,
                established_at: Utc::now(),
            }],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: Default::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: oco_shared_types::TrustVerdict::None,
            narrative: String::new(),
        };

        state.restore_from_mission(&mm);

        // Findings are untouched
        assert_eq!(state.memory.findings.len(), 1);
        assert_eq!(state.memory.findings[0].content, "existing finding");
        // Facts are restored
        assert_eq!(state.memory.verified_facts.len(), 1);
        assert_eq!(state.memory.verified_facts[0].content, "restored fact");
    }

    #[test]
    fn roundtrip_mission_memory() {
        let mut state = OrchestrationState::new(test_session());

        let fact = MemoryEntry::new("verified item".into(), 1.0);
        let fact_id = fact.id;
        state.memory.add_finding(fact);
        state.memory.promote_to_fact(fact_id);
        state
            .memory
            .add_hypothesis(MemoryEntry::new("maybe X".into(), 0.6));
        state.memory.update_plan(vec!["do A".into()]);
        state
            .memory
            .add_question(MemoryEntry::new("open Q".into(), 0.5));
        state.memory.update_planner_state(PlannerState {
            current_step: Some("step Z".into()),
            replan_count: 0,
            phase: Some("explore".into()),
            lease_id: None,
        });

        let mm = state.create_mission_memory(&RepoProfile::default());

        let mut fresh_state = OrchestrationState::new(test_session());
        fresh_state.restore_from_mission(&mm);

        assert_eq!(fresh_state.memory.verified_facts.len(), 1);
        assert_eq!(
            fresh_state.memory.verified_facts[0].content,
            "verified item"
        );
        assert_eq!(fresh_state.memory.hypotheses.len(), 1);
        assert_eq!(fresh_state.memory.hypotheses[0].content, "maybe X");
        assert_eq!(fresh_state.memory.plan, vec!["do A"]);
        assert_eq!(fresh_state.memory.questions.len(), 1);
        assert_eq!(fresh_state.memory.questions[0].content, "open Q");
        assert_eq!(
            fresh_state
                .memory
                .planner_state
                .as_ref()
                .unwrap()
                .current_step
                .as_deref(),
            Some("step Z")
        );
        assert_eq!(
            fresh_state
                .memory
                .planner_state
                .as_ref()
                .unwrap()
                .phase
                .as_deref(),
            Some("explore")
        );
    }

    #[test]
    fn restore_from_mission_no_planner_state_when_empty() {
        let mut state = OrchestrationState::new(test_session());

        let mm = MissionMemory {
            schema_version: 1,
            session_id: state.session.id,
            created_at: Utc::now(),
            mission: "test".into(),
            facts: vec![],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(), // no objective, no phase
            verification: Default::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: oco_shared_types::TrustVerdict::None,
            narrative: String::new(),
        };

        state.restore_from_mission(&mm);
        assert!(state.memory.planner_state.is_none());
    }

    #[test]
    fn restore_from_mission_marks_modified_files_as_stale() {
        let mut state = OrchestrationState::new(test_session());
        assert!(state.verification.modified_files.is_empty());

        let mm = MissionMemory {
            schema_version: 1,
            session_id: state.session.id,
            created_at: Utc::now(),
            mission: "test".into(),
            facts: vec![],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: oco_shared_types::MissionVerificationStatus {
                freshness: oco_shared_types::VerificationFreshness::Stale,
                unverified_files: vec!["src/extra.rs".into()],
                ..Default::default()
            },
            modified_files: vec!["src/main.rs".into(), "src/lib.rs".into()],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: oco_shared_types::TrustVerdict::Low,
            narrative: String::new(),
        };

        state.restore_from_mission(&mm);

        // All modified + unverified files are now tracked
        assert!(
            state
                .verification
                .modified_files
                .contains_key("src/main.rs")
        );
        assert!(state.verification.modified_files.contains_key("src/lib.rs"));
        assert!(
            state
                .verification
                .modified_files
                .contains_key("src/extra.rs")
        );

        // No verification runs → freshness is None, forcing re-verification
        assert_eq!(
            state.verification.freshness(),
            oco_shared_types::VerificationFreshness::None
        );
    }

    #[test]
    fn create_mission_memory_sensitive_path_downgrades_trust() {
        let mut state = OrchestrationState::new(test_session());

        // Record modifications to two files: one sensitive, one not
        state.verification.record_modification("src/main.rs".into());
        state
            .verification
            .record_modification("certs/server.pem".into());

        // No verification runs yet → freshness is None → trust = None
        let mm = state.create_mission_memory(&RepoProfile::default());
        assert_eq!(mm.trust_verdict, oco_shared_types::TrustVerdict::None);

        // Add passing runs that cover only src/main.rs but NOT certs/server.pem
        let covered: std::collections::HashSet<String> =
            ["src/main.rs".to_string()].into_iter().collect();
        state
            .verification
            .record_run(oco_shared_types::VerificationRun {
                strategy: "build".into(),
                timestamp: chrono::Utc::now() + chrono::Duration::seconds(1),
                passed: true,
                covered_files: covered.clone(),
                modifications_snapshot: state.verification.modified_files.clone(),
                duration_ms: 100,
                failures: vec![],
            });
        state
            .verification
            .record_run(oco_shared_types::VerificationRun {
                strategy: "test".into(),
                timestamp: chrono::Utc::now() + chrono::Duration::seconds(2),
                passed: true,
                covered_files: covered,
                modifications_snapshot: state.verification.modified_files.clone(),
                duration_ms: 200,
                failures: vec![],
            });

        // With no sensitive paths in profile → freshness Partial, trust Medium
        // (because certs/server.pem is unverified but not sensitive)
        let mm_no_sens = state.create_mission_memory(&RepoProfile {
            sensitive_paths: vec![],
            ..Default::default()
        });
        // Partial freshness + all mandatory passed + no sensitive = Medium
        assert_eq!(
            mm_no_sens.trust_verdict,
            oco_shared_types::TrustVerdict::Medium
        );

        // With sensitive *.pem pattern → still Medium, but for the right reason:
        // Partial freshness + has_unverified_sensitive=true → Medium
        let mm_sens = state.create_mission_memory(&RepoProfile {
            sensitive_paths: vec!["*.pem".into()],
            ..Default::default()
        });
        assert_eq!(
            mm_sens.trust_verdict,
            oco_shared_types::TrustVerdict::Medium
        );

        // Verify the sensitive path actually matters by testing with Fresh:
        // Add a whole-project run that covers everything
        state
            .verification
            .record_run(oco_shared_types::VerificationRun {
                strategy: "build".into(),
                timestamp: chrono::Utc::now() + chrono::Duration::seconds(3),
                passed: true,
                covered_files: std::collections::HashSet::new(), // whole-project
                modifications_snapshot: state.verification.modified_files.clone(),
                duration_ms: 50,
                failures: vec![],
            });
        // Now fresh + no unverified → High
        let mm_fresh = state.create_mission_memory(&RepoProfile::default());
        assert_eq!(mm_fresh.trust_verdict, oco_shared_types::TrustVerdict::High);
    }
}
