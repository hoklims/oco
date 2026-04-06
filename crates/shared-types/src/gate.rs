//! Q6: Evaluation gate — baseline-driven quality gates for CI and review.
//!
//! A [`GatePolicy`] defines per-dimension thresholds and a verdict strategy.
//! A [`GateResult`] is the outcome of evaluating a candidate [`RunScorecard`]
//! against a baseline, producing a `pass / warn / fail` verdict with reasons.
//!
//! These types power the `oco eval gate` CLI surface and enable CI integration.

use chrono::{DateTime, Utc};
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
    pub created_at: DateTime<Utc>,
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
            created_at: Utc::now(),
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
// Repo gate configuration (Q7)
// ---------------------------------------------------------------------------

/// Per-repo gate configuration, typically declared in `oco.toml` under `[gate]`.
///
/// Allows a repository to express its quality contract — baseline location,
/// default policy, and optional threshold overrides — so that `oco eval-gate`
/// can be run with zero additional arguments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GateConfig {
    /// Path to the baseline file, relative to the workspace root.
    /// Default: `.oco/baseline.json`.
    pub baseline_path: String,
    /// Default gate policy name: `strict`, `balanced`, or `lenient`.
    /// Default: `balanced`.
    pub default_policy: String,
    /// Minimum overall composite score override (0.0–1.0).
    /// When set, overrides the preset policy's `min_overall_score`.
    pub min_overall_score: Option<f64>,
    /// Maximum allowed overall regression delta override (negative value).
    /// When set, overrides the preset policy's `max_overall_regression`.
    pub max_overall_regression: Option<f64>,
    /// Q8: Days before a baseline is considered "aging". Default: 14.
    pub fresh_days: Option<u32>,
    /// Q8: Days before a baseline is considered "stale". Default: 30.
    pub stale_days: Option<u32>,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            baseline_path: ".oco/baseline.json".into(),
            default_policy: "balanced".into(),
            min_overall_score: None,
            max_overall_regression: None,
            fresh_days: None,
            stale_days: None,
        }
    }
}

impl GateConfig {
    /// Resolve the gate policy from this configuration.
    ///
    /// Starts from the named preset (`strict`, `balanced`, `lenient`), then
    /// applies any overrides for `min_overall_score` and `max_overall_regression`.
    pub fn resolve_policy(&self) -> GatePolicy {
        let mut policy = match self.default_policy.as_str() {
            "strict" => GatePolicy::strict(),
            "lenient" => GatePolicy::lenient(),
            _ => GatePolicy::default_balanced(),
        };
        if let Some(min) = self.min_overall_score {
            policy.min_overall_score = min;
        }
        if let Some(max_reg) = self.max_overall_regression {
            policy.max_overall_regression = max_reg;
        }
        policy
    }

    /// Validate semantic constraints.
    pub fn validate(&self) -> Result<(), String> {
        let valid_policies = ["strict", "balanced", "lenient"];
        if !valid_policies.contains(&self.default_policy.as_str()) {
            return Err(format!(
                "unknown gate policy '{}', expected one of: {}",
                self.default_policy,
                valid_policies.join(", ")
            ));
        }
        if let Some(min) = self.min_overall_score
            && !(0.0..=1.0).contains(&min)
        {
            return Err(format!(
                "min_overall_score must be between 0.0 and 1.0, got {min}"
            ));
        }
        if let Some(max_reg) = self.max_overall_regression
            && max_reg > 0.0
        {
            return Err(format!(
                "max_overall_regression must be <= 0.0, got {max_reg}"
            ));
        }
        if let (Some(fresh), Some(stale)) = (self.fresh_days, self.stale_days)
            && fresh > stale
        {
            return Err(format!(
                "fresh_days ({fresh}) must be <= stale_days ({stale})"
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Baseline freshness (Q8)
// ---------------------------------------------------------------------------

/// How fresh a baseline is relative to configured thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineFreshness {
    /// Within the freshness window — no concerns.
    Fresh,
    /// Past the freshness window but not yet stale — update recommended.
    Aging,
    /// Past the staleness window — baseline should be refreshed before trusting gate results.
    Stale,
    /// Cannot determine age (no `created_at` or no thresholds configured).
    Unknown,
}

impl BaselineFreshness {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Aging => "aging",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Fresh => "[FRESH]",
            Self::Aging => "[AGING]",
            Self::Stale => "[STALE]",
            Self::Unknown => "[?]",
        }
    }

    /// Whether the gate result should carry a warning about baseline age.
    pub fn warrants_warning(&self) -> bool {
        matches!(self, Self::Aging | Self::Stale)
    }
}

/// Result of evaluating a baseline's freshness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineFreshnessCheck {
    pub freshness: BaselineFreshness,
    /// Age of the baseline in days (fractional). `None` when age is unknown
    /// (e.g., raw `RunScorecard` baselines without `created_at`).
    pub age_days: Option<f64>,
    /// Threshold in days: below this the baseline is `Fresh`.
    pub fresh_threshold_days: u32,
    /// Threshold in days: above this the baseline is `Stale`.
    pub stale_threshold_days: u32,
    /// Human-readable recommendation.
    pub recommendation: String,
}

impl BaselineFreshnessCheck {
    /// Default freshness threshold in days (baseline considered "aging" after this).
    pub const DEFAULT_FRESH_DAYS: u32 = 14;
    /// Default staleness threshold in days (baseline considered "stale" after this).
    pub const DEFAULT_STALE_DAYS: u32 = 30;

    /// Evaluate baseline freshness from its `created_at` timestamp.
    ///
    /// `now` is passed explicitly for testability.
    /// `fresh_days` / `stale_days` can be `None` to use defaults.
    pub fn evaluate(
        created_at: DateTime<Utc>,
        now: DateTime<Utc>,
        fresh_days: Option<u32>,
        stale_days: Option<u32>,
    ) -> Self {
        let fresh_d = fresh_days.unwrap_or(Self::DEFAULT_FRESH_DAYS);
        let stale_d = stale_days.unwrap_or(Self::DEFAULT_STALE_DAYS);
        let age = now.signed_duration_since(created_at);
        let age_days = age.num_seconds() as f64 / 86_400.0;

        let (freshness, effective_age, recommendation) = if age_days < 0.0 {
            // Baseline is in the future — treat as unknown, age is meaningless
            (
                BaselineFreshness::Unknown,
                None,
                "baseline created_at is in the future".to_string(),
            )
        } else if age_days <= fresh_d as f64 {
            (
                BaselineFreshness::Fresh,
                Some(age_days),
                format!("baseline is {age_days:.1} days old (fresh threshold: {fresh_d}d)"),
            )
        } else if age_days <= stale_d as f64 {
            (
                BaselineFreshness::Aging,
                Some(age_days),
                format!(
                    "baseline is {age_days:.1} days old — consider updating (stale after {stale_d}d)",
                ),
            )
        } else {
            (
                BaselineFreshness::Stale,
                Some(age_days),
                format!(
                    "baseline is {age_days:.1} days old — stale (threshold: {stale_d}d). Update before trusting gate results.",
                ),
            )
        };

        Self {
            freshness,
            age_days: effective_age,
            fresh_threshold_days: fresh_d,
            stale_threshold_days: stale_d,
            recommendation,
        }
    }

    /// Shortcut: evaluate from an `EvalBaseline` using current time.
    pub fn from_baseline(
        baseline: &EvalBaseline,
        fresh_days: Option<u32>,
        stale_days: Option<u32>,
    ) -> Self {
        Self::evaluate(baseline.created_at, Utc::now(), fresh_days, stale_days)
    }

    /// Produce an `Unknown` freshness check for baselines without `created_at`
    /// (e.g., raw `RunScorecard` files that were not saved via `oco baseline-save`).
    pub fn unknown() -> Self {
        Self {
            freshness: BaselineFreshness::Unknown,
            age_days: None,
            fresh_threshold_days: Self::DEFAULT_FRESH_DAYS,
            stale_threshold_days: Self::DEFAULT_STALE_DAYS,
            recommendation: "baseline has no created_at metadata — save it via \
                             `oco baseline-save` for freshness tracking"
                .to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Review artifact (Q8)
// ---------------------------------------------------------------------------

/// High-level summary for a gate review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub verdict: GateVerdict,
    pub baseline_name: String,
    pub candidate_id: String,
    /// Human-readable score change, e.g., "0.82 → 0.78 (−0.04)".
    pub overall_change: String,
    pub dimensions_passing: usize,
    pub dimensions_warning: usize,
    pub dimensions_failing: usize,
    pub baseline_freshness: BaselineFreshness,
}

/// Structured review artifact produced by the gate evaluation.
///
/// Contains everything a reviewer needs: the gate result, baseline freshness
/// assessment, a human-readable summary, and actionable recommendations.
/// Can be serialized to JSON or rendered as Markdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateReviewArtifact {
    /// When this artifact was generated.
    pub generated_at: DateTime<Utc>,
    /// The full gate evaluation result.
    pub gate_result: GateResult,
    /// Baseline freshness assessment.
    pub baseline_freshness: BaselineFreshnessCheck,
    /// High-level summary.
    pub summary: ReviewSummary,
    /// Actionable recommendations.
    pub recommendations: Vec<String>,
}

impl GateReviewArtifact {
    /// Generate a review artifact from a gate result and baseline metadata.
    pub fn generate(
        gate_result: GateResult,
        baseline: &EvalBaseline,
        freshness_check: BaselineFreshnessCheck,
    ) -> Self {
        Self::generate_with_name(gate_result, &baseline.name, freshness_check)
    }

    /// Generate a review artifact using an explicit baseline name.
    ///
    /// Use this when the baseline is a raw `RunScorecard` without `EvalBaseline`
    /// metadata — pass the scorecard's `run_id` (or any descriptive label) as the name.
    pub fn generate_with_name(
        gate_result: GateResult,
        baseline_name: &str,
        freshness_check: BaselineFreshnessCheck,
    ) -> Self {
        let summary = ReviewSummary {
            verdict: gate_result.verdict,
            baseline_name: baseline_name.to_string(),
            candidate_id: gate_result.candidate_id.clone(),
            overall_change: format!(
                "{:.2} → {:.2} ({:+.2})",
                gate_result.baseline_overall,
                gate_result.candidate_overall,
                gate_result.overall_delta,
            ),
            dimensions_passing: gate_result
                .dimension_checks
                .iter()
                .filter(|c| c.verdict == GateVerdict::Pass)
                .count(),
            dimensions_warning: gate_result.warned_dimension_count(),
            dimensions_failing: gate_result.failed_dimension_count(),
            baseline_freshness: freshness_check.freshness,
        };

        let mut recommendations = Vec::new();

        // Freshness recommendations
        if freshness_check.freshness == BaselineFreshness::Unknown {
            recommendations.push(format!(
                "Baseline freshness unknown: {}",
                freshness_check.recommendation,
            ));
        } else if freshness_check.freshness.warrants_warning() {
            recommendations.push(format!(
                "Baseline is {}: {}",
                freshness_check.freshness.label(),
                freshness_check.recommendation,
            ));
        }

        // Gate-driven recommendations
        if gate_result.verdict == GateVerdict::Fail {
            recommendations
                .push("Gate FAILED — investigate failing dimensions before merging.".to_string());
        }
        if gate_result.verdict == GateVerdict::Warn {
            recommendations.push("Gate produced warnings — review flagged dimensions.".to_string());
        }

        // Dimension-specific advice
        for check in &gate_result.dimension_checks {
            if check.verdict == GateVerdict::Fail {
                recommendations.push(format!(
                    "{}: score {:.2} (min {:.2}), delta {:+.2} — requires attention",
                    check.dimension.label(),
                    check.candidate_score,
                    check.min_score,
                    check.delta,
                ));
            }
        }

        if recommendations.is_empty() {
            recommendations.push("No action required — gate passed cleanly.".to_string());
        }

        Self {
            generated_at: Utc::now(),
            gate_result,
            baseline_freshness: freshness_check,
            summary,
            recommendations,
        }
    }

    /// Render as a Markdown review document.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        // Header
        md.push_str("# Gate Review Report\n\n");
        md.push_str(&format!(
            "**Verdict:** {} {}\n\n",
            self.summary.verdict.symbol(),
            self.summary.verdict.label().to_uppercase(),
        ));

        // Summary table
        md.push_str("## Summary\n\n");
        md.push_str("| Field | Value |\n");
        md.push_str("|-------|-------|\n");
        md.push_str(&format!("| Baseline | {} |\n", self.summary.baseline_name));
        md.push_str(&format!("| Candidate | {} |\n", self.summary.candidate_id));
        md.push_str(&format!(
            "| Overall score | {} |\n",
            self.summary.overall_change
        ));
        md.push_str(&format!(
            "| Baseline freshness | {} {} |\n",
            self.baseline_freshness.freshness.symbol(),
            self.baseline_freshness.freshness.label(),
        ));
        match self.baseline_freshness.age_days {
            Some(days) => md.push_str(&format!("| Baseline age | {days:.1} days |\n")),
            None => md.push_str("| Baseline age | n/a |\n"),
        }
        md.push_str(&format!(
            "| Dimensions | {} pass, {} warn, {} fail |\n",
            self.summary.dimensions_passing,
            self.summary.dimensions_warning,
            self.summary.dimensions_failing,
        ));
        md.push_str(&format!(
            "| Policy | {:?} (min: {:.2}, max_reg: {:.2}) |\n",
            self.gate_result.policy.strategy,
            self.gate_result.policy.min_overall_score,
            self.gate_result.policy.max_overall_regression,
        ));

        // Dimension detail table
        md.push_str("\n## Dimensions\n\n");
        md.push_str("| Dimension | Baseline | Candidate | Delta | Min | Verdict |\n");
        md.push_str("|-----------|----------|-----------|-------|-----|--------|\n");
        for check in &self.gate_result.dimension_checks {
            md.push_str(&format!(
                "| {} | {:.2} | {:.2} | {:+.2} | {:.2} | {} |\n",
                check.dimension.label(),
                check.baseline_score,
                check.candidate_score,
                check.delta,
                check.min_score,
                check.verdict.symbol(),
            ));
        }

        // Reasons
        if !self.gate_result.reasons.is_empty() {
            md.push_str("\n## Reasons\n\n");
            for reason in &self.gate_result.reasons {
                md.push_str(&format!("- {reason}\n"));
            }
        }

        // Recommendations
        md.push_str("\n## Recommendations\n\n");
        for rec in &self.recommendations {
            md.push_str(&format!("- {rec}\n"));
        }

        // Footer
        md.push_str(&format!(
            "\n---\n*Generated by OCO eval-gate at {}*\n",
            self.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
        ));

        md
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize review artifact: {e}"))
    }

    /// Save Markdown to a file.
    pub fn save_markdown(&self, path: &std::path::Path) -> Result<(), String> {
        let md = self.to_markdown();
        std::fs::write(path, md).map_err(|e| format!("failed to write markdown report: {e}"))
    }

    /// Save JSON to a file.
    pub fn save_json(&self, path: &std::path::Path) -> Result<(), String> {
        let json = self.to_json()?;
        std::fs::write(path, json).map_err(|e| format!("failed to write JSON report: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Q11: Baseline promotion & audit trail
// ---------------------------------------------------------------------------

/// Recommendation for whether a candidate should be promoted to baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionRecommendation {
    /// Candidate passes all checks — safe to promote.
    Promote,
    /// Candidate has warnings — promotion requires explicit review.
    Review,
    /// Candidate fails critical checks — do not promote.
    Reject,
}

impl PromotionRecommendation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Promote => "promote",
            Self::Review => "review",
            Self::Reject => "reject",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Promote => "[PROMOTE]",
            Self::Review => "[REVIEW]",
            Self::Reject => "[REJECT]",
        }
    }

    /// Derive a recommendation from gate verdict + baseline freshness.
    ///
    /// Rules:
    /// - Gate Fail → Reject
    /// - Gate Warn OR baseline Stale → Review
    /// - Gate Pass + baseline Fresh/Aging/Unknown → Promote
    pub fn from_gate_and_freshness(
        gate_verdict: GateVerdict,
        freshness: BaselineFreshness,
    ) -> Self {
        match gate_verdict {
            GateVerdict::Fail => Self::Reject,
            GateVerdict::Warn => Self::Review,
            GateVerdict::Pass => {
                if freshness == BaselineFreshness::Stale {
                    Self::Review
                } else {
                    Self::Promote
                }
            }
        }
    }
}

/// Summary of differences between old and new baselines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineDiffSummary {
    /// Per-dimension score changes (dimension label → delta).
    pub dimension_deltas: Vec<DimensionDelta>,
    /// Overall score change.
    pub old_overall: f64,
    pub new_overall: f64,
    pub overall_delta: f64,
    /// Human-readable summary line.
    pub summary: String,
}

/// A single dimension's score change between old and new baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionDelta {
    pub dimension: ScorecardDimension,
    pub old_score: f64,
    pub new_score: f64,
    pub delta: f64,
}

impl BaselineDiffSummary {
    /// Compute a diff between two scorecards (old baseline vs new candidate).
    pub fn compute(old: &RunScorecard, new: &RunScorecard) -> Self {
        let dimension_deltas: Vec<DimensionDelta> = ScorecardDimension::all()
            .iter()
            .map(|dim| {
                let old_score = old.dimension_score(*dim).unwrap_or(0.0);
                let new_score = new.dimension_score(*dim).unwrap_or(0.0);
                DimensionDelta {
                    dimension: *dim,
                    old_score,
                    new_score,
                    delta: new_score - old_score,
                }
            })
            .collect();

        let overall_delta = new.overall_score - old.overall_score;

        let improved = dimension_deltas.iter().filter(|d| d.delta > 0.01).count();
        let regressed = dimension_deltas.iter().filter(|d| d.delta < -0.01).count();
        let unchanged = dimension_deltas.len() - improved - regressed;

        let summary = format!(
            "{:.2} → {:.2} ({:+.2}): {} improved, {} regressed, {} unchanged",
            old.overall_score, new.overall_score, overall_delta, improved, regressed, unchanged,
        );

        Self {
            dimension_deltas,
            old_overall: old.overall_score,
            new_overall: new.overall_score,
            overall_delta,
            summary,
        }
    }

    /// Format as a human-readable table.
    pub fn to_report(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Baseline Diff: {}", self.summary));
        lines.push(String::new());
        lines.push("  Dimension                Old       New      Delta".to_string());
        lines.push("  ---------------------------------------------------".to_string());
        for d in &self.dimension_deltas {
            let marker = if d.delta > 0.01 {
                "+"
            } else if d.delta < -0.01 {
                "-"
            } else {
                " "
            };
            lines.push(format!(
                "  {:<24} {:>7.2}   {:>7.2}  {:>+6.2} {}",
                d.dimension.label(),
                d.old_score,
                d.new_score,
                d.delta,
                marker,
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "  Overall: {:.2} → {:.2} ({:+.2})",
            self.old_overall, self.new_overall, self.overall_delta,
        ));
        lines.join("\n")
    }

    /// Format as Markdown table.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("## Baseline Diff\n\n");
        md.push_str(&format!("**Overall:** {}\n\n", self.summary));
        md.push_str("| Dimension | Old | New | Delta |\n");
        md.push_str("|-----------|-----|-----|-------|\n");
        for d in &self.dimension_deltas {
            md.push_str(&format!(
                "| {} | {:.2} | {:.2} | {:+.2} |\n",
                d.dimension.label(),
                d.old_score,
                d.new_score,
                d.delta,
            ));
        }
        md
    }
}

/// A durable record of a baseline promotion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRecord {
    /// When the promotion occurred.
    pub promoted_at: DateTime<Utc>,
    /// Name of the old baseline being replaced.
    pub old_baseline_name: String,
    /// Name of the new baseline.
    pub new_baseline_name: String,
    /// Source of the new baseline (run ID, file path, etc.).
    pub source: String,
    /// Human-provided reason for the promotion.
    pub reason: Option<String>,
    /// Computed recommendation at promotion time.
    pub recommendation: PromotionRecommendation,
    /// Gate verdict at promotion time (if available).
    pub gate_verdict: Option<GateVerdict>,
    /// Baseline freshness at promotion time (if available).
    pub baseline_freshness: Option<BaselineFreshness>,
    /// Diff between old and new baseline scorecards.
    pub diff: BaselineDiffSummary,
}

impl PromotionRecord {
    /// Format as a human-readable summary.
    pub fn to_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "{} {} → {} ({})",
            self.recommendation.symbol(),
            self.old_baseline_name,
            self.new_baseline_name,
            self.promoted_at.format("%Y-%m-%d %H:%M:%S UTC"),
        ));
        lines.push(format!("  Source: {}", self.source));
        if let Some(reason) = &self.reason {
            lines.push(format!("  Reason: {reason}"));
        }
        if let Some(gv) = self.gate_verdict {
            lines.push(format!("  Gate: {}", gv.symbol()));
        }
        if let Some(bf) = self.baseline_freshness {
            lines.push(format!("  Old baseline freshness: {}", bf.symbol()));
        }
        lines.push(format!("  Score: {}", self.diff.summary));
        lines.join("\n")
    }

    /// Format as Markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!(
            "### Promotion: {} → {}\n\n",
            self.old_baseline_name, self.new_baseline_name,
        ));
        md.push_str("| Field | Value |\n");
        md.push_str("|-------|-------|\n");
        md.push_str(&format!(
            "| Date | {} |\n",
            self.promoted_at.format("%Y-%m-%d %H:%M:%S UTC"),
        ));
        md.push_str(&format!(
            "| Recommendation | {} |\n",
            self.recommendation.symbol(),
        ));
        md.push_str(&format!("| Source | {} |\n", self.source));
        if let Some(reason) = &self.reason {
            md.push_str(&format!("| Reason | {reason} |\n"));
        }
        if let Some(gv) = self.gate_verdict {
            md.push_str(&format!("| Gate verdict | {} |\n", gv.symbol()));
        }
        if let Some(bf) = self.baseline_freshness {
            md.push_str(&format!("| Old baseline freshness | {} |\n", bf.symbol(),));
        }
        md.push_str(&format!("| Score change | {} |\n", self.diff.summary));
        md.push('\n');
        md.push_str(&self.diff.to_markdown());
        md
    }
}

/// An entry in the baseline audit trail / history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineHistoryEntry {
    /// Monotonic sequence number (1-based).
    pub sequence: u32,
    /// The promotion record for this entry.
    pub promotion: PromotionRecord,
}

/// The full baseline audit trail for a repository.
///
/// Persisted as a JSON file (typically `.oco/baseline-history.json`).
/// Append-only in normal operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineHistory {
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// History entries, ordered by sequence number (oldest first).
    pub entries: Vec<BaselineHistoryEntry>,
}

/// Current baseline history schema version.
pub const BASELINE_HISTORY_SCHEMA_VERSION: u32 = 1;

impl Default for BaselineHistory {
    fn default() -> Self {
        Self {
            schema_version: BASELINE_HISTORY_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

impl BaselineHistory {
    /// Create a new empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a promotion record and return the new sequence number.
    pub fn append(&mut self, promotion: PromotionRecord) -> u32 {
        let sequence = self.entries.last().map_or(1, |e| e.sequence + 1);
        self.entries.push(BaselineHistoryEntry {
            sequence,
            promotion,
        });
        sequence
    }

    /// Get the most recent entry, if any.
    pub fn latest(&self) -> Option<&BaselineHistoryEntry> {
        self.entries.last()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the last `n` entries (most recent first).
    pub fn recent(&self, n: usize) -> Vec<&BaselineHistoryEntry> {
        self.entries.iter().rev().take(n).collect()
    }

    /// Save to a JSON file.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize baseline history: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("failed to write baseline history: {e}"))
    }

    /// Load from a JSON file. Returns an empty history if the file doesn't exist.
    pub fn load_from(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read baseline history: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("failed to parse baseline history: {e}"))
    }

    /// Format all entries as a human-readable report.
    pub fn to_report(&self) -> String {
        if self.entries.is_empty() {
            return "No baseline promotions recorded.".to_string();
        }
        let mut lines = Vec::new();
        lines.push(format!("Baseline History ({} entries)", self.entries.len()));
        lines.push("=".repeat(50));
        for entry in self.entries.iter().rev() {
            lines.push(String::new());
            lines.push(format!("#{}", entry.sequence));
            lines.push(entry.promotion.to_summary());
        }
        lines.join("\n")
    }

    /// Format as Markdown.
    pub fn to_markdown(&self) -> String {
        if self.entries.is_empty() {
            return "# Baseline History\n\nNo promotions recorded.\n".to_string();
        }
        let mut md = String::new();
        md.push_str(&format!(
            "# Baseline History ({} entries)\n\n",
            self.entries.len(),
        ));
        for entry in self.entries.iter().rev() {
            md.push_str(&format!("---\n\n**#{}**\n\n", entry.sequence));
            md.push_str(&entry.promotion.to_markdown());
            md.push('\n');
        }
        md
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize baseline history: {e}"))
    }

    /// Prune old entries, keeping only the most recent `keep` entries.
    ///
    /// Returns the number of entries removed.
    /// If `keep >= self.len()`, no entries are removed.
    pub fn prune(&mut self, keep: usize) -> usize {
        if self.entries.len() <= keep {
            return 0;
        }
        let removed = self.entries.len() - keep;
        // Keep the tail (most recent entries).
        let start = self.entries.len() - keep;
        self.entries = self.entries.split_off(start);
        removed
    }

    /// Preview which entries would be removed by `prune(keep)`.
    ///
    /// Returns the entries that would be dropped (oldest first), without
    /// modifying the history.
    pub fn prune_preview(&self, keep: usize) -> Vec<&BaselineHistoryEntry> {
        if self.entries.len() <= keep {
            return Vec::new();
        }
        let drop_count = self.entries.len() - keep;
        self.entries.iter().take(drop_count).collect()
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

    // ── GateConfig (Q7) ──

    #[test]
    fn gate_config_default() {
        let cfg = GateConfig::default();
        assert_eq!(cfg.baseline_path, ".oco/baseline.json");
        assert_eq!(cfg.default_policy, "balanced");
        assert!(cfg.min_overall_score.is_none());
        assert!(cfg.max_overall_regression.is_none());
    }

    #[test]
    fn gate_config_resolve_policy_balanced() {
        let cfg = GateConfig::default();
        let policy = cfg.resolve_policy();
        assert_eq!(policy.strategy, GateStrategy::Balanced);
        assert!((policy.min_overall_score - 0.4).abs() < 1e-10);
    }

    #[test]
    fn gate_config_resolve_policy_strict() {
        let cfg = GateConfig {
            default_policy: "strict".into(),
            ..Default::default()
        };
        let policy = cfg.resolve_policy();
        assert_eq!(policy.strategy, GateStrategy::Strict);
    }

    #[test]
    fn gate_config_resolve_policy_lenient() {
        let cfg = GateConfig {
            default_policy: "lenient".into(),
            ..Default::default()
        };
        let policy = cfg.resolve_policy();
        assert_eq!(policy.strategy, GateStrategy::Lenient);
    }

    #[test]
    fn gate_config_resolve_policy_with_overrides() {
        let cfg = GateConfig {
            min_overall_score: Some(0.7),
            max_overall_regression: Some(-0.05),
            ..Default::default()
        };
        let policy = cfg.resolve_policy();
        assert!((policy.min_overall_score - 0.7).abs() < 1e-10);
        assert!((policy.max_overall_regression - (-0.05)).abs() < 1e-10);
        // Strategy still balanced (default)
        assert_eq!(policy.strategy, GateStrategy::Balanced);
    }

    #[test]
    fn gate_config_validate_ok() {
        let cfg = GateConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn gate_config_validate_bad_policy() {
        let cfg = GateConfig {
            default_policy: "ultra".into(),
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("unknown gate policy"));
    }

    #[test]
    fn gate_config_validate_min_score_out_of_range() {
        let cfg = GateConfig {
            min_overall_score: Some(1.5),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());

        let cfg2 = GateConfig {
            min_overall_score: Some(-0.1),
            ..Default::default()
        };
        assert!(cfg2.validate().is_err());
    }

    #[test]
    fn gate_config_validate_max_regression_positive() {
        let cfg = GateConfig {
            max_overall_regression: Some(0.1),
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("max_overall_regression must be <= 0.0"));
    }

    #[test]
    fn gate_config_serde_roundtrip() {
        let cfg = GateConfig {
            baseline_path: ".oco/my-baseline.json".into(),
            default_policy: "strict".into(),
            min_overall_score: Some(0.6),
            max_overall_regression: Some(-0.1),
            fresh_days: None,
            stale_days: None,
        };
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let parsed: GateConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn gate_config_serde_defaults_on_missing_fields() {
        // Minimal JSON with only required-ish fields
        let json = r#"{ "baseline_path": "b.json" }"#;
        let cfg: GateConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.baseline_path, "b.json");
        assert_eq!(cfg.default_policy, "balanced");
        assert!(cfg.min_overall_score.is_none());
    }

    #[test]
    fn gate_config_toml_roundtrip() {
        let cfg = GateConfig {
            baseline_path: ".oco/baseline.json".into(),
            default_policy: "balanced".into(),
            min_overall_score: Some(0.5),
            max_overall_regression: None,
            fresh_days: Some(7),
            stale_days: Some(21),
        };
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: GateConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn gate_config_toml_empty_deserializes_to_default() {
        let cfg: GateConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, GateConfig::default());
    }

    // ── Q8: GateConfig freshness fields ──

    #[test]
    fn gate_config_freshness_defaults_none() {
        let cfg = GateConfig::default();
        assert!(cfg.fresh_days.is_none());
        assert!(cfg.stale_days.is_none());
    }

    #[test]
    fn gate_config_validate_fresh_gt_stale_fails() {
        let cfg = GateConfig {
            fresh_days: Some(30),
            stale_days: Some(14),
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("fresh_days"));
        assert!(err.contains("stale_days"));
    }

    #[test]
    fn gate_config_validate_fresh_eq_stale_ok() {
        let cfg = GateConfig {
            fresh_days: Some(14),
            stale_days: Some(14),
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn gate_config_validate_only_fresh_ok() {
        let cfg = GateConfig {
            fresh_days: Some(7),
            stale_days: None,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    // ── Q8: BaselineFreshness ──

    #[test]
    fn freshness_labels() {
        assert_eq!(BaselineFreshness::Fresh.label(), "fresh");
        assert_eq!(BaselineFreshness::Aging.label(), "aging");
        assert_eq!(BaselineFreshness::Stale.label(), "stale");
        assert_eq!(BaselineFreshness::Unknown.label(), "unknown");
    }

    #[test]
    fn freshness_warnings() {
        assert!(!BaselineFreshness::Fresh.warrants_warning());
        assert!(BaselineFreshness::Aging.warrants_warning());
        assert!(BaselineFreshness::Stale.warrants_warning());
        assert!(!BaselineFreshness::Unknown.warrants_warning());
    }

    #[test]
    fn freshness_serde_roundtrip() {
        for f in [
            BaselineFreshness::Fresh,
            BaselineFreshness::Aging,
            BaselineFreshness::Stale,
            BaselineFreshness::Unknown,
        ] {
            let json = serde_json::to_string(&f).unwrap();
            let parsed: BaselineFreshness = serde_json::from_str(&json).unwrap();
            assert_eq!(f, parsed);
        }
    }

    // ── Q8: BaselineFreshnessCheck ──

    #[test]
    fn freshness_check_fresh_baseline() {
        use chrono::Duration;
        let now = Utc::now();
        let created = now - Duration::days(5);
        let check = BaselineFreshnessCheck::evaluate(created, now, None, None);
        assert_eq!(check.freshness, BaselineFreshness::Fresh);
        let days = check.age_days.expect("known age");
        assert!(days > 4.9 && days < 5.1);
        assert_eq!(check.fresh_threshold_days, 14);
        assert_eq!(check.stale_threshold_days, 30);
    }

    #[test]
    fn freshness_check_aging_baseline() {
        use chrono::Duration;
        let now = Utc::now();
        let created = now - Duration::days(20);
        let check = BaselineFreshnessCheck::evaluate(created, now, None, None);
        assert_eq!(check.freshness, BaselineFreshness::Aging);
        assert!(check.recommendation.contains("consider updating"));
    }

    #[test]
    fn freshness_check_stale_baseline() {
        use chrono::Duration;
        let now = Utc::now();
        let created = now - Duration::days(45);
        let check = BaselineFreshnessCheck::evaluate(created, now, None, None);
        assert_eq!(check.freshness, BaselineFreshness::Stale);
        assert!(check.recommendation.contains("stale"));
    }

    #[test]
    fn freshness_check_custom_thresholds() {
        use chrono::Duration;
        let now = Utc::now();
        let created = now - Duration::days(10);
        // Custom: fresh=7, stale=14 — 10 days should be aging
        let check = BaselineFreshnessCheck::evaluate(created, now, Some(7), Some(14));
        assert_eq!(check.freshness, BaselineFreshness::Aging);
        assert_eq!(check.fresh_threshold_days, 7);
        assert_eq!(check.stale_threshold_days, 14);
    }

    #[test]
    fn freshness_check_future_date_unknown() {
        use chrono::Duration;
        let now = Utc::now();
        let future = now + Duration::days(5);
        let check = BaselineFreshnessCheck::evaluate(future, now, None, None);
        assert_eq!(check.freshness, BaselineFreshness::Unknown);
        assert!(
            check.age_days.is_none(),
            "future date should produce None age"
        );
    }

    #[test]
    fn freshness_check_serde_roundtrip() {
        use chrono::Duration;
        let now = Utc::now();
        let created = now - Duration::days(20);
        let check = BaselineFreshnessCheck::evaluate(created, now, None, None);
        let json = serde_json::to_string_pretty(&check).unwrap();
        let parsed: BaselineFreshnessCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(check.freshness, parsed.freshness);
        assert_eq!(check.age_days.is_some(), parsed.age_days.is_some());
        if let (Some(a), Some(b)) = (check.age_days, parsed.age_days) {
            assert!((a - b).abs() < 0.01);
        }
    }

    #[test]
    fn freshness_from_baseline_shortcut() {
        use chrono::Duration;
        let sc = full_scorecard("test", 0.8);
        let mut baseline = EvalBaseline::from_scorecard("v1", sc, "test");
        baseline.created_at = Utc::now() - Duration::days(3);
        let check = BaselineFreshnessCheck::from_baseline(&baseline, None, None);
        assert_eq!(check.freshness, BaselineFreshness::Fresh);
    }

    // ── Q8: GateReviewArtifact ──

    #[test]
    fn review_artifact_generate_pass() {
        use chrono::Duration;
        let baseline_sc = full_scorecard("base", 0.8);
        let candidate_sc = full_scorecard("cand", 0.8);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let mut eval_baseline = EvalBaseline::from_scorecard("v1-stable", baseline_sc, "test");
        eval_baseline.created_at = Utc::now() - Duration::days(5);
        let freshness = BaselineFreshnessCheck::from_baseline(&eval_baseline, None, None);

        let artifact = GateReviewArtifact::generate(gate_result, &eval_baseline, freshness);
        assert_eq!(artifact.summary.verdict, GateVerdict::Pass);
        assert_eq!(artifact.summary.baseline_name, "v1-stable");
        assert_eq!(
            artifact.summary.baseline_freshness,
            BaselineFreshness::Fresh
        );
        assert_eq!(artifact.summary.dimensions_failing, 0);
    }

    #[test]
    fn review_artifact_generate_fail_with_stale_baseline() {
        use chrono::Duration;
        let baseline_sc = full_scorecard("base", 0.9);
        let candidate_sc = full_scorecard("cand", 0.2);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let mut eval_baseline = EvalBaseline::from_scorecard("old-baseline", baseline_sc, "test");
        eval_baseline.created_at = Utc::now() - Duration::days(45);
        let freshness = BaselineFreshnessCheck::from_baseline(&eval_baseline, None, None);

        let artifact = GateReviewArtifact::generate(gate_result, &eval_baseline, freshness);
        assert_eq!(artifact.summary.verdict, GateVerdict::Fail);
        assert_eq!(
            artifact.summary.baseline_freshness,
            BaselineFreshness::Stale
        );
        // Should have recommendations about staleness and failing
        assert!(
            artifact
                .recommendations
                .iter()
                .any(|r| r.contains("stale") || r.contains("Stale"))
        );
        assert!(
            artifact
                .recommendations
                .iter()
                .any(|r| r.contains("FAILED"))
        );
    }

    #[test]
    fn review_artifact_to_markdown() {
        use chrono::Duration;
        let baseline_sc = full_scorecard("base", 0.8);
        let candidate_sc = full_scorecard("cand", 0.7);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let mut eval_baseline = EvalBaseline::from_scorecard("v1", baseline_sc, "test");
        eval_baseline.created_at = Utc::now() - Duration::days(3);
        let freshness = BaselineFreshnessCheck::from_baseline(&eval_baseline, None, None);

        let artifact = GateReviewArtifact::generate(gate_result, &eval_baseline, freshness);
        let md = artifact.to_markdown();

        assert!(md.contains("# Gate Review Report"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("## Dimensions"));
        assert!(md.contains("success"));
        assert!(md.contains("Generated by OCO"));
    }

    #[test]
    fn review_artifact_serde_roundtrip() {
        use chrono::Duration;
        let baseline_sc = full_scorecard("base", 0.8);
        let candidate_sc = full_scorecard("cand", 0.6);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let mut eval_baseline = EvalBaseline::from_scorecard("v1", baseline_sc, "test");
        eval_baseline.created_at = Utc::now() - Duration::days(10);
        let freshness = BaselineFreshnessCheck::from_baseline(&eval_baseline, None, None);

        let artifact = GateReviewArtifact::generate(gate_result, &eval_baseline, freshness);
        let json = artifact.to_json().unwrap();
        let parsed: GateReviewArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(artifact.summary.verdict, parsed.summary.verdict);
        assert_eq!(artifact.recommendations.len(), parsed.recommendations.len());
    }

    #[test]
    fn review_artifact_save_files() {
        use chrono::Duration;
        let baseline_sc = full_scorecard("base", 0.8);
        let candidate_sc = full_scorecard("cand", 0.8);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let mut eval_baseline = EvalBaseline::from_scorecard("v1", baseline_sc, "test");
        eval_baseline.created_at = Utc::now() - Duration::days(2);
        let freshness = BaselineFreshnessCheck::from_baseline(&eval_baseline, None, None);

        let artifact = GateReviewArtifact::generate(gate_result, &eval_baseline, freshness);

        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("report.md");
        let json_path = dir.path().join("report.json");

        artifact.save_markdown(&md_path).unwrap();
        artifact.save_json(&json_path).unwrap();

        let md_content = std::fs::read_to_string(&md_path).unwrap();
        assert!(md_content.contains("Gate Review Report"));

        let json_content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: GateReviewArtifact = serde_json::from_str(&json_content).unwrap();
        assert_eq!(parsed.summary.verdict, GateVerdict::Pass);
    }

    // ── Q8: ReviewSummary ──

    #[test]
    fn review_summary_serde_roundtrip() {
        let summary = ReviewSummary {
            verdict: GateVerdict::Warn,
            baseline_name: "test-baseline".to_string(),
            candidate_id: "test-candidate".to_string(),
            overall_change: "0.80 → 0.75 (−0.05)".to_string(),
            dimensions_passing: 5,
            dimensions_warning: 1,
            dimensions_failing: 1,
            baseline_freshness: BaselineFreshness::Aging,
        };
        let json = serde_json::to_string_pretty(&summary).unwrap();
        let parsed: ReviewSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(summary.verdict, parsed.verdict);
        assert_eq!(summary.baseline_name, parsed.baseline_name);
        assert_eq!(summary.dimensions_warning, parsed.dimensions_warning);
    }

    // ── Q8 consolidation: Unknown freshness ──

    #[test]
    fn freshness_check_unknown_constructor() {
        let check = BaselineFreshnessCheck::unknown();
        assert_eq!(check.freshness, BaselineFreshness::Unknown);
        assert!(check.age_days.is_none());
        assert!(check.recommendation.contains("baseline-save"));
        assert_eq!(
            check.fresh_threshold_days,
            BaselineFreshnessCheck::DEFAULT_FRESH_DAYS
        );
        assert_eq!(
            check.stale_threshold_days,
            BaselineFreshnessCheck::DEFAULT_STALE_DAYS
        );
    }

    #[test]
    fn freshness_unknown_does_not_warrant_warning() {
        assert!(!BaselineFreshness::Unknown.warrants_warning());
    }

    #[test]
    fn generate_with_name_matches_generate() {
        use chrono::Duration;
        let baseline_sc = full_scorecard("base", 0.8);
        let candidate_sc = full_scorecard("cand", 0.8);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let mut eval_baseline = EvalBaseline::from_scorecard("v1-stable", baseline_sc, "test");
        eval_baseline.created_at = Utc::now() - Duration::days(5);
        let freshness = BaselineFreshnessCheck::from_baseline(&eval_baseline, None, None);

        let via_generate =
            GateReviewArtifact::generate(gate_result.clone(), &eval_baseline, freshness.clone());
        let via_name =
            GateReviewArtifact::generate_with_name(gate_result, &eval_baseline.name, freshness);

        assert_eq!(
            via_generate.summary.baseline_name,
            via_name.summary.baseline_name
        );
        assert_eq!(via_generate.summary.verdict, via_name.summary.verdict);
        assert_eq!(
            via_generate.recommendations.len(),
            via_name.recommendations.len()
        );
    }

    #[test]
    fn review_artifact_with_unknown_freshness() {
        let baseline_sc = full_scorecard("base", 0.8);
        let candidate_sc = full_scorecard("cand", 0.8);
        let policy = GatePolicy::default_balanced();
        let gate_result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

        let freshness = BaselineFreshnessCheck::unknown();
        let artifact =
            GateReviewArtifact::generate_with_name(gate_result, "legacy-scorecard", freshness);

        assert_eq!(artifact.summary.baseline_name, "legacy-scorecard");
        assert_eq!(
            artifact.summary.baseline_freshness,
            BaselineFreshness::Unknown
        );
        // Should recommend baseline-save
        assert!(
            artifact
                .recommendations
                .iter()
                .any(|r| r.contains("baseline-save")),
            "expected baseline-save recommendation, got: {:?}",
            artifact.recommendations,
        );
        // Markdown should still render with n/a instead of 0.0
        let md = artifact.to_markdown();
        assert!(md.contains("Gate Review Report"));
        assert!(md.contains("[?]")); // Unknown symbol
        assert!(
            md.contains("n/a"),
            "expected 'n/a' for unknown age, got:\n{md}"
        );
        assert!(
            !md.contains("0.0 days"),
            "must not show '0.0 days' for unknown age"
        );
    }

    #[test]
    fn freshness_check_unknown_serde_roundtrip() {
        let check = BaselineFreshnessCheck::unknown();
        let json = serde_json::to_string_pretty(&check).unwrap();
        assert!(
            json.contains("null"),
            "age_days should be null in JSON: {json}"
        );
        let parsed: BaselineFreshnessCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(check.freshness, parsed.freshness);
        assert!(parsed.age_days.is_none());
        assert!(parsed.recommendation.contains("baseline-save"));
    }

    // ── Q11: Promotion recommendation ──

    #[test]
    fn promotion_recommendation_labels() {
        assert_eq!(PromotionRecommendation::Promote.label(), "promote");
        assert_eq!(PromotionRecommendation::Review.label(), "review");
        assert_eq!(PromotionRecommendation::Reject.label(), "reject");
    }

    #[test]
    fn promotion_recommendation_serde_roundtrip() {
        for r in [
            PromotionRecommendation::Promote,
            PromotionRecommendation::Review,
            PromotionRecommendation::Reject,
        ] {
            let json = serde_json::to_string(&r).unwrap();
            let parsed: PromotionRecommendation = serde_json::from_str(&json).unwrap();
            assert_eq!(r, parsed);
        }
    }

    #[test]
    fn promotion_from_gate_fail_rejects() {
        let rec = PromotionRecommendation::from_gate_and_freshness(
            GateVerdict::Fail,
            BaselineFreshness::Fresh,
        );
        assert_eq!(rec, PromotionRecommendation::Reject);
    }

    #[test]
    fn promotion_from_gate_warn_reviews() {
        let rec = PromotionRecommendation::from_gate_and_freshness(
            GateVerdict::Warn,
            BaselineFreshness::Fresh,
        );
        assert_eq!(rec, PromotionRecommendation::Review);
    }

    #[test]
    fn promotion_from_gate_pass_fresh_promotes() {
        let rec = PromotionRecommendation::from_gate_and_freshness(
            GateVerdict::Pass,
            BaselineFreshness::Fresh,
        );
        assert_eq!(rec, PromotionRecommendation::Promote);
    }

    #[test]
    fn promotion_from_gate_pass_stale_reviews() {
        let rec = PromotionRecommendation::from_gate_and_freshness(
            GateVerdict::Pass,
            BaselineFreshness::Stale,
        );
        assert_eq!(rec, PromotionRecommendation::Review);
    }

    #[test]
    fn promotion_from_gate_pass_aging_promotes() {
        let rec = PromotionRecommendation::from_gate_and_freshness(
            GateVerdict::Pass,
            BaselineFreshness::Aging,
        );
        assert_eq!(rec, PromotionRecommendation::Promote);
    }

    #[test]
    fn promotion_from_gate_pass_unknown_promotes() {
        let rec = PromotionRecommendation::from_gate_and_freshness(
            GateVerdict::Pass,
            BaselineFreshness::Unknown,
        );
        assert_eq!(rec, PromotionRecommendation::Promote);
    }

    // ── Q11: BaselineDiffSummary ──

    #[test]
    fn diff_summary_identical_scorecards() {
        let a = full_scorecard("old", 0.8);
        let b = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&a, &b);
        assert!((diff.overall_delta).abs() < 0.001);
        assert!(diff.summary.contains("unchanged"));
        for d in &diff.dimension_deltas {
            assert!((d.delta).abs() < 0.001);
        }
    }

    #[test]
    fn diff_summary_improved_scorecard() {
        let old = full_scorecard("old", 0.5);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        assert!(diff.overall_delta > 0.0);
        assert!(diff.summary.contains("improved"));
    }

    #[test]
    fn diff_summary_regressed_scorecard() {
        let old = full_scorecard("old", 0.9);
        let new = full_scorecard("new", 0.5);
        let diff = BaselineDiffSummary::compute(&old, &new);
        assert!(diff.overall_delta < 0.0);
        assert!(diff.summary.contains("regressed"));
    }

    #[test]
    fn diff_summary_report_format() {
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let report = diff.to_report();
        assert!(report.contains("Baseline Diff"));
        assert!(report.contains("Overall"));
    }

    #[test]
    fn diff_summary_markdown_format() {
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let md = diff.to_markdown();
        assert!(md.contains("## Baseline Diff"));
        assert!(md.contains("| Dimension |"));
    }

    #[test]
    fn diff_summary_serde_roundtrip() {
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let json = serde_json::to_string_pretty(&diff).unwrap();
        let parsed: BaselineDiffSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(diff.dimension_deltas.len(), parsed.dimension_deltas.len());
        assert!((diff.overall_delta - parsed.overall_delta).abs() < 0.001);
    }

    // ── Q11: PromotionRecord ──

    #[test]
    fn promotion_record_summary() {
        let old = full_scorecard("old-base", 0.7);
        let new = full_scorecard("new-base", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1-stable".to_string(),
            new_baseline_name: "v2-stable".to_string(),
            source: "run:abc123".to_string(),
            reason: Some("CI passed".to_string()),
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: Some(GateVerdict::Pass),
            baseline_freshness: Some(BaselineFreshness::Fresh),
            diff,
        };
        let summary = record.to_summary();
        assert!(summary.contains("[PROMOTE]"));
        assert!(summary.contains("v1-stable"));
        assert!(summary.contains("v2-stable"));
        assert!(summary.contains("CI passed"));
    }

    #[test]
    fn promotion_record_markdown() {
        let old = full_scorecard("old", 0.7);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "file:scorecard.json".to_string(),
            reason: None,
            recommendation: PromotionRecommendation::Review,
            gate_verdict: Some(GateVerdict::Warn),
            baseline_freshness: None,
            diff,
        };
        let md = record.to_markdown();
        assert!(md.contains("### Promotion:"));
        assert!(md.contains("[REVIEW]"));
        assert!(md.contains("[WARN]"));
    }

    #[test]
    fn promotion_record_serde_roundtrip() {
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "old".to_string(),
            new_baseline_name: "new".to_string(),
            source: "test".to_string(),
            reason: Some("testing".to_string()),
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: Some(GateVerdict::Pass),
            baseline_freshness: Some(BaselineFreshness::Fresh),
            diff,
        };
        let json = serde_json::to_string_pretty(&record).unwrap();
        let parsed: PromotionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record.old_baseline_name, parsed.old_baseline_name);
        assert_eq!(record.recommendation, parsed.recommendation);
    }

    // ── Q11: BaselineHistory ──

    #[test]
    fn history_new_is_empty() {
        let h = BaselineHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(h.latest().is_none());
    }

    #[test]
    fn history_append_increments_sequence() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);

        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: None,
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: None,
            baseline_freshness: None,
            diff: diff.clone(),
        };

        let seq1 = h.append(record.clone());
        assert_eq!(seq1, 1);
        assert_eq!(h.len(), 1);

        let mut record2 = record;
        record2.new_baseline_name = "v3".to_string();
        let seq2 = h.append(record2);
        assert_eq!(seq2, 2);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn history_latest_returns_last() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);

        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: None,
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: None,
            baseline_freshness: None,
            diff,
        };
        h.append(record);
        let latest = h.latest().unwrap();
        assert_eq!(latest.promotion.new_baseline_name, "v2");
    }

    #[test]
    fn history_recent_order() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);

        for i in 1..=5 {
            let record = PromotionRecord {
                promoted_at: Utc::now(),
                old_baseline_name: format!("v{}", i - 1),
                new_baseline_name: format!("v{i}"),
                source: "test".to_string(),
                reason: None,
                recommendation: PromotionRecommendation::Promote,
                gate_verdict: None,
                baseline_freshness: None,
                diff: diff.clone(),
            };
            h.append(record);
        }

        let recent = h.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].sequence, 5);
        assert_eq!(recent[1].sequence, 4);
        assert_eq!(recent[2].sequence, 3);
    }

    #[test]
    fn history_serde_roundtrip() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);

        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: Some("release".to_string()),
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: Some(GateVerdict::Pass),
            baseline_freshness: Some(BaselineFreshness::Fresh),
            diff,
        };
        h.append(record);

        let json = serde_json::to_string_pretty(&h).unwrap();
        let parsed: BaselineHistory = serde_json::from_str(&json).unwrap();
        assert_eq!(h.len(), parsed.len());
        assert_eq!(
            h.latest().unwrap().promotion.new_baseline_name,
            parsed.latest().unwrap().promotion.new_baseline_name,
        );
    }

    #[test]
    fn history_save_load_roundtrip() {
        let dir = std::env::temp_dir().join("oco-test-q11-history");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-history.json");

        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);

        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: None,
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: None,
            baseline_freshness: None,
            diff,
        };
        h.append(record);

        h.save_to(&path).unwrap();
        let loaded = BaselineHistory::load_from(&path).unwrap();
        assert_eq!(h.len(), loaded.len());

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn history_load_missing_file_returns_empty() {
        let path = std::path::Path::new("/tmp/oco-test-nonexistent-q11.json");
        let h = BaselineHistory::load_from(path).unwrap();
        assert!(h.is_empty());
    }

    #[test]
    fn history_report_empty() {
        let h = BaselineHistory::new();
        assert!(h.to_report().contains("No baseline promotions"));
    }

    #[test]
    fn history_report_with_entries() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: None,
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: None,
            baseline_freshness: None,
            diff,
        };
        h.append(record);
        let report = h.to_report();
        assert!(report.contains("#1"));
        assert!(report.contains("v1"));
        assert!(report.contains("v2"));
    }

    #[test]
    fn history_markdown_empty() {
        let h = BaselineHistory::new();
        let md = h.to_markdown();
        assert!(md.contains("No promotions recorded"));
    }

    #[test]
    fn history_markdown_with_entries() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.8);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: Some("release".to_string()),
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: None,
            baseline_freshness: None,
            diff,
        };
        h.append(record);
        let md = h.to_markdown();
        assert!(md.contains("# Baseline History"));
        assert!(md.contains("**#1**"));
    }

    #[test]
    fn history_json_output() {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        let diff = BaselineDiffSummary::compute(&old, &new);
        let record = PromotionRecord {
            promoted_at: Utc::now(),
            old_baseline_name: "v1".to_string(),
            new_baseline_name: "v2".to_string(),
            source: "test".to_string(),
            reason: None,
            recommendation: PromotionRecommendation::Promote,
            gate_verdict: None,
            baseline_freshness: None,
            diff,
        };
        h.append(record);
        let json = h.to_json().unwrap();
        assert!(json.contains("schema_version"));
        assert!(json.contains("entries"));
    }

    // ── BaselineHistory::prune ──

    fn make_history(n: usize) -> BaselineHistory {
        let mut h = BaselineHistory::new();
        let old = full_scorecard("old", 0.6);
        let new = full_scorecard("new", 0.7);
        for i in 0..n {
            let diff = BaselineDiffSummary::compute(&old, &new);
            h.append(PromotionRecord {
                promoted_at: Utc::now(),
                old_baseline_name: format!("v{i}"),
                new_baseline_name: format!("v{}", i + 1),
                source: "test".to_string(),
                reason: None,
                recommendation: PromotionRecommendation::Promote,
                gate_verdict: None,
                baseline_freshness: None,
                diff,
            });
        }
        h
    }

    #[test]
    fn prune_keeps_most_recent() {
        let mut h = make_history(5);
        assert_eq!(h.len(), 5);
        let removed = h.prune(3);
        assert_eq!(removed, 2);
        assert_eq!(h.len(), 3);
        // Kept entries should be the last 3 (sequences 3, 4, 5)
        assert_eq!(h.entries[0].sequence, 3);
        assert_eq!(h.entries[2].sequence, 5);
    }

    #[test]
    fn prune_noop_when_fewer_entries() {
        let mut h = make_history(3);
        let removed = h.prune(5);
        assert_eq!(removed, 0);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn prune_noop_when_exact() {
        let mut h = make_history(3);
        let removed = h.prune(3);
        assert_eq!(removed, 0);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn prune_to_zero() {
        let mut h = make_history(5);
        let removed = h.prune(0);
        assert_eq!(removed, 5);
        assert!(h.is_empty());
    }

    #[test]
    fn prune_preview_does_not_modify() {
        let h = make_history(5);
        let preview = h.prune_preview(3);
        assert_eq!(preview.len(), 2);
        assert_eq!(preview[0].sequence, 1);
        assert_eq!(preview[1].sequence, 2);
        // History unchanged
        assert_eq!(h.len(), 5);
    }

    #[test]
    fn prune_preview_empty_when_under_limit() {
        let h = make_history(3);
        let preview = h.prune_preview(5);
        assert!(preview.is_empty());
    }
}
