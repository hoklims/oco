//! Scorecard builder — constructs a [`RunScorecard`] from heterogeneous data sources.
//!
//! Not every run produces every data source.  The builder accepts whatever is
//! available and fills in sensible defaults for missing dimensions.

use chrono::Utc;
use oco_shared_types::{
    CostMetrics, DimensionScore, MissionMemory, RunScorecard, RunSummary, ScenarioResult,
    ScorecardDimension, ScorecardWeights, TrustVerdict,
};

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Incrementally constructs a [`RunScorecard`] from available data sources.
///
/// Call any combination of `with_*` methods, then [`build`](Self::build) to
/// produce the final scorecard.  Missing data is handled gracefully: dimensions
/// without a data source receive a documented default score.
pub struct ScorecardBuilder {
    run_id: String,

    // -- Extracted signals (Option = not yet provided) --
    success: Option<bool>,
    trust_verdict: Option<TrustVerdict>,
    files_modified: Option<usize>,
    files_verified: Option<usize>,
    verification_passed: Option<Option<bool>>,
    has_mission_content: Option<bool>,
    total_tokens: Option<u64>,
    total_steps: Option<u32>,
    duration_ms: Option<u64>,
    replans: Option<u32>,
    error_count: Option<usize>,
    step_count_for_errors: Option<u32>,
    tool_calls: Option<u32>,
    verify_cycles: Option<u32>,

    // -- Per-repo weight overrides --
    weights: Option<ScorecardWeights>,
}

impl ScorecardBuilder {
    /// Start building a scorecard for the given run/scenario identifier.
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            success: None,
            trust_verdict: None,
            files_modified: None,
            files_verified: None,
            verification_passed: None,
            has_mission_content: None,
            total_tokens: None,
            total_steps: None,
            duration_ms: None,
            replans: None,
            error_count: None,
            step_count_for_errors: None,
            tool_calls: None,
            verify_cycles: None,
            weights: None,
        }
    }

    /// Feed a [`ScenarioResult`] (from eval runs).
    pub fn with_scenario_result(mut self, result: &ScenarioResult) -> Self {
        self.success = Some(result.success);
        self.total_tokens = Some(result.total_tokens);
        self.total_steps = Some(result.step_count);
        self.duration_ms = Some(result.duration_ms);
        self.verification_passed = Some(result.verification_passed);
        self.error_count = Some(result.errors.len());
        self.step_count_for_errors = Some(result.step_count);
        self
    }

    /// Feed a [`RunSummary`] (from orchestration runs).
    pub fn with_run_summary(mut self, summary: &RunSummary) -> Self {
        // Success: infer from trust_verdict != None
        self.success = Some(summary.trust_verdict != TrustVerdict::None);
        self.trust_verdict = Some(summary.trust_verdict);
        self.files_modified = Some(summary.files_modified.len());
        self.files_verified = Some(summary.files_verified.len());
        self.total_tokens = Some(summary.total_tokens);
        self.total_steps = Some(summary.total_steps);
        self.duration_ms = Some(summary.total_duration_ms);
        self.replans = Some(summary.replans);
        self
    }

    /// Feed a [`MissionMemory`] (if available).
    pub fn with_mission_memory(mut self, mission: &MissionMemory) -> Self {
        self.has_mission_content = Some(mission.has_content());
        // MissionMemory also carries a trust_verdict; use it if we don't
        // already have one from RunSummary.
        if self.trust_verdict.is_none() {
            self.trust_verdict = Some(mission.trust_verdict);
        }
        self
    }

    // -- Granular setters for callers that don't have full typed sources --

    /// Set success explicitly.
    pub fn success(mut self, success: bool) -> Self {
        self.success = Some(success);
        self
    }

    /// Set trust verdict explicitly.
    pub fn trust_verdict(mut self, verdict: TrustVerdict) -> Self {
        self.trust_verdict = Some(verdict);
        self
    }

    /// Set file counts for verification coverage computation.
    pub fn file_counts(mut self, modified: usize, verified: usize) -> Self {
        self.files_modified = Some(modified);
        self.files_verified = Some(verified);
        self
    }

    /// Set mission continuity explicitly.
    pub fn mission_continuity(mut self, has_content: bool) -> Self {
        self.has_mission_content = Some(has_content);
        self
    }

    /// Set cost metrics explicitly.
    pub fn cost(
        mut self,
        tokens: u64,
        steps: u32,
        duration_ms: u64,
        tool_calls: u32,
        verify_cycles: u32,
    ) -> Self {
        self.total_tokens = Some(tokens);
        self.total_steps = Some(steps);
        self.duration_ms = Some(duration_ms);
        self.tool_calls = Some(tool_calls);
        self.verify_cycles = Some(verify_cycles);
        self
    }

    /// Set replan count explicitly.
    pub fn replans(mut self, count: u32) -> Self {
        self.replans = Some(count);
        self
    }

    /// Set error data explicitly.
    pub fn errors(mut self, error_count: usize, step_count: u32) -> Self {
        self.error_count = Some(error_count);
        self.step_count_for_errors = Some(step_count);
        self
    }

    /// Set per-repo scorecard weight overrides.
    ///
    /// When provided, `build()` uses these weights instead of the hardcoded
    /// defaults for the overall score computation.
    pub fn with_weights(mut self, weights: ScorecardWeights) -> Self {
        self.weights = Some(weights);
        self
    }

    /// Build the final [`RunScorecard`].
    pub fn build(self) -> RunScorecard {
        let dimensions = vec![
            self.score_success(),
            self.score_trust_verdict(),
            self.score_verification_coverage(),
            self.score_mission_continuity(),
            self.score_cost_efficiency(),
            self.score_replan_stability(),
            self.score_error_rate(),
        ];

        let overall_score =
            RunScorecard::compute_overall_with_weights(&dimensions, self.weights.as_ref());

        let cost = CostMetrics {
            steps: self.total_steps.unwrap_or(0),
            tokens: self.total_tokens.unwrap_or(0),
            duration_ms: self.duration_ms.unwrap_or(0),
            tool_calls: self.tool_calls.unwrap_or(0),
            verify_cycles: self.verify_cycles.unwrap_or(0),
            replans: self.replans.unwrap_or(0),
        };

        RunScorecard {
            run_id: self.run_id,
            computed_at: Utc::now(),
            dimensions,
            overall_score,
            cost,
        }
    }

    // -- Private dimension scorers --

    fn score_success(&self) -> DimensionScore {
        let (score, detail) = match self.success {
            Some(true) => (1.0, "success=true".to_string()),
            Some(false) => (0.0, "success=false".to_string()),
            None => (0.0, "no success data available".to_string()),
        };
        DimensionScore {
            dimension: ScorecardDimension::Success,
            score,
            detail,
        }
    }

    fn score_trust_verdict(&self) -> DimensionScore {
        let (score, detail) = match self.trust_verdict {
            Some(TrustVerdict::High) => (1.0, "trust=high".to_string()),
            Some(TrustVerdict::Medium) => (0.66, "trust=medium".to_string()),
            Some(TrustVerdict::Low) => (0.33, "trust=low".to_string()),
            Some(TrustVerdict::None) => (0.0, "trust=none".to_string()),
            None => (0.0, "no trust verdict available".to_string()),
        };
        DimensionScore {
            dimension: ScorecardDimension::TrustVerdict,
            score,
            detail,
        }
    }

    fn score_verification_coverage(&self) -> DimensionScore {
        // Prefer file-based ratio when available.
        if let (Some(modified), Some(verified)) = (self.files_modified, self.files_verified) {
            if modified > 0 {
                let ratio = verified as f64 / modified as f64;
                let score = ratio.min(1.0);
                return DimensionScore {
                    dimension: ScorecardDimension::VerificationCoverage,
                    score,
                    detail: format!("{verified}/{modified} files verified"),
                };
            }
            // No files modified => full coverage trivially.
            return DimensionScore {
                dimension: ScorecardDimension::VerificationCoverage,
                score: 1.0,
                detail: "no files modified".to_string(),
            };
        }

        // Fall back to verification_passed flag (from ScenarioResult).
        let (score, detail) = match self.verification_passed {
            Some(Some(true)) => (1.0, "verification_passed=true".to_string()),
            Some(Some(false)) => (0.0, "verification_passed=false".to_string()),
            Some(None) | None => (0.5, "no verification data available".to_string()),
        };
        DimensionScore {
            dimension: ScorecardDimension::VerificationCoverage,
            score,
            detail,
        }
    }

    fn score_mission_continuity(&self) -> DimensionScore {
        let (score, detail) = match self.has_mission_content {
            Some(true) => (1.0, "mission memory has content".to_string()),
            Some(false) => (0.0, "mission memory is empty".to_string()),
            None => (0.5, "no mission memory provided".to_string()),
        };
        DimensionScore {
            dimension: ScorecardDimension::MissionContinuity,
            score,
            detail,
        }
    }

    fn score_cost_efficiency(&self) -> DimensionScore {
        let tokens = self.total_tokens.unwrap_or(0);
        let score = 1.0 - (tokens as f64 / 100_000.0).min(1.0);
        DimensionScore {
            dimension: ScorecardDimension::CostEfficiency,
            score,
            detail: format!("{tokens} tokens (ref: 100k)"),
        }
    }

    fn score_replan_stability(&self) -> DimensionScore {
        let replans = self.replans.unwrap_or(0);
        let score = match replans {
            0 => 1.0,
            1 => 0.66,
            2 => 0.33,
            _ => 0.0,
        };
        DimensionScore {
            dimension: ScorecardDimension::ReplanStability,
            score,
            detail: format!("{replans} replans"),
        }
    }

    fn score_error_rate(&self) -> DimensionScore {
        let (score, detail) = match (self.error_count, self.step_count_for_errors) {
            (Some(errors), Some(steps)) if steps > 0 => {
                let rate = errors as f64 / steps as f64;
                let s = (1.0 - rate).max(0.0);
                (s, format!("{errors} errors / {steps} steps"))
            }
            _ => (1.0, "no error data (assumed clean)".to_string()),
        };
        DimensionScore {
            dimension: ScorecardDimension::ErrorRate,
            score,
            detail,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use oco_shared_types::{
        MISSION_SCHEMA_VERSION, MissionFact, MissionMemory, MissionPlan, MissionVerificationStatus,
        PolicyPack, RunSummary, ScenarioResult, ScorecardDimension, SessionId, TaskComplexity,
        TrustVerdict, VerificationFreshness,
    };

    // -- Helpers --

    fn make_scenario_result(
        success: bool,
        tokens: u64,
        steps: u32,
        errors: Vec<String>,
    ) -> ScenarioResult {
        ScenarioResult {
            scenario_name: "test-scenario".to_string(),
            success,
            step_count: steps,
            total_tokens: tokens,
            duration_ms: 5000,
            verification_passed: Some(success),
            actions: vec!["Search".to_string()],
            errors,
            expected_match: true,
            response_generated: false,
        }
    }

    fn make_run_summary(
        trust: TrustVerdict,
        tokens: u64,
        steps: u32,
        replans: u32,
        modified: Vec<String>,
        verified: Vec<String>,
    ) -> RunSummary {
        RunSummary {
            session_id: SessionId::new(),
            request: "test request".to_string(),
            complexity: TaskComplexity::Medium,
            policy_pack: PolicyPack::Balanced,
            total_steps: steps,
            total_tokens: tokens,
            total_duration_ms: 10000,
            files_modified: modified,
            files_verified: verified,
            files_unverified: vec![],
            verification_freshness: VerificationFreshness::Fresh,
            checks_run: vec![],
            replans,
            key_decisions: vec![],
            trust_verdict: trust,
            risks: vec![],
        }
    }

    fn make_mission_memory(has_content: bool, trust: TrustVerdict) -> MissionMemory {
        let facts = if has_content {
            vec![MissionFact {
                content: "test fact".to_string(),
                source: Some("test".to_string()),
                established_at: Utc::now(),
            }]
        } else {
            vec![]
        };
        MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "test mission".to_string(),
            facts,
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: trust,
            narrative: String::new(),
        }
    }

    // -- Tests --

    #[test]
    fn builder_from_scenario_result_only() {
        let result = make_scenario_result(true, 20000, 10, vec![]);
        let scorecard = ScorecardBuilder::new("eval-1")
            .with_scenario_result(&result)
            .build();

        assert_eq!(scorecard.run_id, "eval-1");
        assert_eq!(scorecard.dimensions.len(), 7);
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::Success),
            Some(1.0)
        );
        // verification_passed=Some(true) => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::VerificationCoverage),
            Some(1.0)
        );
        // CostEfficiency: 1.0 - 20000/100000 = 0.8
        let cost_score = scorecard
            .dimension_score(ScorecardDimension::CostEfficiency)
            .expect("cost dimension present");
        assert!((cost_score - 0.8).abs() < 1e-6);
        // ErrorRate: 0 errors / 10 steps = 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::ErrorRate),
            Some(1.0)
        );
        // MissionContinuity: no mission memory => 0.5
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::MissionContinuity),
            Some(0.5)
        );
        // Cost metrics populated
        assert_eq!(scorecard.cost.tokens, 20000);
        assert_eq!(scorecard.cost.steps, 10);
    }

    #[test]
    fn builder_from_run_summary_only() {
        let summary = make_run_summary(
            TrustVerdict::High,
            50000,
            20,
            1,
            vec!["a.rs".to_string(), "b.rs".to_string()],
            vec!["a.rs".to_string()],
        );
        let scorecard = ScorecardBuilder::new("run-1")
            .with_run_summary(&summary)
            .build();

        // Success: trust != None => true => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::Success),
            Some(1.0)
        );
        // TrustVerdict: High => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::TrustVerdict),
            Some(1.0)
        );
        // VerificationCoverage: 1/2 = 0.5
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::VerificationCoverage),
            Some(0.5)
        );
        // ReplanStability: 1 replan => 0.66
        let replan = scorecard
            .dimension_score(ScorecardDimension::ReplanStability)
            .expect("replan dimension present");
        assert!((replan - 0.66).abs() < 1e-6);
        // CostEfficiency: 1.0 - 50000/100000 = 0.5
        let cost = scorecard
            .dimension_score(ScorecardDimension::CostEfficiency)
            .expect("cost dimension present");
        assert!((cost - 0.5).abs() < 1e-6);
        // Cost metrics
        assert_eq!(scorecard.cost.replans, 1);
        assert_eq!(scorecard.cost.tokens, 50000);
    }

    #[test]
    fn builder_from_all_sources() {
        let result = make_scenario_result(true, 30000, 15, vec!["oops".to_string()]);
        let summary = make_run_summary(
            TrustVerdict::Medium,
            30000,
            15,
            0,
            vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()],
            vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()],
        );
        let mission = make_mission_memory(true, TrustVerdict::Medium);

        let scorecard = ScorecardBuilder::new("full-run")
            .with_scenario_result(&result)
            .with_run_summary(&summary)
            .with_mission_memory(&mission)
            .build();

        // RunSummary overrides ScenarioResult success (trust != None => true)
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::Success),
            Some(1.0)
        );
        // TrustVerdict from RunSummary: Medium => 0.66
        let trust = scorecard
            .dimension_score(ScorecardDimension::TrustVerdict)
            .expect("trust present");
        assert!((trust - 0.66).abs() < 1e-6);
        // VerificationCoverage from RunSummary: 3/3 = 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::VerificationCoverage),
            Some(1.0)
        );
        // MissionContinuity: has_content=true => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::MissionContinuity),
            Some(1.0)
        );
        // ReplanStability: 0 replans => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::ReplanStability),
            Some(1.0)
        );
        // ErrorRate from ScenarioResult: 1 error / 15 steps
        let error_rate = scorecard
            .dimension_score(ScorecardDimension::ErrorRate)
            .expect("error rate present");
        let expected = 1.0 - (1.0 / 15.0);
        assert!((error_rate - expected).abs() < 1e-6);

        // Overall score should be in (0, 1)
        assert!(scorecard.overall_score > 0.0);
        assert!(scorecard.overall_score <= 1.0);
    }

    #[test]
    fn builder_empty_produces_defaults() {
        let scorecard = ScorecardBuilder::new("empty").build();

        assert_eq!(scorecard.run_id, "empty");
        assert_eq!(scorecard.dimensions.len(), 7);

        // Success: no data => 0.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::Success),
            Some(0.0)
        );
        // TrustVerdict: no data => 0.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::TrustVerdict),
            Some(0.0)
        );
        // VerificationCoverage: no data => 0.5
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::VerificationCoverage),
            Some(0.5)
        );
        // MissionContinuity: no mission => 0.5
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::MissionContinuity),
            Some(0.5)
        );
        // CostEfficiency: 0 tokens => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::CostEfficiency),
            Some(1.0)
        );
        // ReplanStability: 0 replans => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::ReplanStability),
            Some(1.0)
        );
        // ErrorRate: no error data => 1.0
        assert_eq!(
            scorecard.dimension_score(ScorecardDimension::ErrorRate),
            Some(1.0)
        );
        // Cost metrics all zero
        assert_eq!(scorecard.cost.tokens, 0);
        assert_eq!(scorecard.cost.steps, 0);
    }

    #[test]
    fn builder_mission_memory_continuity() {
        // With content
        let mission_full = make_mission_memory(true, TrustVerdict::High);
        let sc = ScorecardBuilder::new("m1")
            .with_mission_memory(&mission_full)
            .build();
        assert_eq!(
            sc.dimension_score(ScorecardDimension::MissionContinuity),
            Some(1.0)
        );

        // Without content
        let mission_empty = make_mission_memory(false, TrustVerdict::None);
        let sc = ScorecardBuilder::new("m2")
            .with_mission_memory(&mission_empty)
            .build();
        assert_eq!(
            sc.dimension_score(ScorecardDimension::MissionContinuity),
            Some(0.0)
        );
    }

    #[test]
    fn builder_replan_stability_levels() {
        for (replans, expected) in [(0u32, 1.0), (1, 0.66), (2, 0.33), (3, 0.0), (10, 0.0)] {
            let summary = make_run_summary(TrustVerdict::High, 1000, 5, replans, vec![], vec![]);
            let sc = ScorecardBuilder::new(format!("replan-{replans}"))
                .with_run_summary(&summary)
                .build();
            let score = sc
                .dimension_score(ScorecardDimension::ReplanStability)
                .expect("replan dimension present");
            assert!(
                (score - expected).abs() < 1e-6,
                "replans={replans}: expected {expected}, got {score}"
            );
        }
    }

    #[test]
    fn builder_cost_efficiency_normalization() {
        let cases: Vec<(u64, f64)> = vec![
            (0, 1.0),
            (25_000, 0.75),
            (50_000, 0.5),
            (100_000, 0.0),
            (200_000, 0.0), // capped at 0.0
        ];
        for (tokens, expected) in cases {
            let result = make_scenario_result(true, tokens, 10, vec![]);
            let sc = ScorecardBuilder::new(format!("cost-{tokens}"))
                .with_scenario_result(&result)
                .build();
            let score = sc
                .dimension_score(ScorecardDimension::CostEfficiency)
                .expect("cost dimension present");
            assert!(
                (score - expected).abs() < 1e-6,
                "tokens={tokens}: expected {expected}, got {score}"
            );
        }
    }

    #[test]
    fn builder_trust_verdict_scores() {
        let cases = [
            (TrustVerdict::High, 1.0),
            (TrustVerdict::Medium, 0.66),
            (TrustVerdict::Low, 0.33),
            (TrustVerdict::None, 0.0),
        ];
        for (verdict, expected) in cases {
            let summary = make_run_summary(verdict, 1000, 5, 0, vec![], vec![]);
            let sc = ScorecardBuilder::new(format!("trust-{expected}"))
                .with_run_summary(&summary)
                .build();
            let score = sc
                .dimension_score(ScorecardDimension::TrustVerdict)
                .expect("trust dimension present");
            assert!(
                (score - expected).abs() < 1e-6,
                "verdict={verdict:?}: expected {expected}, got {score}"
            );
        }
    }

    #[test]
    fn builder_verification_coverage_from_files() {
        // 3 modified, 2 verified => 0.666...
        let summary = make_run_summary(
            TrustVerdict::High,
            1000,
            5,
            0,
            vec!["a.rs".into(), "b.rs".into(), "c.rs".into()],
            vec!["a.rs".into(), "b.rs".into()],
        );
        let sc = ScorecardBuilder::new("coverage")
            .with_run_summary(&summary)
            .build();
        let score = sc
            .dimension_score(ScorecardDimension::VerificationCoverage)
            .expect("coverage dimension present");
        assert!(
            (score - 2.0 / 3.0).abs() < 1e-6,
            "expected 2/3, got {score}"
        );

        // 0 modified, 0 verified => 1.0 (trivially covered)
        let summary_empty = make_run_summary(TrustVerdict::High, 1000, 5, 0, vec![], vec![]);
        let sc = ScorecardBuilder::new("coverage-empty")
            .with_run_summary(&summary_empty)
            .build();
        assert_eq!(
            sc.dimension_score(ScorecardDimension::VerificationCoverage),
            Some(1.0)
        );
    }

    #[test]
    fn builder_serde_roundtrip() {
        let result = make_scenario_result(true, 40000, 20, vec!["err".to_string()]);
        let summary = make_run_summary(
            TrustVerdict::Medium,
            40000,
            20,
            1,
            vec!["x.rs".into()],
            vec!["x.rs".into()],
        );
        let mission = make_mission_memory(true, TrustVerdict::Medium);

        let scorecard = ScorecardBuilder::new("serde-test")
            .with_scenario_result(&result)
            .with_run_summary(&summary)
            .with_mission_memory(&mission)
            .build();

        let json = serde_json::to_string_pretty(&scorecard).expect("serialize");
        let parsed: RunScorecard = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(scorecard.run_id, parsed.run_id);
        assert_eq!(scorecard.dimensions.len(), parsed.dimensions.len());
        assert!((scorecard.overall_score - parsed.overall_score).abs() < 1e-10);
        assert_eq!(scorecard.cost, parsed.cost);

        for dim in ScorecardDimension::all() {
            assert_eq!(
                scorecard.dimension_score(*dim),
                parsed.dimension_score(*dim),
                "dimension {:?} mismatch after roundtrip",
                dim
            );
        }
    }

    // -- Granular setter tests --

    #[test]
    fn builder_granular_setters_match_typed_sources() {
        // Build via typed source
        let result = make_scenario_result(true, 30000, 10, vec!["e1".into()]);
        let via_typed = ScorecardBuilder::new("typed")
            .with_scenario_result(&result)
            .build();

        // Build via granular setters (same data)
        let via_granular = ScorecardBuilder::new("granular")
            .success(true)
            .cost(30000, 10, 5000, 0, 0)
            .errors(1, 10)
            .build();

        // Dimensions that both paths populate should produce identical scores.
        assert_eq!(
            via_typed.dimension_score(ScorecardDimension::Success),
            via_granular.dimension_score(ScorecardDimension::Success),
        );
        assert_eq!(
            via_typed.dimension_score(ScorecardDimension::CostEfficiency),
            via_granular.dimension_score(ScorecardDimension::CostEfficiency),
        );
        assert_eq!(
            via_typed.dimension_score(ScorecardDimension::ErrorRate),
            via_granular.dimension_score(ScorecardDimension::ErrorRate),
        );
    }

    #[test]
    fn builder_granular_replans_wired() {
        let sc = ScorecardBuilder::new("r").replans(2).build();
        let score = sc
            .dimension_score(ScorecardDimension::ReplanStability)
            .unwrap();
        assert!((score - 0.33).abs() < 1e-6);
        assert_eq!(sc.cost.replans, 2);
    }

    #[test]
    fn builder_granular_file_counts() {
        let sc = ScorecardBuilder::new("f").file_counts(4, 3).build();
        let score = sc
            .dimension_score(ScorecardDimension::VerificationCoverage)
            .unwrap();
        assert!((score - 0.75).abs() < 1e-6);
    }

    #[test]
    fn builder_granular_trust_verdict() {
        let sc = ScorecardBuilder::new("t")
            .trust_verdict(TrustVerdict::Low)
            .build();
        let score = sc
            .dimension_score(ScorecardDimension::TrustVerdict)
            .unwrap();
        assert!((score - 0.33).abs() < 1e-6);
    }

    #[test]
    fn builder_granular_mission_continuity() {
        let sc_yes = ScorecardBuilder::new("y").mission_continuity(true).build();
        assert_eq!(
            sc_yes.dimension_score(ScorecardDimension::MissionContinuity),
            Some(1.0)
        );
        let sc_no = ScorecardBuilder::new("n").mission_continuity(false).build();
        assert_eq!(
            sc_no.dimension_score(ScorecardDimension::MissionContinuity),
            Some(0.0)
        );
    }

    // ── with_weights ──

    #[test]
    fn builder_with_weights_changes_overall() {
        // Build without custom weights
        let sc_default = ScorecardBuilder::new("default")
            .success(true) // Success=1.0 (default weight 3.0)
            .trust_verdict(TrustVerdict::Low) // TrustVerdict=0.33 (default weight 2.0)
            .build();

        // Build with custom weights: make trust_verdict much heavier
        let weights = ScorecardWeights {
            success: Some(1.0),
            trust_verdict: Some(10.0),
            ..Default::default()
        };
        let sc_custom = ScorecardBuilder::new("custom")
            .success(true)
            .trust_verdict(TrustVerdict::Low)
            .with_weights(weights)
            .build();

        // Dimension scores should be identical
        assert_eq!(
            sc_default.dimension_score(ScorecardDimension::Success),
            sc_custom.dimension_score(ScorecardDimension::Success),
        );
        assert_eq!(
            sc_default.dimension_score(ScorecardDimension::TrustVerdict),
            sc_custom.dimension_score(ScorecardDimension::TrustVerdict),
        );

        // But overall scores should differ because weights are different
        assert!(
            (sc_default.overall_score - sc_custom.overall_score).abs() > 0.01,
            "custom weights should change overall: default={}, custom={}",
            sc_default.overall_score,
            sc_custom.overall_score,
        );

        // With trust_verdict heavily weighted and low (0.33), custom overall should be lower
        assert!(
            sc_custom.overall_score < sc_default.overall_score,
            "heavier trust weight with low trust should lower overall"
        );
    }

    #[test]
    fn builder_without_weights_matches_default() {
        let sc = ScorecardBuilder::new("no-weights")
            .success(true)
            .trust_verdict(TrustVerdict::High)
            .build();

        // Manually compute expected overall with default weights
        let expected = RunScorecard::compute_overall(&sc.dimensions);
        assert!((sc.overall_score - expected).abs() < 1e-10);
    }
}
