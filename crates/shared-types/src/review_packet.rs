//! Q9: Unified review packet — merge-readiness bundle for a single run.
//!
//! A [`ReviewPacket`] aggregates all per-run artifacts into one reviewable
//! document. It **references** existing types (scorecard, gate result, mission
//! memory, freshness) without duplicating their logic.
//!
//! A reviewer (human or CI) reads one packet and answers:
//! - What changed?
//! - What was verified?
//! - What is the trust verdict?
//! - What is the gate verdict?
//! - Is the baseline credible?
//! - What risks remain open?
//! - Is this run merge-ready?

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    BaselineFreshness, BaselineFreshnessCheck, GateResult, GateVerdict, MissionMemory,
    RunScorecard, TrustVerdict,
};

// ---------------------------------------------------------------------------
// Merge readiness
// ---------------------------------------------------------------------------

/// Final merge-readiness assessment for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeReadiness {
    /// All checks passed, no open risks, gate passed — safe to merge.
    Ready,
    /// Minor concerns remain (warnings, aging baseline) — merge with review.
    ConditionallyReady,
    /// Significant issues — do not merge without resolution.
    NotReady,
    /// Insufficient data to determine readiness.
    Unknown,
}

impl MergeReadiness {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ConditionallyReady => "conditionally_ready",
            Self::NotReady => "not_ready",
            Self::Unknown => "unknown",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Ready => "[READY]",
            Self::ConditionallyReady => "[CONDITIONAL]",
            Self::NotReady => "[NOT READY]",
            Self::Unknown => "[?]",
        }
    }
}

// ---------------------------------------------------------------------------
// Review packet sections — thin wrappers over existing data
// ---------------------------------------------------------------------------

/// Summary of what changed during the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesSummary {
    /// Files modified during the run.
    pub modified_files: Vec<String>,
    /// Key decisions made (from mission memory, if available).
    pub key_decisions: Vec<String>,
    /// Narrative summary (from mission memory, if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative: Option<String>,
}

/// Verification and trust summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub trust_verdict: TrustVerdict,
    /// Checks that passed.
    pub checks_passed: Vec<String>,
    /// Checks that failed.
    pub checks_failed: Vec<String>,
    /// Files modified but not verified.
    pub unverified_files: Vec<String>,
}

/// Open risks and residual concerns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRisks {
    /// Risks identified during the run (from mission memory).
    pub risks: Vec<String>,
    /// Open questions (from mission memory).
    pub open_questions: Vec<String>,
    /// Data that was unavailable when building the packet.
    pub unavailable_data: Vec<String>,
}

// ---------------------------------------------------------------------------
// ReviewPacket — the unified artifact
// ---------------------------------------------------------------------------

/// Unified review packet for a single run.
///
/// Aggregates scorecard, gate result, mission memory, and baseline freshness
/// into a single reviewable document. Each field is optional because not every
/// run produces every artifact (e.g., a quick run may not have a gate result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPacket {
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// When this packet was generated.
    pub generated_at: DateTime<Utc>,
    /// Run/session identifier.
    pub run_id: String,

    // -- Core verdicts (always present when derivable) --
    /// Final merge-readiness verdict.
    pub merge_readiness: MergeReadiness,
    /// Trust verdict (from verification / mission memory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_verdict: Option<TrustVerdict>,
    /// Gate verdict (from eval-gate, if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_verdict: Option<GateVerdict>,

    // -- Detailed sections --
    /// What changed during the run.
    pub changes: ChangesSummary,
    /// Verification and trust details.
    pub verification: VerificationSummary,
    /// Open risks and residual concerns.
    pub open_risks: OpenRisks,

    // -- Full artifacts (optional, for drill-down) --
    /// The run's scorecard, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorecard: Option<RunScorecard>,
    /// The gate evaluation result, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_result: Option<GateResult>,
    /// Baseline freshness check, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_freshness: Option<BaselineFreshnessCheck>,
}

/// Current review packet schema version.
pub const REVIEW_PACKET_SCHEMA_VERSION: u32 = 1;

impl ReviewPacket {
    /// Compute merge readiness from the available data.
    ///
    /// Rules:
    /// - If gate verdict is Fail → NotReady
    /// - If trust verdict is None → NotReady
    /// - If gate verdict is Warn OR baseline is Aging/Stale OR trust is Low → ConditionallyReady
    /// - If gate verdict is Pass AND trust is High/Medium AND baseline is Fresh → Ready
    /// - If insufficient data → Unknown
    pub fn compute_merge_readiness(
        trust: Option<TrustVerdict>,
        gate: Option<GateVerdict>,
        freshness: Option<BaselineFreshness>,
        has_open_risks: bool,
    ) -> MergeReadiness {
        // Hard blockers
        if gate == Some(GateVerdict::Fail) {
            return MergeReadiness::NotReady;
        }
        if trust == Some(TrustVerdict::None) {
            return MergeReadiness::NotReady;
        }

        // Insufficient data — we need at least trust to judge
        if trust.is_none() {
            return MergeReadiness::Unknown;
        }

        // Conditional signals
        let gate_warns = gate == Some(GateVerdict::Warn);
        let baseline_aging = matches!(
            freshness,
            Some(BaselineFreshness::Aging) | Some(BaselineFreshness::Stale)
        );
        let trust_low = trust == Some(TrustVerdict::Low);

        if gate_warns || baseline_aging || trust_low || has_open_risks {
            return MergeReadiness::ConditionallyReady;
        }

        // If gate is available and passed, or no gate but trust is good
        if matches!(trust, Some(TrustVerdict::High) | Some(TrustVerdict::Medium)) {
            return MergeReadiness::Ready;
        }

        MergeReadiness::Unknown
    }

    /// Build a review packet from its component parts.
    ///
    /// This is the primary constructor. The `ReviewPacketBuilder` in
    /// `orchestrator-core` provides a higher-level API that loads artifacts
    /// from disk.
    pub fn build(
        run_id: String,
        scorecard: Option<RunScorecard>,
        gate_result: Option<GateResult>,
        mission: Option<&MissionMemory>,
        baseline_freshness: Option<BaselineFreshnessCheck>,
    ) -> Self {
        // Extract trust verdict from multiple sources (prefer mission, fall back to scorecard dim)
        let trust_verdict = mission
            .map(|m| m.trust_verdict)
            .or_else(|| {
                scorecard.as_ref().and_then(|sc| {
                    sc.dimension_score(crate::ScorecardDimension::TrustVerdict)
                        .map(trust_from_score)
                })
            });

        let gate_verdict = gate_result.as_ref().map(|gr| gr.verdict);
        let freshness_classification = baseline_freshness.as_ref().map(|bf| bf.freshness);

        // Build changes summary
        let changes = ChangesSummary {
            modified_files: mission
                .map(|m| m.modified_files.clone())
                .unwrap_or_default(),
            key_decisions: mission
                .map(|m| m.key_decisions.clone())
                .unwrap_or_default(),
            narrative: mission
                .filter(|m| !m.narrative.is_empty())
                .map(|m| m.narrative.clone()),
        };

        // Build verification summary
        let verification = if let Some(m) = mission {
            VerificationSummary {
                trust_verdict: m.trust_verdict,
                checks_passed: m.verification.checks_passed.clone(),
                checks_failed: m.verification.checks_failed.clone(),
                unverified_files: m.verification.unverified_files.clone(),
            }
        } else {
            VerificationSummary {
                trust_verdict: trust_verdict.unwrap_or(TrustVerdict::None),
                checks_passed: Vec::new(),
                checks_failed: Vec::new(),
                unverified_files: Vec::new(),
            }
        };

        // Build open risks
        let mut unavailable_data = Vec::new();
        if mission.is_none() {
            unavailable_data.push("mission memory not available".to_string());
        }
        if scorecard.is_none() {
            unavailable_data.push("scorecard not available".to_string());
        }
        if gate_result.is_none() {
            unavailable_data.push("gate result not available".to_string());
        }
        if baseline_freshness.is_none() {
            unavailable_data.push("baseline freshness not evaluated".to_string());
        }

        let open_risks = OpenRisks {
            risks: mission.map(|m| m.risks.clone()).unwrap_or_default(),
            open_questions: mission
                .map(|m| m.open_questions.clone())
                .unwrap_or_default(),
            unavailable_data,
        };

        let has_open_risks = !open_risks.risks.is_empty();

        let merge_readiness = Self::compute_merge_readiness(
            trust_verdict,
            gate_verdict,
            freshness_classification,
            has_open_risks,
        );

        Self {
            schema_version: REVIEW_PACKET_SCHEMA_VERSION,
            generated_at: Utc::now(),
            run_id,
            merge_readiness,
            trust_verdict,
            gate_verdict,
            changes,
            verification,
            open_risks,
            scorecard,
            gate_result,
            baseline_freshness,
        }
    }

    /// Render as a human-readable review document.
    pub fn to_review_text(&self) -> String {
        let mut sections = Vec::new();

        // Header
        sections.push(format!(
            "OCO Review Packet\n\
             ====================\n\
             Run: {}\n\
             Generated: {}\n\
             Merge Readiness: {} {}",
            self.run_id,
            self.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
            self.merge_readiness.symbol(),
            self.merge_readiness.label(),
        ));

        // Verdicts block
        {
            let mut verdict_lines = Vec::new();
            if let Some(tv) = self.trust_verdict {
                verdict_lines.push(format!("  Trust: {}", tv.label()));
            }
            if let Some(gv) = self.gate_verdict {
                verdict_lines.push(format!("  Gate: {} {}", gv.symbol(), gv.label()));
            }
            if let Some(ref bf) = self.baseline_freshness {
                verdict_lines.push(format!(
                    "  Baseline: {} {}",
                    bf.freshness.symbol(),
                    bf.freshness.label()
                ));
                if let Some(age) = bf.age_days {
                    verdict_lines.push(format!("  Baseline age: {age:.1} days"));
                }
            }
            if !verdict_lines.is_empty() {
                sections.push(format!("VERDICTS:\n{}", verdict_lines.join("\n")));
            }
        }

        // Scorecard summary
        if let Some(ref sc) = self.scorecard {
            let dim_lines: Vec<String> = sc
                .dimensions
                .iter()
                .map(|d| format!("  {:<24} {:.2}", d.dimension.label(), d.score))
                .collect();
            sections.push(format!(
                "SCORECARD (overall: {:.2}):\n{}",
                sc.overall_score,
                dim_lines.join("\n"),
            ));
        }

        // Changes
        if !self.changes.modified_files.is_empty() {
            let items: Vec<String> = self
                .changes
                .modified_files
                .iter()
                .map(|f| format!("  - {f}"))
                .collect();
            sections.push(format!(
                "CHANGES ({} files):\n{}",
                self.changes.modified_files.len(),
                items.join("\n"),
            ));
        }

        if !self.changes.key_decisions.is_empty() {
            let items: Vec<String> = self
                .changes
                .key_decisions
                .iter()
                .map(|d| format!("  - {d}"))
                .collect();
            sections.push(format!("KEY DECISIONS:\n{}", items.join("\n")));
        }

        if let Some(ref narrative) = self.changes.narrative {
            sections.push(format!("NARRATIVE:\n  {narrative}"));
        }

        // Verification
        {
            let mut ver_lines = Vec::new();
            ver_lines.push(format!(
                "  Trust: {}",
                self.verification.trust_verdict.label()
            ));
            if !self.verification.checks_passed.is_empty() {
                ver_lines.push(format!(
                    "  Passed: {}",
                    self.verification.checks_passed.join(", ")
                ));
            }
            if !self.verification.checks_failed.is_empty() {
                ver_lines.push(format!(
                    "  Failed: {}",
                    self.verification.checks_failed.join(", ")
                ));
            }
            if !self.verification.unverified_files.is_empty() {
                for f in &self.verification.unverified_files {
                    ver_lines.push(format!("  ! {f}"));
                }
            }
            sections.push(format!("VERIFICATION:\n{}", ver_lines.join("\n")));
        }

        // Open risks
        if !self.open_risks.risks.is_empty()
            || !self.open_risks.open_questions.is_empty()
            || !self.open_risks.unavailable_data.is_empty()
        {
            let mut risk_lines = Vec::new();
            for r in &self.open_risks.risks {
                risk_lines.push(format!("  ! {r}"));
            }
            for q in &self.open_risks.open_questions {
                risk_lines.push(format!("  ? {q}"));
            }
            for u in &self.open_risks.unavailable_data {
                risk_lines.push(format!("  ~ {u}"));
            }
            sections.push(format!("OPEN RISKS:\n{}", risk_lines.join("\n")));
        }

        // Gate detail (if present)
        if let Some(ref gr) = self.gate_result {
            if !gr.reasons.is_empty() {
                let items: Vec<String> =
                    gr.reasons.iter().map(|r| format!("  - {r}")).collect();
                sections.push(format!("GATE REASONS:\n{}", items.join("\n")));
            }
        }

        sections.join("\n\n")
    }

    /// Render as a Markdown review document.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# OCO Review Packet\n\n");
        md.push_str(&format!(
            "**Merge Readiness:** {} {}\n\n",
            self.merge_readiness.symbol(),
            self.merge_readiness.label(),
        ));

        // Summary table
        md.push_str("## Summary\n\n");
        md.push_str("| Field | Value |\n");
        md.push_str("|-------|-------|\n");
        md.push_str(&format!("| Run | {} |\n", self.run_id));
        md.push_str(&format!(
            "| Generated | {} |\n",
            self.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
        ));
        if let Some(tv) = self.trust_verdict {
            md.push_str(&format!("| Trust | {} |\n", tv.label()));
        }
        if let Some(gv) = self.gate_verdict {
            md.push_str(&format!("| Gate | {} {} |\n", gv.symbol(), gv.label()));
        }
        if let Some(ref bf) = self.baseline_freshness {
            md.push_str(&format!(
                "| Baseline | {} {} |\n",
                bf.freshness.symbol(),
                bf.freshness.label(),
            ));
        }
        if let Some(ref sc) = self.scorecard {
            md.push_str(&format!("| Overall score | {:.2} |\n", sc.overall_score));
        }

        // Scorecard dimensions
        if let Some(ref sc) = self.scorecard {
            md.push_str("\n## Scorecard\n\n");
            md.push_str("| Dimension | Score |\n");
            md.push_str("|-----------|-------|\n");
            for d in &sc.dimensions {
                md.push_str(&format!("| {} | {:.2} |\n", d.dimension.label(), d.score));
            }
        }

        // Changes
        if !self.changes.modified_files.is_empty() {
            md.push_str(&format!(
                "\n## Changes ({} files)\n\n",
                self.changes.modified_files.len()
            ));
            for f in &self.changes.modified_files {
                md.push_str(&format!("- `{f}`\n"));
            }
        }

        if !self.changes.key_decisions.is_empty() {
            md.push_str("\n## Key Decisions\n\n");
            for d in &self.changes.key_decisions {
                md.push_str(&format!("- {d}\n"));
            }
        }

        if let Some(ref narrative) = self.changes.narrative {
            md.push_str(&format!("\n## Narrative\n\n{narrative}\n"));
        }

        // Verification
        md.push_str("\n## Verification\n\n");
        md.push_str(&format!(
            "**Trust verdict:** {}\n\n",
            self.verification.trust_verdict.label()
        ));
        if !self.verification.checks_passed.is_empty() {
            md.push_str(&format!(
                "**Passed:** {}\n\n",
                self.verification.checks_passed.join(", ")
            ));
        }
        if !self.verification.checks_failed.is_empty() {
            md.push_str(&format!(
                "**Failed:** {}\n\n",
                self.verification.checks_failed.join(", ")
            ));
        }
        if !self.verification.unverified_files.is_empty() {
            md.push_str("**Unverified files:**\n\n");
            for f in &self.verification.unverified_files {
                md.push_str(&format!("- `{f}`\n"));
            }
        }

        // Open risks
        if !self.open_risks.risks.is_empty()
            || !self.open_risks.open_questions.is_empty()
            || !self.open_risks.unavailable_data.is_empty()
        {
            md.push_str("\n## Open Risks\n\n");
            for r in &self.open_risks.risks {
                md.push_str(&format!("- :warning: {r}\n"));
            }
            for q in &self.open_risks.open_questions {
                md.push_str(&format!("- :question: {q}\n"));
            }
            for u in &self.open_risks.unavailable_data {
                md.push_str(&format!("- :grey_question: {u}\n"));
            }
        }

        // Gate detail
        if let Some(ref gr) = self.gate_result {
            if !gr.reasons.is_empty() {
                md.push_str("\n## Gate Reasons\n\n");
                for r in &gr.reasons {
                    md.push_str(&format!("- {r}\n"));
                }
            }
        }

        md.push_str(&format!(
            "\n---\n*Generated by OCO review-pack at {}*\n",
            self.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
        ));

        md
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize review packet: {e}"))
    }

    /// Persist to a JSON file.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directories: {e}"))?;
        }
        let json = self.to_json()?;
        std::fs::write(path, json).map_err(|e| format!("failed to write review packet: {e}"))
    }

    /// Save Markdown to a file.
    pub fn save_markdown(&self, path: &std::path::Path) -> Result<(), String> {
        let md = self.to_markdown();
        std::fs::write(path, md)
            .map_err(|e| format!("failed to write markdown review packet: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a TrustVerdict dimension score (0.0–1.0) back to a TrustVerdict enum.
fn trust_from_score(score: f64) -> TrustVerdict {
    if score >= 0.9 {
        TrustVerdict::High
    } else if score >= 0.5 {
        TrustVerdict::Medium
    } else if score > 0.0 {
        TrustVerdict::Low
    } else {
        TrustVerdict::None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CostMetrics, DimensionScore, GatePolicy, ScorecardDimension,
    };

    fn sample_scorecard() -> RunScorecard {
        let dimensions: Vec<DimensionScore> = ScorecardDimension::all()
            .iter()
            .map(|d| DimensionScore {
                dimension: *d,
                score: 0.8,
                detail: "test".to_string(),
            })
            .collect();
        let overall = RunScorecard::compute_overall(&dimensions);
        RunScorecard {
            run_id: "test-run".to_string(),
            computed_at: Utc::now(),
            dimensions,
            overall_score: overall,
            cost: CostMetrics::default(),
        }
    }

    fn sample_mission() -> MissionMemory {
        MissionMemory {
            schema_version: crate::mission::MISSION_SCHEMA_VERSION,
            session_id: crate::SessionId::new(),
            created_at: Utc::now(),
            mission: "fix the auth bug".to_string(),
            facts: vec![crate::MissionFact {
                content: "JWT is used".to_string(),
                source: Some("src/auth.rs".to_string()),
                established_at: Utc::now(),
            }],
            hypotheses: vec![],
            open_questions: vec!["does it support refresh tokens?".to_string()],
            plan: crate::MissionPlan::default(),
            verification: crate::MissionVerificationStatus {
                freshness: crate::VerificationFreshness::Fresh,
                unverified_files: vec![],
                last_check: Some(Utc::now()),
                checks_passed: vec!["build".to_string(), "test".to_string()],
                checks_failed: vec![],
            },
            modified_files: vec!["src/auth.rs".to_string()],
            key_decisions: vec!["chose direct fix".to_string()],
            risks: vec![],
            trust_verdict: TrustVerdict::High,
            narrative: "Auth bug was a missing HttpOnly flag".to_string(),
        }
    }

    fn sample_gate_result() -> GateResult {
        let baseline = sample_scorecard();
        let candidate = sample_scorecard();
        let policy = GatePolicy::default_balanced();
        GateResult::evaluate(&baseline, &candidate, &policy)
    }

    // ── MergeReadiness ──

    #[test]
    fn merge_readiness_labels() {
        assert_eq!(MergeReadiness::Ready.label(), "ready");
        assert_eq!(MergeReadiness::ConditionallyReady.label(), "conditionally_ready");
        assert_eq!(MergeReadiness::NotReady.label(), "not_ready");
        assert_eq!(MergeReadiness::Unknown.label(), "unknown");
    }

    #[test]
    fn merge_readiness_serde_roundtrip() {
        for mr in [
            MergeReadiness::Ready,
            MergeReadiness::ConditionallyReady,
            MergeReadiness::NotReady,
            MergeReadiness::Unknown,
        ] {
            let json = serde_json::to_string(&mr).unwrap();
            let parsed: MergeReadiness = serde_json::from_str(&json).unwrap();
            assert_eq!(mr, parsed);
        }
    }

    // ── compute_merge_readiness ──

    #[test]
    fn readiness_gate_fail_is_not_ready() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                Some(GateVerdict::Fail),
                Some(BaselineFreshness::Fresh),
                false,
            ),
            MergeReadiness::NotReady,
        );
    }

    #[test]
    fn readiness_trust_none_is_not_ready() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::None),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Fresh),
                false,
            ),
            MergeReadiness::NotReady,
        );
    }

    #[test]
    fn readiness_gate_warn_is_conditional() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                Some(GateVerdict::Warn),
                Some(BaselineFreshness::Fresh),
                false,
            ),
            MergeReadiness::ConditionallyReady,
        );
    }

    #[test]
    fn readiness_stale_baseline_is_conditional() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Stale),
                false,
            ),
            MergeReadiness::ConditionallyReady,
        );
    }

    #[test]
    fn readiness_aging_baseline_is_conditional() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Aging),
                false,
            ),
            MergeReadiness::ConditionallyReady,
        );
    }

    #[test]
    fn readiness_open_risks_is_conditional() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Fresh),
                true,
            ),
            MergeReadiness::ConditionallyReady,
        );
    }

    #[test]
    fn readiness_trust_low_is_conditional() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::Low),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Fresh),
                false,
            ),
            MergeReadiness::ConditionallyReady,
        );
    }

    #[test]
    fn readiness_all_good_is_ready() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Fresh),
                false,
            ),
            MergeReadiness::Ready,
        );
    }

    #[test]
    fn readiness_medium_trust_is_ready() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::Medium),
                Some(GateVerdict::Pass),
                Some(BaselineFreshness::Fresh),
                false,
            ),
            MergeReadiness::Ready,
        );
    }

    #[test]
    fn readiness_no_trust_is_unknown() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(None, None, None, false),
            MergeReadiness::Unknown,
        );
    }

    #[test]
    fn readiness_no_gate_but_good_trust_is_ready() {
        assert_eq!(
            ReviewPacket::compute_merge_readiness(
                Some(TrustVerdict::High),
                None,
                None,
                false,
            ),
            MergeReadiness::Ready,
        );
    }

    // ── ReviewPacket::build ──

    #[test]
    fn build_full_packet() {
        let sc = sample_scorecard();
        let mission = sample_mission();
        let gate = sample_gate_result();
        let freshness = BaselineFreshnessCheck::evaluate(
            Utc::now(),
            Utc::now(),
            None,
            None,
        );

        let packet = ReviewPacket::build(
            "test-run".to_string(),
            Some(sc),
            Some(gate),
            Some(&mission),
            Some(freshness),
        );

        assert_eq!(packet.run_id, "test-run");
        assert_eq!(packet.trust_verdict, Some(TrustVerdict::High));
        assert_eq!(packet.gate_verdict, Some(GateVerdict::Pass));
        assert_eq!(packet.merge_readiness, MergeReadiness::Ready);
        assert_eq!(packet.changes.modified_files.len(), 1);
        assert_eq!(packet.changes.key_decisions.len(), 1);
        assert!(packet.changes.narrative.is_some());
        assert_eq!(packet.verification.checks_passed.len(), 2);
        assert!(packet.open_risks.risks.is_empty());
        assert_eq!(packet.open_risks.open_questions.len(), 1);
        assert!(packet.open_risks.unavailable_data.is_empty());
        assert_eq!(packet.schema_version, REVIEW_PACKET_SCHEMA_VERSION);
    }

    #[test]
    fn build_minimal_packet() {
        let packet = ReviewPacket::build(
            "minimal".to_string(),
            None,
            None,
            None,
            None,
        );

        assert_eq!(packet.merge_readiness, MergeReadiness::Unknown);
        assert!(packet.trust_verdict.is_none());
        assert!(packet.gate_verdict.is_none());
        assert!(packet.scorecard.is_none());
        assert!(packet.gate_result.is_none());
        assert!(packet.baseline_freshness.is_none());
        assert_eq!(packet.open_risks.unavailable_data.len(), 4);
    }

    #[test]
    fn build_scorecard_only() {
        let sc = sample_scorecard();
        let packet = ReviewPacket::build(
            "sc-only".to_string(),
            Some(sc),
            None,
            None,
            None,
        );

        // Trust derived from scorecard dimension score (0.8 → Medium)
        assert_eq!(packet.trust_verdict, Some(TrustVerdict::Medium));
        assert!(packet.scorecard.is_some());
        assert_eq!(packet.open_risks.unavailable_data.len(), 3);
    }

    // ── Serde roundtrip ──

    #[test]
    fn serde_roundtrip_full() {
        let sc = sample_scorecard();
        let mission = sample_mission();
        let gate = sample_gate_result();
        let freshness = BaselineFreshnessCheck::evaluate(Utc::now(), Utc::now(), None, None);

        let packet = ReviewPacket::build(
            "roundtrip".to_string(),
            Some(sc),
            Some(gate),
            Some(&mission),
            Some(freshness),
        );

        let json = serde_json::to_string_pretty(&packet).unwrap();
        let restored: ReviewPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(packet.run_id, restored.run_id);
        assert_eq!(packet.merge_readiness, restored.merge_readiness);
        assert_eq!(packet.trust_verdict, restored.trust_verdict);
        assert_eq!(packet.gate_verdict, restored.gate_verdict);
    }

    #[test]
    fn serde_roundtrip_minimal() {
        let packet = ReviewPacket::build("min".to_string(), None, None, None, None);
        let json = serde_json::to_string(&packet).unwrap();
        let restored: ReviewPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(packet.merge_readiness, restored.merge_readiness);
    }

    // ── Rendering ���─

    #[test]
    fn review_text_contains_key_info() {
        let mission = sample_mission();
        let packet = ReviewPacket::build(
            "render-test".to_string(),
            Some(sample_scorecard()),
            Some(sample_gate_result()),
            Some(&mission),
            None,
        );

        let text = packet.to_review_text();
        assert!(text.contains("OCO Review Packet"));
        assert!(text.contains("render-test"));
        assert!(text.contains("Merge Readiness:"));
        assert!(text.contains("VERDICTS:"));
        assert!(text.contains("SCORECARD"));
        assert!(text.contains("CHANGES"));
        assert!(text.contains("VERIFICATION:"));
    }

    #[test]
    fn markdown_contains_key_sections() {
        let mission = sample_mission();
        let packet = ReviewPacket::build(
            "md-test".to_string(),
            Some(sample_scorecard()),
            Some(sample_gate_result()),
            Some(&mission),
            None,
        );

        let md = packet.to_markdown();
        assert!(md.contains("# OCO Review Packet"));
        assert!(md.contains("**Merge Readiness:**"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("## Scorecard"));
        assert!(md.contains("## Verification"));
    }

    #[test]
    fn to_json_produces_valid_json() {
        let packet = ReviewPacket::build("json-test".to_string(), None, None, None, None);
        let json_str = packet.to_json().unwrap();
        let _: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    }

    // ── Persistence ──

    #[test]
    fn persistence_roundtrip() {
        let packet = ReviewPacket::build(
            "persist-test".to_string(),
            Some(sample_scorecard()),
            None,
            None,
            None,
        );
        let dir = std::env::temp_dir().join("oco-test-review-packet");
        let path = dir.join("review-packet.json");

        packet.save_to(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let restored: ReviewPacket = serde_json::from_str(&content).unwrap();
        assert_eq!(packet.run_id, restored.run_id);
        assert_eq!(packet.merge_readiness, restored.merge_readiness);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── trust_from_score ──

    #[test]
    fn trust_from_score_mapping() {
        assert_eq!(trust_from_score(1.0), TrustVerdict::High);
        assert_eq!(trust_from_score(0.9), TrustVerdict::High);
        assert_eq!(trust_from_score(0.66), TrustVerdict::Medium);
        assert_eq!(trust_from_score(0.5), TrustVerdict::Medium);
        assert_eq!(trust_from_score(0.33), TrustVerdict::Low);
        assert_eq!(trust_from_score(0.1), TrustVerdict::Low);
        assert_eq!(trust_from_score(0.0), TrustVerdict::None);
    }
}
