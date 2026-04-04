//! Q5: Evaluation scorecard — measurable, comparable run/scenario results.
//!
//! A [`RunScorecard`] captures how a run performed across multiple dimensions.
//! [`ScorecardComparison`] compares two scorecards and flags regressions.
//! These types power the `oco eval compare` and `oco runs compare` surfaces.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------------

/// Evaluation dimensions tracked by a scorecard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScorecardDimension {
    /// Did the run complete successfully?
    Success,
    /// Trust verdict quality (High=1.0, Medium=0.66, Low=0.33, None=0.0).
    TrustVerdict,
    /// Ratio of verified files to modified files (0.0–1.0).
    VerificationCoverage,
    /// Was a mission memory produced with substantive content?
    MissionContinuity,
    /// Cost efficiency: lower is better, normalized against budget.
    CostEfficiency,
    /// Stability: did the run avoid excessive replanning?
    ReplanStability,
    /// Error rate: ratio of error-free steps.
    ErrorRate,
}

impl ScorecardDimension {
    /// All known dimensions in canonical order.
    pub fn all() -> &'static [ScorecardDimension] {
        &[
            Self::Success,
            Self::TrustVerdict,
            Self::VerificationCoverage,
            Self::MissionContinuity,
            Self::CostEfficiency,
            Self::ReplanStability,
            Self::ErrorRate,
        ]
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::TrustVerdict => "trust_verdict",
            Self::VerificationCoverage => "verification_coverage",
            Self::MissionContinuity => "mission_continuity",
            Self::CostEfficiency => "cost_efficiency",
            Self::ReplanStability => "replan_stability",
            Self::ErrorRate => "error_rate",
        }
    }

    /// Default weight for composite score computation.
    pub fn default_weight(&self) -> f64 {
        match self {
            Self::Success => 3.0,
            Self::TrustVerdict => 2.0,
            Self::VerificationCoverage => 1.5,
            Self::MissionContinuity => 1.0,
            Self::CostEfficiency => 1.0,
            Self::ReplanStability => 0.5,
            Self::ErrorRate => 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Dimension score
// ---------------------------------------------------------------------------

/// A single dimension's score within a scorecard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DimensionScore {
    pub dimension: ScorecardDimension,
    /// Normalized score from 0.0 (worst) to 1.0 (best).
    pub score: f64,
    /// Human-readable explanation of how this score was derived.
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Cost metrics
// ---------------------------------------------------------------------------

/// Raw cost metrics for a run — not normalized, used for comparison.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CostMetrics {
    pub steps: u32,
    pub tokens: u64,
    pub duration_ms: u64,
    pub tool_calls: u32,
    pub verify_cycles: u32,
    pub replans: u32,
}

// ---------------------------------------------------------------------------
// RunScorecard
// ---------------------------------------------------------------------------

/// Structured evaluation scorecard for a single run or scenario.
///
/// Answers: how did this run perform across multiple dimensions?
/// Designed for comparison, regression detection, and trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunScorecard {
    /// Identifier: scenario name or session ID.
    pub run_id: String,
    /// When this scorecard was computed.
    pub computed_at: DateTime<Utc>,
    /// Per-dimension scores.
    pub dimensions: Vec<DimensionScore>,
    /// Overall composite score (0.0–1.0), weighted average of dimensions.
    pub overall_score: f64,
    /// Raw cost metrics.
    pub cost: CostMetrics,
}

impl RunScorecard {
    /// Look up a specific dimension's score.
    pub fn dimension_score(&self, dim: ScorecardDimension) -> Option<f64> {
        self.dimensions
            .iter()
            .find(|d| d.dimension == dim)
            .map(|d| d.score)
    }

    /// Compute the weighted overall score from dimension scores.
    pub fn compute_overall(dimensions: &[DimensionScore]) -> f64 {
        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;
        for d in dimensions {
            let w = d.dimension.default_weight();
            weighted_sum += d.score * w;
            weight_total += w;
        }
        if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            0.0
        }
    }
}

// ---------------------------------------------------------------------------
// Comparison types
// ---------------------------------------------------------------------------

/// Severity of a regression between two scorecards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionSeverity {
    /// Score dropped by >= 0.5 or success changed from pass to fail.
    Critical,
    /// Score dropped by >= 0.2.
    Warning,
    /// Score dropped by < 0.2.
    Minor,
}

/// A detected regression on one dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionFlag {
    pub dimension: ScorecardDimension,
    pub baseline_score: f64,
    pub candidate_score: f64,
    /// Negative delta means regression.
    pub delta: f64,
    pub severity: RegressionSeverity,
}

/// A detected improvement on one dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementFlag {
    pub dimension: ScorecardDimension,
    pub baseline_score: f64,
    pub candidate_score: f64,
    /// Positive delta means improvement.
    pub delta: f64,
}

/// Overall verdict when comparing two scorecards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonVerdict {
    /// Overall score improved and no critical regressions.
    Improved,
    /// No significant changes.
    Stable,
    /// Overall score dropped or critical regression detected.
    Regressed,
}

impl ComparisonVerdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Improved => "improved",
            Self::Stable => "stable",
            Self::Regressed => "regressed",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Improved => "[UP]",
            Self::Stable => "[==]",
            Self::Regressed => "[DOWN]",
        }
    }
}

/// Full comparison result between a baseline and a candidate scorecard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScorecardComparison {
    pub baseline_id: String,
    pub candidate_id: String,
    pub baseline_overall: f64,
    pub candidate_overall: f64,
    pub overall_delta: f64,
    pub regressions: Vec<RegressionFlag>,
    pub improvements: Vec<ImprovementFlag>,
    pub verdict: ComparisonVerdict,
}

impl ScorecardComparison {
    /// Compare two scorecards and produce a detailed comparison.
    pub fn compare(baseline: &RunScorecard, candidate: &RunScorecard) -> Self {
        let mut regressions = Vec::new();
        let mut improvements = Vec::new();

        for dim in ScorecardDimension::all() {
            let b_score = baseline.dimension_score(*dim).unwrap_or(0.0);
            let c_score = candidate.dimension_score(*dim).unwrap_or(0.0);
            let delta = c_score - b_score;

            if delta < -0.01 {
                let severity = if delta <= -0.5 {
                    RegressionSeverity::Critical
                } else if delta <= -0.2 {
                    RegressionSeverity::Warning
                } else {
                    RegressionSeverity::Minor
                };
                regressions.push(RegressionFlag {
                    dimension: *dim,
                    baseline_score: b_score,
                    candidate_score: c_score,
                    delta,
                    severity,
                });
            } else if delta > 0.01 {
                improvements.push(ImprovementFlag {
                    dimension: *dim,
                    baseline_score: b_score,
                    candidate_score: c_score,
                    delta,
                });
            }
        }

        let overall_delta = candidate.overall_score - baseline.overall_score;
        let has_critical = regressions
            .iter()
            .any(|r| r.severity == RegressionSeverity::Critical);

        let verdict = if has_critical || overall_delta < -0.1 {
            ComparisonVerdict::Regressed
        } else if overall_delta > 0.05 {
            ComparisonVerdict::Improved
        } else {
            ComparisonVerdict::Stable
        };

        Self {
            baseline_id: baseline.run_id.clone(),
            candidate_id: candidate.run_id.clone(),
            baseline_overall: baseline.overall_score,
            candidate_overall: candidate.overall_score,
            overall_delta,
            regressions,
            improvements,
            verdict,
        }
    }

    /// True if any critical regression was detected.
    pub fn has_critical_regression(&self) -> bool {
        self.regressions
            .iter()
            .any(|r| r.severity == RegressionSeverity::Critical)
    }

    /// Format as a human-readable comparison report.
    pub fn to_report(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Scorecard Comparison: {} vs {}",
            self.baseline_id, self.candidate_id
        ));
        lines.push(format!(
            "  Overall: {:.2} -> {:.2} (delta: {:+.2}) {}",
            self.baseline_overall,
            self.candidate_overall,
            self.overall_delta,
            self.verdict.symbol(),
        ));

        if !self.regressions.is_empty() {
            lines.push(format!("  REGRESSIONS ({}):", self.regressions.len()));
            for r in &self.regressions {
                lines.push(format!(
                    "    {:?} {}: {:.2} -> {:.2} ({:+.2}) [{:?}]",
                    r.severity,
                    r.dimension.label(),
                    r.baseline_score,
                    r.candidate_score,
                    r.delta,
                    r.severity,
                ));
            }
        }

        if !self.improvements.is_empty() {
            lines.push(format!("  IMPROVEMENTS ({}):", self.improvements.len()));
            for i in &self.improvements {
                lines.push(format!(
                    "    {}: {:.2} -> {:.2} ({:+.2})",
                    i.dimension.label(),
                    i.baseline_score,
                    i.candidate_score,
                    i.delta,
                ));
            }
        }

        lines.push(format!(
            "  Verdict: {} {}",
            self.verdict.symbol(),
            self.verdict.label()
        ));
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Batch comparison (scenario suite)
// ---------------------------------------------------------------------------

/// Aggregate comparison across multiple scenario scorecards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchComparison {
    pub comparisons: Vec<ScorecardComparison>,
    pub total_scenarios: usize,
    pub improved_count: usize,
    pub stable_count: usize,
    pub regressed_count: usize,
    pub overall_verdict: ComparisonVerdict,
}

impl BatchComparison {
    /// Build a batch comparison from parallel baseline/candidate scorecard vectors.
    ///
    /// Pairs are matched by `run_id`. Unmatched scorecards are skipped.
    pub fn from_paired(baselines: &[RunScorecard], candidates: &[RunScorecard]) -> Self {
        let mut comparisons = Vec::new();
        for baseline in baselines {
            if let Some(candidate) = candidates.iter().find(|c| c.run_id == baseline.run_id) {
                comparisons.push(ScorecardComparison::compare(baseline, candidate));
            }
        }

        let improved_count = comparisons
            .iter()
            .filter(|c| c.verdict == ComparisonVerdict::Improved)
            .count();
        let regressed_count = comparisons
            .iter()
            .filter(|c| c.verdict == ComparisonVerdict::Regressed)
            .count();
        let stable_count = comparisons.len() - improved_count - regressed_count;

        let overall_verdict = if regressed_count > 0 {
            ComparisonVerdict::Regressed
        } else if improved_count > stable_count {
            ComparisonVerdict::Improved
        } else {
            ComparisonVerdict::Stable
        };

        Self {
            total_scenarios: comparisons.len(),
            comparisons,
            improved_count,
            stable_count,
            regressed_count,
            overall_verdict,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scorecard(run_id: &str, scores: &[(ScorecardDimension, f64, &str)]) -> RunScorecard {
        let dimensions: Vec<DimensionScore> = scores
            .iter()
            .map(|(dim, score, detail)| DimensionScore {
                dimension: *dim,
                score: *score,
                detail: detail.to_string(),
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

    fn full_scorecard(run_id: &str, base_score: f64) -> RunScorecard {
        let scores: Vec<(ScorecardDimension, f64, &str)> = ScorecardDimension::all()
            .iter()
            .map(|d| (*d, base_score, "test"))
            .collect();
        make_scorecard(run_id, &scores)
    }

    // ── ScorecardDimension ──

    #[test]
    fn dimension_all_returns_seven() {
        assert_eq!(ScorecardDimension::all().len(), 7);
    }

    #[test]
    fn dimension_labels_unique() {
        let labels: Vec<&str> = ScorecardDimension::all()
            .iter()
            .map(|d| d.label())
            .collect();
        let set: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), set.len());
    }

    #[test]
    fn dimension_weights_positive() {
        for dim in ScorecardDimension::all() {
            assert!(dim.default_weight() > 0.0);
        }
    }

    #[test]
    fn dimension_serde_roundtrip() {
        for dim in ScorecardDimension::all() {
            let json = serde_json::to_string(dim).unwrap();
            let parsed: ScorecardDimension = serde_json::from_str(&json).unwrap();
            assert_eq!(*dim, parsed);
        }
    }

    // ── RunScorecard ──

    #[test]
    fn compute_overall_weighted() {
        let scores = vec![
            DimensionScore {
                dimension: ScorecardDimension::Success,
                score: 1.0,
                detail: "passed".into(),
            },
            DimensionScore {
                dimension: ScorecardDimension::CostEfficiency,
                score: 0.5,
                detail: "50% budget".into(),
            },
        ];
        let overall = RunScorecard::compute_overall(&scores);
        // Success weight=3.0, CostEfficiency weight=1.0
        // (1.0*3.0 + 0.5*1.0) / (3.0 + 1.0) = 3.5 / 4.0 = 0.875
        assert!((overall - 0.875).abs() < 1e-6);
    }

    #[test]
    fn compute_overall_empty() {
        assert_eq!(RunScorecard::compute_overall(&[]), 0.0);
    }

    #[test]
    fn dimension_score_lookup() {
        let sc = full_scorecard("test", 0.8);
        assert_eq!(sc.dimension_score(ScorecardDimension::Success), Some(0.8));
        assert_eq!(
            sc.dimension_score(ScorecardDimension::TrustVerdict),
            Some(0.8)
        );
    }

    #[test]
    fn scorecard_serde_roundtrip() {
        let sc = full_scorecard("round-trip", 0.75);
        let json = serde_json::to_string_pretty(&sc).unwrap();
        let parsed: RunScorecard = serde_json::from_str(&json).unwrap();
        assert_eq!(sc.run_id, parsed.run_id);
        assert_eq!(sc.dimensions.len(), parsed.dimensions.len());
        assert!((sc.overall_score - parsed.overall_score).abs() < 1e-10);
    }

    // ── ScorecardComparison ──

    #[test]
    fn compare_identical_is_stable() {
        let a = full_scorecard("a", 0.8);
        let b = full_scorecard("b", 0.8);
        let cmp = ScorecardComparison::compare(&a, &b);
        assert_eq!(cmp.verdict, ComparisonVerdict::Stable);
        assert!(cmp.regressions.is_empty());
        assert!(cmp.improvements.is_empty());
    }

    #[test]
    fn compare_improvement_detected() {
        let baseline = full_scorecard("base", 0.5);
        let candidate = full_scorecard("cand", 0.9);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        assert_eq!(cmp.verdict, ComparisonVerdict::Improved);
        assert!(!cmp.improvements.is_empty());
        assert!(cmp.regressions.is_empty());
        assert!(cmp.overall_delta > 0.0);
    }

    #[test]
    fn compare_regression_detected() {
        let baseline = full_scorecard("base", 0.9);
        let candidate = full_scorecard("cand", 0.3);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        assert_eq!(cmp.verdict, ComparisonVerdict::Regressed);
        assert!(!cmp.regressions.is_empty());
        assert!(cmp.has_critical_regression());
        assert!(cmp.overall_delta < 0.0);
    }

    #[test]
    fn compare_critical_on_success_drop() {
        let baseline = make_scorecard("base", &[(ScorecardDimension::Success, 1.0, "pass")]);
        let candidate = make_scorecard("cand", &[(ScorecardDimension::Success, 0.0, "fail")]);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        assert!(cmp.has_critical_regression());
        let success_reg = cmp
            .regressions
            .iter()
            .find(|r| r.dimension == ScorecardDimension::Success)
            .unwrap();
        assert_eq!(success_reg.severity, RegressionSeverity::Critical);
    }

    #[test]
    fn compare_minor_regression() {
        let baseline = make_scorecard("base", &[(ScorecardDimension::CostEfficiency, 0.8, "good")]);
        let candidate = make_scorecard("cand", &[(ScorecardDimension::CostEfficiency, 0.7, "ok")]);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        assert_eq!(cmp.regressions.len(), 1);
        assert_eq!(cmp.regressions[0].severity, RegressionSeverity::Minor);
    }

    #[test]
    fn compare_warning_regression() {
        let baseline = make_scorecard("base", &[(ScorecardDimension::TrustVerdict, 0.9, "high")]);
        let candidate =
            make_scorecard("cand", &[(ScorecardDimension::TrustVerdict, 0.6, "medium")]);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        let reg = &cmp.regressions[0];
        assert_eq!(reg.severity, RegressionSeverity::Warning);
    }

    #[test]
    fn comparison_report_contains_key_info() {
        let baseline = full_scorecard("baseline-run", 0.9);
        let candidate = full_scorecard("candidate-run", 0.4);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        let report = cmp.to_report();
        assert!(report.contains("baseline-run"));
        assert!(report.contains("candidate-run"));
        assert!(report.contains("[DOWN]"));
        assert!(report.contains("REGRESSIONS"));
    }

    #[test]
    fn comparison_serde_roundtrip() {
        let baseline = full_scorecard("a", 0.8);
        let candidate = full_scorecard("b", 0.6);
        let cmp = ScorecardComparison::compare(&baseline, &candidate);
        let json = serde_json::to_string(&cmp).unwrap();
        let parsed: ScorecardComparison = serde_json::from_str(&json).unwrap();
        assert_eq!(cmp.verdict, parsed.verdict);
        assert_eq!(cmp.regressions.len(), parsed.regressions.len());
    }

    // ── ComparisonVerdict ──

    #[test]
    fn verdict_labels() {
        assert_eq!(ComparisonVerdict::Improved.label(), "improved");
        assert_eq!(ComparisonVerdict::Stable.label(), "stable");
        assert_eq!(ComparisonVerdict::Regressed.label(), "regressed");
    }

    #[test]
    fn verdict_serde_roundtrip() {
        for verdict in [
            ComparisonVerdict::Improved,
            ComparisonVerdict::Stable,
            ComparisonVerdict::Regressed,
        ] {
            let json = serde_json::to_string(&verdict).unwrap();
            let parsed: ComparisonVerdict = serde_json::from_str(&json).unwrap();
            assert_eq!(verdict, parsed);
        }
    }

    // ── CostMetrics ──

    #[test]
    fn cost_metrics_default() {
        let cost = CostMetrics::default();
        assert_eq!(cost.steps, 0);
        assert_eq!(cost.tokens, 0);
        assert_eq!(cost.replans, 0);
    }

    #[test]
    fn cost_metrics_serde() {
        let cost = CostMetrics {
            steps: 5,
            tokens: 10000,
            duration_ms: 30000,
            tool_calls: 12,
            verify_cycles: 2,
            replans: 1,
        };
        let json = serde_json::to_string(&cost).unwrap();
        let parsed: CostMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(cost, parsed);
    }

    // ── BatchComparison ──

    #[test]
    fn batch_comparison_pairs_by_run_id() {
        let baselines = vec![
            full_scorecard("scenario-1", 0.8),
            full_scorecard("scenario-2", 0.7),
            full_scorecard("scenario-3", 0.9),
        ];
        let candidates = vec![
            full_scorecard("scenario-1", 0.85),
            full_scorecard("scenario-2", 0.3), // regression
                                               // scenario-3 missing
        ];
        let batch = BatchComparison::from_paired(&baselines, &candidates);
        assert_eq!(batch.total_scenarios, 2); // only matched pairs
        assert_eq!(batch.regressed_count, 1);
        assert_eq!(batch.overall_verdict, ComparisonVerdict::Regressed);
    }

    #[test]
    fn batch_comparison_all_stable() {
        let baselines = vec![full_scorecard("s1", 0.8), full_scorecard("s2", 0.8)];
        let candidates = vec![full_scorecard("s1", 0.8), full_scorecard("s2", 0.8)];
        let batch = BatchComparison::from_paired(&baselines, &candidates);
        assert_eq!(batch.overall_verdict, ComparisonVerdict::Stable);
        assert_eq!(batch.stable_count, 2);
    }

    #[test]
    fn batch_comparison_all_improved() {
        let baselines = vec![full_scorecard("s1", 0.5), full_scorecard("s2", 0.5)];
        let candidates = vec![full_scorecard("s1", 0.9), full_scorecard("s2", 0.9)];
        let batch = BatchComparison::from_paired(&baselines, &candidates);
        assert_eq!(batch.overall_verdict, ComparisonVerdict::Improved);
        assert_eq!(batch.improved_count, 2);
    }

    #[test]
    fn batch_comparison_empty() {
        let batch = BatchComparison::from_paired(&[], &[]);
        assert_eq!(batch.total_scenarios, 0);
        assert_eq!(batch.overall_verdict, ComparisonVerdict::Stable);
    }

    // ── RegressionSeverity ──

    #[test]
    fn regression_severity_serde() {
        for sev in [
            RegressionSeverity::Critical,
            RegressionSeverity::Warning,
            RegressionSeverity::Minor,
        ] {
            let json = serde_json::to_string(&sev).unwrap();
            let parsed: RegressionSeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(sev, parsed);
        }
    }
}
