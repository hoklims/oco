use std::sync::Mutex;

use oco_shared_types::{
    DecisionTrace, InterventionOutcome, InterventionSummary, PolicyPack, RunCheckSummary,
    RunSummary, SessionId, TaskComplexity, TelemetryEvent, TelemetryEventType, TrustVerdict,
    VerificationState,
};

/// Thread-safe collector for decision traces and telemetry events.
pub struct DecisionTraceCollector {
    traces: Mutex<Vec<DecisionTrace>>,
    /// v2: Fine-grained telemetry events.
    events: Mutex<Vec<TelemetryEvent>>,
}

impl DecisionTraceCollector {
    /// Create a new empty trace collector.
    pub fn new() -> Self {
        Self {
            traces: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
        }
    }

    /// Record a decision trace.
    pub fn record(&self, trace: DecisionTrace) {
        let mut traces = self.traces.lock().expect("trace mutex poisoned");
        traces.push(trace);
    }

    /// v2: Record a telemetry event. Returns the event index for outcome tracking.
    pub fn record_event(&self, event_type: TelemetryEventType) -> usize {
        let event = TelemetryEvent {
            timestamp: chrono::Utc::now(),
            event_type,
            outcome: None,
        };
        let mut events = self.events.lock().expect("events mutex poisoned");
        let idx = events.len();
        events.push(event);
        idx
    }

    /// v2: Mark a specific event by index with an outcome. Thread-safe.
    pub fn mark_outcome(&self, event_idx: usize, outcome: InterventionOutcome) {
        let mut events = self.events.lock().expect("events mutex poisoned");
        if let Some(event) = events.get_mut(event_idx) {
            event.outcome = Some(outcome);
        }
    }

    /// v2: Get all events.
    pub fn get_events(&self) -> Vec<TelemetryEvent> {
        let events = self.events.lock().expect("events mutex poisoned");
        events.clone()
    }

    /// v2: Compute an intervention summary from recorded events.
    pub fn intervention_summary(&self) -> InterventionSummary {
        let events = self.events.lock().expect("events mutex poisoned");
        let mut useful = 0u32;
        let mut redundant = 0u32;
        let mut harmful = 0u32;
        let mut unknown = 0u32;
        for event in events.iter() {
            match event.outcome {
                Some(InterventionOutcome::Useful) => useful += 1,
                Some(InterventionOutcome::Redundant) => redundant += 1,
                Some(InterventionOutcome::Harmful) => harmful += 1,
                Some(InterventionOutcome::Unknown) | None => unknown += 1,
            }
        }
        InterventionSummary {
            total_interventions: events.len() as u32,
            useful,
            redundant,
            harmful,
            unknown,
        }
    }

    /// v2: Export all events as JSONL (one event per line).
    pub fn export_events_jsonl(&self) -> String {
        let events = self.events.lock().expect("events mutex poisoned");
        events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get all traces for a given session.
    pub fn get_session_traces(&self, session_id: SessionId) -> Vec<DecisionTrace> {
        let traces = self.traces.lock().expect("trace mutex poisoned");
        traces
            .iter()
            .filter(|t| t.session_id == session_id)
            .cloned()
            .collect()
    }

    /// Export all traces for a session as a JSON string.
    pub fn export_json(&self, session_id: SessionId) -> String {
        let session_traces = self.get_session_traces(session_id);
        serde_json::to_string_pretty(&session_traces).unwrap_or_else(|_| "[]".to_string())
    }

    /// Return the total number of recorded traces.
    pub fn len(&self) -> usize {
        let traces = self.traces.lock().expect("trace mutex poisoned");
        traces.len()
    }

    /// Check if the collector is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all stored traces and events.
    pub fn clear(&self) {
        let mut traces = self.traces.lock().expect("trace mutex poisoned");
        traces.clear();
        drop(traces);
        let mut events = self.events.lock().expect("events mutex poisoned");
        events.clear();
    }
}

impl Default for DecisionTraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ── RunSummaryBuilder ────────────────────────────────────

/// Builds a [`RunSummary`] from orchestration artifacts (traces, verification state, etc.).
pub struct RunSummaryBuilder {
    session_id: SessionId,
    request: String,
    complexity: TaskComplexity,
    policy_pack: PolicyPack,
    verification_state: VerificationState,
    sensitive_paths: Vec<String>,
    risks: Vec<String>,
    key_decisions: Vec<String>,
    total_steps: u32,
    total_tokens: u64,
    total_duration_ms: u64,
    replans: u32,
}

impl RunSummaryBuilder {
    /// Create a new builder with required fields.
    pub fn new(session_id: SessionId, request: String) -> Self {
        Self {
            session_id,
            request,
            complexity: TaskComplexity::Medium,
            policy_pack: PolicyPack::default(),
            verification_state: VerificationState::default(),
            sensitive_paths: Vec::new(),
            risks: Vec::new(),
            key_decisions: Vec::new(),
            total_steps: 0,
            total_tokens: 0,
            total_duration_ms: 0,
            replans: 0,
        }
    }

    /// Set the task complexity.
    pub fn complexity(mut self, complexity: TaskComplexity) -> Self {
        self.complexity = complexity;
        self
    }

    /// Set the policy pack used for this run.
    pub fn policy_pack(mut self, pack: PolicyPack) -> Self {
        self.policy_pack = pack;
        self
    }

    /// Set the verification state to derive file coverage and freshness.
    pub fn verification_state(mut self, state: VerificationState) -> Self {
        self.verification_state = state;
        self
    }

    /// Set sensitive paths for accurate trust verdict computation.
    pub fn sensitive_paths(mut self, paths: Vec<String>) -> Self {
        self.sensitive_paths = paths;
        self
    }

    /// Add a risk note.
    pub fn risk(mut self, risk: impl Into<String>) -> Self {
        self.risks.push(risk.into());
        self
    }

    /// Add a key decision.
    pub fn decision(mut self, decision: impl Into<String>) -> Self {
        self.key_decisions.push(decision.into());
        self
    }

    /// Set run metrics.
    pub fn metrics(mut self, steps: u32, tokens: u64, duration_ms: u64, replans: u32) -> Self {
        self.total_steps = steps;
        self.total_tokens = tokens;
        self.total_duration_ms = duration_ms;
        self.replans = replans;
        self
    }

    /// Build the final [`RunSummary`], computing derived fields automatically.
    pub fn build(self) -> RunSummary {
        let files_modified: Vec<String> = self
            .verification_state
            .modified_files
            .keys()
            .cloned()
            .collect();

        // Determine which files are verified vs unverified
        let mut files_verified = Vec::new();
        let mut files_unverified = Vec::new();
        for f in &files_modified {
            let covered = self
                .verification_state
                .runs
                .iter()
                .any(|run| run.covered_files.is_empty() || run.covered_files.contains(f));
            if covered {
                files_verified.push(f.clone());
            } else {
                files_unverified.push(f.clone());
            }
        }

        // If no runs exist, all modified files are unverified
        if self.verification_state.runs.is_empty() {
            files_unverified = files_modified.clone();
            files_verified.clear();
        }

        let freshness = self.verification_state.freshness();

        let mandatory_strats = self.policy_pack.mandatory_strategies();
        let checks_run: Vec<RunCheckSummary> = self
            .verification_state
            .runs
            .iter()
            .map(|run| {
                let is_mandatory = mandatory_strats
                    .iter()
                    .any(|s| format!("{s:?}").to_lowercase() == run.strategy);
                RunCheckSummary {
                    strategy: run.strategy.clone(),
                    passed: run.passed,
                    duration_ms: run.duration_ms,
                    mandatory: is_mandatory,
                }
            })
            .collect();

        let all_mandatory_passed = checks_run.iter().filter(|c| c.mandatory).all(|c| c.passed);

        let has_unverified_sensitive = !self.sensitive_paths.is_empty()
            && files_unverified.iter().any(|f| {
                self.sensitive_paths.iter().any(|pat| {
                    if pat.contains('*') {
                        let suffix = pat.trim_start_matches('*');
                        f.ends_with(suffix)
                    } else {
                        f == pat || f.ends_with(pat)
                    }
                })
            });

        let trust_verdict =
            TrustVerdict::compute(freshness, all_mandatory_passed, has_unverified_sensitive);

        RunSummary {
            session_id: self.session_id,
            request: self.request,
            complexity: self.complexity,
            policy_pack: self.policy_pack,
            total_steps: self.total_steps,
            total_tokens: self.total_tokens,
            total_duration_ms: self.total_duration_ms,
            files_modified,
            files_verified,
            files_unverified,
            verification_freshness: freshness,
            checks_run,
            replans: self.replans,
            key_decisions: self.key_decisions,
            trust_verdict,
            risks: self.risks,
        }
    }

    /// Build and serialize to JSON. Returns `Err` on serialization failure.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let summary = Self {
            session_id: self.session_id,
            request: self.request.clone(),
            complexity: self.complexity,
            policy_pack: self.policy_pack,
            verification_state: self.verification_state.clone(),
            sensitive_paths: self.sensitive_paths.clone(),
            risks: self.risks.clone(),
            key_decisions: self.key_decisions.clone(),
            total_steps: self.total_steps,
            total_tokens: self.total_tokens,
            total_duration_ms: self.total_duration_ms,
            replans: self.replans,
        }
        .build();
        serde_json::to_string_pretty(&summary)
    }

    /// Build and format as human-readable text.
    pub fn to_text(&self) -> String {
        let summary = Self {
            session_id: self.session_id,
            request: self.request.clone(),
            complexity: self.complexity,
            policy_pack: self.policy_pack,
            verification_state: self.verification_state.clone(),
            sensitive_paths: self.sensitive_paths.clone(),
            risks: self.risks.clone(),
            key_decisions: self.key_decisions.clone(),
            total_steps: self.total_steps,
            total_tokens: self.total_tokens,
            total_duration_ms: self.total_duration_ms,
            replans: self.replans,
        }
        .build();
        summary_to_text(&summary)
    }
}

/// Format a [`RunSummary`] as human-readable text.
pub fn summary_to_text(summary: &RunSummary) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Run Summary ({})", summary.session_id.0));
    lines.push(format!("  Request:    {}", summary.request));
    lines.push(format!("  Complexity: {:?}", summary.complexity));
    lines.push(format!("  Policy:     {}", summary.policy_pack.label()));
    lines.push(format!(
        "  Modified:   {} file(s)",
        summary.files_modified.len()
    ));
    lines.push(format!(
        "  Verified:   {} file(s)",
        summary.files_verified.len()
    ));
    if !summary.files_unverified.is_empty() {
        lines.push(format!(
            "  Unverified: {} file(s) — {}",
            summary.files_unverified.len(),
            summary.files_unverified.join(", ")
        ));
    }
    lines.push(format!(
        "  Freshness:  {:?}",
        summary.verification_freshness
    ));
    lines.push(format!(
        "  Checks:     {} ({} passed / {} total)",
        if summary.checks_run.iter().all(|c| c.passed) {
            "all pass"
        } else {
            "failures"
        },
        summary.checks_run.iter().filter(|c| c.passed).count(),
        summary.checks_run.len(),
    ));
    lines.push(format!(
        "  Trust:      {} {}",
        summary.trust_verdict.symbol(),
        summary.trust_verdict.label()
    ));
    if !summary.risks.is_empty() {
        lines.push(format!("  Risks:      {}", summary.risks.join("; ")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_event_and_summarize() {
        let collector = DecisionTraceCollector::new();
        let idx = collector.record_event(TelemetryEventType::HookTriggered {
            hook_name: "pre_tool_use".into(),
            tool_name: Some("Bash".into()),
        });
        collector.mark_outcome(idx, InterventionOutcome::Useful);
        collector.record_event(TelemetryEventType::VerifyCompleted {
            strategy: "build".into(),
            passed: true,
            duration_ms: 500,
        });

        let summary = collector.intervention_summary();
        assert_eq!(summary.total_interventions, 2);
        assert_eq!(summary.useful, 1);
        assert_eq!(summary.unknown, 1);
    }

    #[test]
    fn export_events_jsonl_format() {
        let collector = DecisionTraceCollector::new();
        collector.record_event(TelemetryEventType::SkillInvoked {
            skill_name: "oco-verify-fix".into(),
        });
        collector.record_event(TelemetryEventType::MemoryUpdated {
            operation: "add_finding".into(),
            active_count: 3,
        });

        let jsonl = collector.export_events_jsonl();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("skill_invoked"));
        assert!(lines[1].contains("memory_updated"));
    }

    // ── RunSummaryBuilder tests ────────────────��─────────

    use oco_shared_types::{
        PolicyPack, TaskComplexity, TrustVerdict, VerificationFreshness, VerificationRun,
        VerificationState,
    };
    use std::collections::{HashMap, HashSet};

    fn make_verification_state_with_run(files: &[&str], passed: bool) -> VerificationState {
        let now = chrono::Utc::now();
        let mut modified = HashMap::new();
        for f in files {
            modified.insert(f.to_string(), now);
        }
        let mut state = VerificationState {
            modified_files: modified.clone(),
            runs: vec![],
        };
        state.runs.push(VerificationRun {
            strategy: "build".into(),
            timestamp: now + chrono::Duration::seconds(1),
            passed,
            covered_files: HashSet::new(),
            modifications_snapshot: modified,
            duration_ms: 150,
            failures: if passed {
                vec![]
            } else {
                vec!["compile error".into()]
            },
        });
        state
    }

    #[test]
    fn builder_high_trust_all_pass() {
        let state = make_verification_state_with_run(&["src/lib.rs"], true);
        let summary = RunSummaryBuilder::new(SessionId::new(), "fix typo".into())
            .complexity(TaskComplexity::Trivial)
            .policy_pack(PolicyPack::Balanced)
            .verification_state(state)
            .build();

        assert_eq!(summary.trust_verdict, TrustVerdict::High);
        assert_eq!(summary.files_modified.len(), 1);
        assert_eq!(summary.files_verified.len(), 1);
        assert!(summary.files_unverified.is_empty());
        assert_eq!(summary.verification_freshness, VerificationFreshness::Fresh);
    }

    #[test]
    fn builder_low_trust_on_failure() {
        let state = make_verification_state_with_run(&["src/main.rs"], false);
        let summary = RunSummaryBuilder::new(SessionId::new(), "break things".into())
            .complexity(TaskComplexity::Low)
            .policy_pack(PolicyPack::Strict)
            .verification_state(state)
            .build();

        assert_eq!(summary.trust_verdict, TrustVerdict::Low);
        assert!(!summary.checks_run.is_empty());
        assert!(!summary.checks_run[0].passed);
    }

    #[test]
    fn builder_none_trust_no_verification() {
        let mut state = VerificationState::default();
        state
            .modified_files
            .insert("src/foo.rs".into(), chrono::Utc::now());

        let summary = RunSummaryBuilder::new(SessionId::new(), "add feature".into())
            .verification_state(state)
            .build();

        assert_eq!(summary.trust_verdict, TrustVerdict::None);
        assert_eq!(summary.files_unverified.len(), 1);
        assert!(summary.checks_run.is_empty());
    }

    #[test]
    fn builder_risks_propagated() {
        let summary = RunSummaryBuilder::new(SessionId::new(), "deploy".into())
            .risk("touches auth module")
            .risk("no tests for edge case")
            .build();

        assert_eq!(summary.risks.len(), 2);
        assert!(summary.risks[0].contains("auth"));
    }

    #[test]
    fn builder_to_json_succeeds() {
        let builder = RunSummaryBuilder::new(SessionId::new(), "test json".into())
            .complexity(TaskComplexity::Medium)
            .policy_pack(PolicyPack::Fast);

        let json = builder.to_json();
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"policy_pack\": \"fast\""));
        assert!(json_str.contains("\"trust_verdict\": \"none\""));
    }

    #[test]
    fn builder_to_text_contains_key_fields() {
        let state = make_verification_state_with_run(&["src/lib.rs"], true);
        let builder = RunSummaryBuilder::new(SessionId::new(), "refactor module".into())
            .complexity(TaskComplexity::High)
            .policy_pack(PolicyPack::Strict)
            .verification_state(state);

        let text = builder.to_text();
        assert!(text.contains("Run Summary"));
        assert!(text.contains("refactor module"));
        assert!(text.contains("strict"));
        assert!(text.contains("Verified:"));
    }

    #[test]
    fn builder_default_policy_is_balanced() {
        let summary = RunSummaryBuilder::new(SessionId::new(), "test".into()).build();
        assert_eq!(summary.policy_pack, PolicyPack::Balanced);
    }

    #[test]
    fn builder_sensitive_unverified_downgrades_to_medium() {
        // Fresh verification, all mandatory passed, but unverified sensitive file
        let now = chrono::Utc::now();
        let mut state = VerificationState::default();
        // Two files modified: one verified, one not covered
        state.modified_files.insert("src/lib.rs".into(), now);
        state.modified_files.insert(".env".into(), now);
        state.runs.push(VerificationRun {
            strategy: "build".into(),
            timestamp: now + chrono::Duration::seconds(1),
            passed: true,
            covered_files: {
                let mut s = std::collections::HashSet::new();
                s.insert("src/lib.rs".into());
                s
            },
            modifications_snapshot: state.modified_files.clone(),
            duration_ms: 100,
            failures: vec![],
        });

        let summary = RunSummaryBuilder::new(SessionId::new(), "deploy".into())
            .verification_state(state)
            .sensitive_paths(vec![".env".into(), "*.pem".into()])
            .build();

        // .env is unverified and sensitive → downgrades from High to Medium
        assert_eq!(summary.trust_verdict, TrustVerdict::Medium);
    }

    #[test]
    fn builder_no_sensitive_paths_means_no_downgrade() {
        let state = make_verification_state_with_run(&["src/lib.rs"], true);
        let summary = RunSummaryBuilder::new(SessionId::new(), "fix".into())
            .verification_state(state)
            // no sensitive_paths set → has_unverified_sensitive = false
            .build();

        assert_eq!(summary.trust_verdict, TrustVerdict::High);
    }
}
