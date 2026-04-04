//! Q6: Evaluation gate — baseline-driven quality gates for CI and review.
//!
//! A [`GatePolicy`] defines per-dimension thresholds and a verdict strategy.
//! A [`GateResult`] is the outcome of evaluating a candidate [`RunScorecard`]
//! against a baseline, producing a `pass / warn / fail` verdict with reasons.
//!
//! These types power the `oco eval gate` CLI surface and enable CI integration.

use serde::{Deserialize, Serialize};

use crate::{RegressionSeverity, RunScorecard, ScorecardComparison, ScorecardDimension};

// ---------------------------------------------------------------------------
// Gate verdict
// ---------------------------------------------------------------------------

/// Outcome of a quality gate evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    /// All thresholds met, no critical regressions.
    Pass,
    /// Minor regressions or threshold warnings, but no blockers.
    Warn,
    /// Critical regression or threshold violation — blocks merge/deploy.
    Fail,
}

impl GateVerdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Pass => "[PASS]",
            Self::Warn => "[WARN]",
            Self::Fail => "[FAIL]",
        }
    }

    /// Exit code for CLI usage: 0 = pass, 1 = warn, 2 = fail.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Pass => 0,
            Self::Warn => 1,
            Self::Fail => 2,
        }
    }

    /// True if this verdict should block a CI pipeline.
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Fail)
    }
}

// ---------------------------------------------------------------------------
// Gate threshold
// ---------------------------------------------------------------------------

/// Per-dimension minimum threshold for gate evaluation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateThreshold {
    pub dimension: ScorecardDimension,
    /// Minimum acceptable score (0.0–1.0). Below this = fail.
    pub min_score: f64,
    /// Maximum acceptable regression delta (negative). Beyond this = fail.
    /// E.g., -0.2 means a drop of more than 0.2 triggers failure.
    pub max_regression: f64,
}

impl GateThreshold {
    pub fn new(dimension: ScorecardDimension, min_score: f64, max_regression: f64) -> Self {
        Self {
            dimension,
            min_score,
            max_regression,
        }
    }
}

// ---------------------------------------------------------------------------
// Gate policy
// ---------------------------------------------------------------------------

/// Strategy for combining per-dimension results into a final verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStrategy {
    /// Fail if ANY dimension fails its threshold.
    Strict,
    /// Fail only if a critical dimension (Success, TrustVerdict) fails.
    /// Other dimension failures produce warnings.
    Balanced,
    /// Only fail on critical regressions (severity=Critical).
    Lenient,
}

/// A complete gate policy: thresholds + strategy + overall minimum score.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GatePolicy {
    /// Per-dimension thresholds.
    pub thresholds: Vec<GateThreshold>,
    /// How to combine dimension results.
    pub strategy: GateStrategy,
    /// Minimum overall composite score (0.0–1.0). Below this = fail regardless
    /// of per-dimension results.
    pub min_overall_score: f64,
    /// Maximum allowed overall regression delta (negative value).
    /// E.g., -0.1 means an overall drop > 0.1 triggers failure.
    pub max_overall_regression: f64,
}

impl GatePolicy {
    /// Sensible production defaults: balanced strategy, reasonable thresholds.
    pub fn default_balanced() -> Self {
        Self {
            thresholds: vec![
                GateThreshold::new(ScorecardDimension::Success, 0.5, -0.5),
                GateThreshold::new(ScorecardDimension::TrustVerdict, 0.3, -0.3),
                GateThreshold::new(ScorecardDimension::VerificationCoverage, 0.3, -0.3),
                GateThreshold::new(ScorecardDimension::MissionContinuity, 0.0, -0.5),
                GateThreshold::new(ScorecardDimension::CostEfficiency, 0.0, -0.5),
                GateThreshold::new(ScorecardDimension::ReplanStability, 0.0, -0.5),
                GateThreshold::new(ScorecardDimension::ErrorRate, 0.3, -0.3),
            ],
            strategy: GateStrategy::Balanced,
            min_overall_score: 0.4,
            max_overall_regression: -0.15,
        }
    }

    /// Strict policy: higher thresholds, any failure blocks.
    pub fn strict() -> Self {
        Self {
            thresholds: vec![
                GateThreshold::new(ScorecardDimension::Success, 1.0, -0.01),
                GateThreshold::new(ScorecardDimension::TrustVerdict, 0.6, -0.2),
                GateThreshold::new(ScorecardDimension::VerificationCoverage, 0.5, -0.2),
                GateThreshold::new(ScorecardDimension::MissionContinuity, 0.3, -0.3),
                GateThreshold::new(ScorecardDimension::CostEfficiency, 0.2, -0.3),
                GateThreshold::new(ScorecardDimension::ReplanStability, 0.2, -0.3),
                GateThreshold::new(ScorecardDimension::ErrorRate, 0.5, -0.2),
            ],
            strategy: GateStrategy::Strict,
            min_overall_score: 0.6,
            max_overall_regression: -0.1,
        }
    }

    /// Lenient policy: only block on critical failures.
    pub fn lenient() -> Self {
        Self {
            thresholds: vec![
                GateThreshold::new(ScorecardDimension::Success, 0.0, -1.0),
                GateThreshold::new(ScorecardDimension::TrustVerdict, 0.0, -1.0),
                GateThreshold::new(ScorecardDimension::VerificationCoverage, 0.0, -1.0),
                GateThreshold::new(ScorecardDimension::MissionContinuity, 0.0, -1.0),
                GateThreshold::new(ScorecardDimension::CostEfficiency, 0.0, -1.0),
                GateThreshold::new(ScorecardDimension::ReplanStability, 0.0, -1.0),
                GateThreshold::new(ScorecardDimension::ErrorRate, 0.0, -1.0),
            ],
            strategy: GateStrategy::Lenient,
            min_overall_score: 0.0,
            max_overall_regression: -0.5,
        }
    }

    /// Look up threshold for a specific dimension.
    pub fn threshold_for(&self, dim: ScorecardDimension) -> Option<&GateThreshold> {
        self.thresholds.iter().find(|t| t.dimension == dim)
    }
}

// ---------------------------------------------------------------------------
// Dimension gate check
// ---------------------------------------------------------------------------

/// Result of checking a single dimension against its threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionGateCheck {
    pub dimension: ScorecardDimension,
    pub candidate_score: f64,
    pub baseline_score: f64,
    pub delta: f64,
    pub min_score: f64,
    pub max_regression: f64,
    pub verdict: GateVerdict,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Gate result
// ---------------------------------------------------------------------------

/// Full gate evaluation result — the reviewable artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    /// Baseline identifier.
    pub baseline_id: String,
    /// Candidate identifier.
    pub candidate_id: String,
    /// The policy that was applied.
    pub policy: GatePolicy,
    /// Per-dimension check results.
    pub dimension_checks: Vec<DimensionGateCheck>,
    /// Overall scores.
    pub baseline_overall: f64,
    pub candidate_overall: f64,
    pub overall_delta: f64,
    /// The underlying scorecard comparison (for full detail).
    pub comparison: ScorecardComparison,
    /// Final verdict.
    pub verdict: GateVerdict,
    /// Human-readable reasons that drove the verdict.
    pub reasons: Vec<String>,
}

impl GateResult {
    /// Evaluate a candidate against a baseline using the given policy.
    pub fn evaluate(
        baseline: &RunScorecard,
        candidate: &RunScorecard,
        policy: &GatePolicy,
    ) -> Self {
        let comparison = ScorecardComparison::compare(baseline, candidate);
        let mut dimension_checks = Vec::new();
        let mut reasons = Vec::new();
        let mut has_fail = false;
        let mut has_warn = false;

        for dim in ScorecardDimension::all() {
            let b_score = baseline.dimension_score(*dim).unwrap_or(0.0);
            let c_score = candidate.dimension_score(*dim).unwrap_or(0.0);
            let delta = c_score - b_score;

            let threshold = policy.threshold_for(*dim);
            let min_score = threshold.map(|t| t.min_score).unwrap_or(0.0);
            let max_regression = threshold.map(|t| t.max_regression).unwrap_or(-1.0);

            let dim_verdict = if c_score < min_score {
                let reason = format!(
                    "{}: score {:.2} below minimum {:.2}",
                    dim.label(),
                    c_score,
                    min_score
                );
                reasons.push(reason.clone());
                // In balanced mode, only critical dims cause fail
                match policy.strategy {
                    GateStrategy::Strict => {
                        has_fail = true;
                        GateVerdict::Fail
                    }
                    GateStrategy::Balanced => {
                        if is_critical_dimension(*dim) {
                            has_fail = true;
                            GateVerdict::Fail
                        } else {
                            has_warn = true;
                            GateVerdict::Warn
                        }
                    }
                    GateStrategy::Lenient => {
                        has_warn = true;
                        GateVerdict::Warn
                    }
                }
            } else if delta < max_regression {
                let reason = format!(
                    "{}: regression {:.2} exceeds limit {:.2}",
                    dim.label(),
                    delta,
                    max_regression
                );
                reasons.push(reason.clone());
                match policy.strategy {
                    GateStrategy::Strict => {
                        has_fail = true;
                        GateVerdict::Fail
                    }
                    GateStrategy::Balanced => {
                        if is_critical_dimension(*dim) {
                            has_fail = true;
                            GateVerdict::Fail
                        } else {
                            has_warn = true;
                            GateVerdict::Warn
                        }
                    }
                    GateStrategy::Lenient => {
                        // Only fail on critical severity regressions
                        let is_critical_regression = comparison.regressions.iter().any(|r| {
                            r.dimension == *dim && r.severity == RegressionSeverity::Critical
                        });
                        if is_critical_regression {
                            has_fail = true;
                            GateVerdict::Fail
                        } else {
                            has_warn = true;
                            GateVerdict::Warn
                        }
                    }
                }
            } else {
                GateVerdict::Pass
            };

            dimension_checks.push(DimensionGateCheck {
                dimension: *dim,
                candidate_score: c_score,
                baseline_score: b_score,
                delta,
                min_score,
                max_regression,
                verdict: dim_verdict,
                reason: if dim_verdict == GateVerdict::Pass {
                    "ok".to_string()
                } else {
                    reasons.last().cloned().unwrap_or_default()
                },
            });
        }

        // Check overall score thresholds
        let overall_delta = candidate.overall_score - baseline.overall_score;

        if candidate.overall_score < policy.min_overall_score {
            let reason = format!(
                "overall score {:.2} below minimum {:.2}",
                candidate.overall_score, policy.min_overall_score
            );
            reasons.push(reason);
            has_fail = true;
        }

        if overall_delta < policy.max_overall_regression {
            let reason = format!(
                "overall regression {:.2} exceeds limit {:.2}",
                overall_delta, policy.max_overall_regression
            );
            reasons.push(reason);
            has_fail = true;
        }

        let verdict = if has_fail {
            GateVerdict::Fail
        } else if has_warn {
            GateVerdict::Warn
        } else {
            GateVerdict::Pass
        };

        Self {
            baseline_id: baseline.run_id.clone(),
            candidate_id: candidate.run_id.clone(),
            policy: policy.clone(),
            dimension_checks,
            baseline_overall: baseline.overall_score,
            candidate_overall: candidate.overall_score,
            overall_delta,
            comparison,
            verdict,
            reasons,
        }
    }

    /// Format as a human-readable gate report.
    pub fn to_report(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Eval Gate: {} vs {}",
            self.baseline_id, self.candidate_id
        ));
        lines.push(format!(
            "  Policy: {:?} | min_overall: {:.2} | max_regression: {:.2}",
            self.policy.strategy, self.policy.min_overall_score, self.policy.max_overall_regression,
        ));
        lines.push(format!(
            "  Overall: {:.2} -> {:.2} (delta: {:+.2})",
            self.baseline_overall, self.candidate_overall, self.overall_delta,
        ));
        lines.push(String::new());

        // Dimension table
        lines.push(
            "  Dimension                Baseline  Candidate  Delta   Min    Verdict".to_string(),
        );
        lines.push(
            "  -----------------------------------------------------------------------".to_string(),
        );
        for check in &self.dimension_checks {
            lines.push(format!(
                "  {:<24} {:>7.2}   {:>8.2}  {:>+6.2}  {:>5.2}  {}",
                check.dimension.label(),
                check.baseline_score,
                check.candidate_score,
                check.delta,
                check.min_score,
                check.verdict.symbol(),
            ));
        }

        if !self.reasons.is_empty() {
            lines.push(String::new());
            lines.push(format!("  Reasons ({}):", self.reasons.len()));
            for reason in &self.reasons {
                lines.push(format!("    - {reason}"));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "  Verdict: {} {}",
            self.verdict.symbol(),
            self.verdict.label()
        ));
        lines.push(format!("  Exit code: {}", self.verdict.exit_code()));

        lines.join("\n")
    }

    /// Count of failed dimensions.
    pub fn failed_dimension_count(&self) -> usize {
        self.dimension_checks
            .iter()
            .filter(|c| c.verdict == GateVerdict::Fail)
            .count()
    }

    /// Count of warned dimensions.
    pub fn warned_dimension_count(&self) -> usize {
        self.dimension_checks
            .iter()
            .filter(|c| c.verdict == GateVerdict::Warn)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Baseline reference
// ---------------------------------------------------------------------------

/// A saved baseline: a scorecard snapshot with metadata for identification.
///
/// Baselines are saved as `baseline.json` and loaded by `oco eval gate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalBaseline {
    /// Human-readable name (e.g., "v0.5-stable", "main-2026-04-01").
    pub name: String,
    /// When this baseline was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The scorecard snapshot.
    pub scorecard: RunScorecard,
    /// Optional description/notes.
    pub description: Option<String>,
    /// Source: run ID, eval file, or manual.
    pub source: String,
}

impl EvalBaseline {
    /// Create a baseline from an existing scorecard.
    pub fn from_scorecard(
        name: impl Into<String>,
        scorecard: RunScorecard,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            created_at: chrono::Utc::now(),
            scorecard,
            description: None,
            source: source.into(),
        }
    }

    /// Create with a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Save to a JSON file.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize baseline: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("failed to write baseline: {e}"))
    }

    /// Load from a JSON file.
    pub fn load_from(path: &std::path::Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("failed to read baseline: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("failed to parse baseline: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Dimensions that are considered "critical" for balanced gate strategy.
fn is_critical_dimension(dim: ScorecardDimension) -> bool {
    matches!(
        dim,
        ScorecardDimension::Success | ScorecardDimension::TrustVerdict
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CostMetrics, DimensionScore};
    use chrono::Utc;

    fn make_scorecard(run_id: &str, scores: &[(ScorecardDimension, f64)]) -> RunScorecard {
        let dimensions: Vec<DimensionScore> = scores
            .iter()
            .map(|(dim, score)| DimensionScore {
                dimension: *dim,
                score: *score,
                detail: "test".to_string(),
            })
            .collect();
        let overall = RunScorecard::compute_overall(&dimensions);
        RunScorecard {
            run_id: run_id.to_string(),
            computed_at: Utc::now(),
            dimensions,
            overall_score: overall,
            cost: CostMetrics::default(),
        }
    }

    fn full_scorecard(run_id: &str, base: f64) -> RunScorecard {
        let scores: Vec<(ScorecardDimension, f64)> = ScorecardDimension::all()
            .iter()
            .map(|d| (*d, base))
            .collect();
        make_scorecard(run_id, &scores)
    }

    // ── GateVerdict ──

    #[test]
    fn verdict_labels() {
        assert_eq!(GateVerdict::Pass.label(), "pass");
        assert_eq!(GateVerdict::Warn.label(), "warn");
        assert_eq!(GateVerdict::Fail.label(), "fail");
    }

    #[test]
    fn verdict_exit_codes() {
        assert_eq!(GateVerdict::Pass.exit_code(), 0);
        assert_eq!(GateVerdict::Warn.exit_code(), 1);
        assert_eq!(GateVerdict::Fail.exit_code(), 2);
    }

    #[test]
    fn verdict_is_blocking() {
        assert!(!GateVerdict::Pass.is_blocking());
        assert!(!GateVerdict::Warn.is_blocking());
        assert!(GateVerdict::Fail.is_blocking());
    }

    #[test]
    fn verdict_serde_roundtrip() {
        for v in [GateVerdict::Pass, GateVerdict::Warn, GateVerdict::Fail] {
            let json = serde_json::to_string(&v).unwrap();
            let parsed: GateVerdict = serde_json::from_str(&json).unwrap();
            assert_eq!(v, parsed);
        }
    }

    // ── GatePolicy presets ──

    #[test]
    fn default_balanced_policy_has_all_dimensions() {
        let policy = GatePolicy::default_balanced();
        assert_eq!(policy.thresholds.len(), 7);
        assert_eq!(policy.strategy, GateStrategy::Balanced);
        for dim in ScorecardDimension::all() {
            assert!(policy.threshold_for(*dim).is_some(), "missing {dim:?}");
        }
    }

    #[test]
    fn strict_policy_has_higher_thresholds() {
        let strict = GatePolicy::strict();
        let balanced = GatePolicy::default_balanced();
        assert!(strict.min_overall_score >= balanced.min_overall_score);
        let strict_success = strict.threshold_for(ScorecardDimension::Success).unwrap();
        let balanced_success = balanced.threshold_for(ScorecardDimension::Success).unwrap();
        assert!(strict_success.min_score >= balanced_success.min_score);
    }

    #[test]
    fn lenient_policy_has_zero_thresholds() {
        let policy = GatePolicy::lenient();
        for t in &policy.thresholds {
            assert_eq!(t.min_score, 0.0);
        }
    }

    #[test]
    fn policy_serde_roundtrip() {
        let policy = GatePolicy::default_balanced();
        let json = serde_json::to_string_pretty(&policy).unwrap();
        let parsed: GatePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, parsed);
    }

    // ── GateResult::evaluate ──

    #[test]
    fn identical_scorecards_pass() {
        let baseline = full_scorecard("base", 0.8);
        let candidate = full_scorecard("cand", 0.8);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Pass);
        assert!(result.reasons.is_empty());
        assert_eq!(result.failed_dimension_count(), 0);
    }

    #[test]
    fn improved_scorecards_pass() {
        let baseline = full_scorecard("base", 0.6);
        let candidate = full_scorecard("cand", 0.9);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Pass);
    }

    #[test]
    fn critical_regression_fails_balanced() {
        let baseline = full_scorecard("base", 0.9);
        let candidate = full_scorecard("cand", 0.2);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Fail);
        assert!(!result.reasons.is_empty());
    }

    #[test]
    fn success_below_minimum_fails_strict() {
        let baseline = full_scorecard("base", 0.8);
        // Candidate with Success=0 (below strict min of 1.0)
        let mut scores: Vec<(ScorecardDimension, f64)> = ScorecardDimension::all()
            .iter()
            .map(|d| (*d, 0.8))
            .collect();
        scores[0] = (ScorecardDimension::Success, 0.0);
        let candidate = make_scorecard("cand", &scores);
        let policy = GatePolicy::strict();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Fail);
        assert!(result.reasons.iter().any(|r| r.contains("success")));
    }

    #[test]
    fn non_critical_regression_warns_balanced() {
        let baseline = full_scorecard("base", 0.8);
        // Drop only CostEfficiency significantly
        let mut scores: Vec<(ScorecardDimension, f64)> = ScorecardDimension::all()
            .iter()
            .map(|d| (*d, 0.8))
            .collect();
        // CostEfficiency is index 4
        scores[4] = (ScorecardDimension::CostEfficiency, 0.1);
        let candidate = make_scorecard("cand", &scores);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        // CostEfficiency is not critical in balanced mode — should warn, not fail
        // (unless overall regression triggers fail)
        assert!(
            result.verdict == GateVerdict::Warn || result.verdict == GateVerdict::Pass,
            "expected warn or pass for non-critical regression, got {:?}",
            result.verdict
        );
    }

    #[test]
    fn overall_score_below_minimum_fails() {
        let baseline = full_scorecard("base", 0.8);
        let candidate = full_scorecard("cand", 0.1);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Fail);
        assert!(
            result.reasons.iter().any(|r| r.contains("overall")),
            "expected overall-related reason, got: {:?}",
            result.reasons
        );
    }

    #[test]
    fn overall_regression_beyond_limit_fails() {
        let baseline = full_scorecard("base", 0.9);
        let candidate = full_scorecard("cand", 0.5);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Fail);
    }

    #[test]
    fn lenient_policy_tolerates_minor_drops() {
        let baseline = full_scorecard("base", 0.8);
        let candidate = full_scorecard("cand", 0.7);
        let policy = GatePolicy::lenient();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        // Lenient: no min_score thresholds, high regression tolerance
        assert_ne!(result.verdict, GateVerdict::Fail);
    }

    #[test]
    fn report_contains_key_elements() {
        let baseline = full_scorecard("base-v1", 0.9);
        let candidate = full_scorecard("cand-v2", 0.4);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        let report = result.to_report();

        assert!(report.contains("base-v1"));
        assert!(report.contains("cand-v2"));
        assert!(report.contains("[FAIL]"));
        assert!(report.contains("Reasons"));
        assert!(report.contains("Exit code: 2"));
    }

    #[test]
    fn report_pass_has_no_reasons() {
        let baseline = full_scorecard("base", 0.8);
        let candidate = full_scorecard("cand", 0.8);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        let report = result.to_report();

        assert!(report.contains("[PASS]"));
        assert!(!report.contains("Reasons"));
    }

    #[test]
    fn gate_result_serde_roundtrip() {
        let baseline = full_scorecard("base", 0.8);
        let candidate = full_scorecard("cand", 0.6);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);

        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: GateResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.verdict, parsed.verdict);
        assert_eq!(result.reasons.len(), parsed.reasons.len());
        assert_eq!(result.dimension_checks.len(), parsed.dimension_checks.len());
    }

    #[test]
    fn failed_and_warned_counts() {
        let baseline = full_scorecard("base", 0.9);
        let candidate = full_scorecard("cand", 0.3);
        let policy = GatePolicy::strict();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert!(result.failed_dimension_count() > 0);
        // Total checks = dimension_checks that are fail + warn + pass
        assert_eq!(result.dimension_checks.len(), 7);
    }

    // ── EvalBaseline ──

    #[test]
    fn baseline_from_scorecard() {
        let sc = full_scorecard("test-run", 0.85);
        let baseline = EvalBaseline::from_scorecard("v1-stable", sc.clone(), "manual");
        assert_eq!(baseline.name, "v1-stable");
        assert_eq!(baseline.scorecard.run_id, "test-run");
        assert_eq!(baseline.source, "manual");
        assert!(baseline.description.is_none());
    }

    #[test]
    fn baseline_with_description() {
        let sc = full_scorecard("test", 0.8);
        let baseline = EvalBaseline::from_scorecard("v1", sc, "eval")
            .with_description("Stable baseline from CI");
        assert_eq!(
            baseline.description.as_deref(),
            Some("Stable baseline from CI")
        );
    }

    #[test]
    fn baseline_serde_roundtrip() {
        let sc = full_scorecard("b-run", 0.75);
        let baseline =
            EvalBaseline::from_scorecard("b1", sc, "eval").with_description("test baseline");
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let parsed: EvalBaseline = serde_json::from_str(&json).unwrap();
        assert_eq!(baseline.name, parsed.name);
        assert_eq!(baseline.source, parsed.source);
        assert_eq!(
            baseline.scorecard.overall_score,
            parsed.scorecard.overall_score
        );
    }

    #[test]
    fn baseline_save_and_load() {
        let sc = full_scorecard("persist-test", 0.7);
        let baseline = EvalBaseline::from_scorecard("persist", sc, "test");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");

        baseline.save_to(&path).unwrap();
        let loaded = EvalBaseline::load_from(&path).unwrap();
        assert_eq!(baseline.name, loaded.name);
        assert_eq!(
            baseline.scorecard.overall_score,
            loaded.scorecard.overall_score
        );
    }

    // ── GateStrategy ──

    #[test]
    fn strategy_serde_roundtrip() {
        for s in [
            GateStrategy::Strict,
            GateStrategy::Balanced,
            GateStrategy::Lenient,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let parsed: GateStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(s, parsed);
        }
    }

    // ── GateThreshold ──

    #[test]
    fn threshold_construction() {
        let t = GateThreshold::new(ScorecardDimension::Success, 0.9, -0.1);
        assert_eq!(t.dimension, ScorecardDimension::Success);
        assert!((t.min_score - 0.9).abs() < 1e-10);
        assert!((t.max_regression - (-0.1)).abs() < 1e-10);
    }

    #[test]
    fn threshold_serde_roundtrip() {
        let t = GateThreshold::new(ScorecardDimension::ErrorRate, 0.5, -0.2);
        let json = serde_json::to_string(&t).unwrap();
        let parsed: GateThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(t, parsed);
    }

    // ── Edge cases ──

    #[test]
    fn gate_with_empty_policy_thresholds() {
        let baseline = full_scorecard("base", 0.8);
        let candidate = full_scorecard("cand", 0.8);
        let policy = GatePolicy {
            thresholds: vec![],
            strategy: GateStrategy::Balanced,
            min_overall_score: 0.0,
            max_overall_regression: -1.0,
        };
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        assert_eq!(result.verdict, GateVerdict::Pass);
    }

    #[test]
    fn gate_with_zero_scores() {
        let baseline = full_scorecard("base", 0.0);
        let candidate = full_scorecard("cand", 0.0);
        let policy = GatePolicy::default_balanced();
        let result = GateResult::evaluate(&baseline, &candidate, &policy);
        // Both at 0.0 — no regression delta, but below min thresholds
        assert!(
            result.verdict == GateVerdict::Fail || result.verdict == GateVerdict::Warn,
            "expected fail or warn with zero scores"
        );
    }
}
