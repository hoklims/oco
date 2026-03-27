//! Counterfactual verification (#64) — epistemic confidence gating.
//!
//! Before declaring a task complete, OCO asks:
//! - What should have failed if the hypothesis was wrong?
//! - What proof is still missing?
//! - What alternative branches remain plausible?
//!
//! This prevents premature completion and naive self-validation.

use serde::{Deserialize, Serialize};

/// Result of a counterfactual verification check.
/// Produced after implementation + standard verification, before completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterfactualResult {
    /// Overall confidence that the fix/implementation is correct (0.0 to 1.0).
    pub confidence: f64,
    /// What was verified and passed.
    pub evidence_for: Vec<EvidencePoint>,
    /// What proof is still missing.
    pub missing_proof: Vec<String>,
    /// Alternative hypotheses that haven't been ruled out.
    pub remaining_alternatives: Vec<Alternative>,
    /// Recommended action based on the analysis.
    pub recommendation: Recommendation,
}

impl CounterfactualResult {
    /// Whether confidence is high enough to allow completion.
    pub fn allows_completion(&self, threshold: f64) -> bool {
        self.confidence >= threshold && self.missing_proof.is_empty()
    }

    /// Whether the result suggests more investigation is needed.
    pub fn needs_investigation(&self) -> bool {
        !self.remaining_alternatives.is_empty() || !self.missing_proof.is_empty()
    }
}

/// A point of evidence supporting the current conclusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePoint {
    /// What was checked.
    pub check: String,
    /// Whether it passed.
    pub passed: bool,
    /// How this supports the conclusion.
    pub significance: String,
}

impl EvidencePoint {
    pub fn passed(check: impl Into<String>, significance: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            passed: true,
            significance: significance.into(),
        }
    }

    pub fn failed(check: impl Into<String>, significance: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            passed: false,
            significance: significance.into(),
        }
    }
}

/// An alternative explanation that hasn't been ruled out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alternative {
    /// Description of the alternative.
    pub description: String,
    /// How plausible this alternative is (0.0 to 1.0).
    pub plausibility: f64,
    /// What would need to be checked to rule this out.
    pub ruling_out_check: String,
}

impl Alternative {
    pub fn new(
        description: impl Into<String>,
        plausibility: f64,
        ruling_out_check: impl Into<String>,
    ) -> Self {
        Self {
            description: description.into(),
            plausibility: plausibility.clamp(0.0, 1.0),
            ruling_out_check: ruling_out_check.into(),
        }
    }
}

/// What the counterfactual verifier recommends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Recommendation {
    /// Sufficient evidence — safe to complete.
    Proceed,
    /// Investigate specific gaps before completing.
    Investigate { areas: Vec<String> },
    /// Confidence too low — do not complete yet.
    Block { reason: String },
}

// ---------------------------------------------------------------------------
// Deterministic counterfactual analysis
// ---------------------------------------------------------------------------

/// Analyze the current state of a task for counterfactual confidence.
/// Deterministic — no LLM call. Uses verification state, memory, and risk analysis.
pub fn analyze_counterfactual(input: &CounterfactualInput) -> CounterfactualResult {
    let mut evidence_for = Vec::new();
    let mut missing_proof = Vec::new();
    let mut remaining_alternatives = Vec::new();
    let mut confidence: f64 = 0.5; // Start neutral

    // --- Check: verification ran and passed ---
    if input.verification_passed {
        evidence_for.push(EvidencePoint::passed(
            "verification_suite",
            "Build/test/lint passed after changes",
        ));
        confidence += 0.2;
    } else if input.verification_ran {
        evidence_for.push(EvidencePoint::failed(
            "verification_suite",
            "Verification ran but had failures",
        ));
        confidence -= 0.3;
    } else {
        missing_proof.push("No verification has been run after changes".into());
        confidence -= 0.2;
    }

    // --- Check: targeted tests exist for the change ---
    if input.has_targeted_tests {
        evidence_for.push(EvidencePoint::passed(
            "targeted_test",
            "Specific test exercises the changed behavior",
        ));
        confidence += 0.15;
    } else if input.files_modified > 0 {
        missing_proof.push("No targeted test for the specific change".into());
    }

    // --- Check: impact scope vs verification scope ---
    if input.files_modified > 0 && input.files_verified < input.files_modified {
        let unverified = input.files_modified - input.files_verified;
        missing_proof.push(format!(
            "{unverified} modified file(s) not covered by verification"
        ));
        confidence -= 0.1;
    }

    // --- Check: hypotheses status from working memory ---
    if input.active_hypotheses > 1 {
        remaining_alternatives.push(Alternative::new(
            format!("{} hypotheses still active — root cause may not be the one addressed", input.active_hypotheses),
            0.3 * input.active_hypotheses as f64,
            "Invalidate remaining hypotheses with targeted checks",
        ));
        confidence -= 0.1;
    }

    if input.contradicting_evidence > 0 {
        remaining_alternatives.push(Alternative::new(
            "Contradicting evidence exists in working memory",
            0.5,
            "Resolve contradictions before concluding",
        ));
        confidence -= 0.15;
    }

    // --- Check: risk level of the task ---
    if input.risk_score > 0.7 {
        if !input.verification_passed {
            missing_proof.push("High-risk task requires verification before completion".into());
            confidence -= 0.2;
        }
        // Even with verification, high-risk needs extra caution
        if input.risk_score > 0.9 {
            missing_proof.push("Critical-risk task: consider manual review".into());
        }
    }

    // Clamp confidence
    confidence = confidence.clamp(0.0, 1.0);

    // --- Determine recommendation ---
    let recommendation = if confidence >= 0.8 && missing_proof.is_empty() {
        Recommendation::Proceed
    } else if confidence < 0.4 || (!missing_proof.is_empty() && !input.verification_passed) {
        Recommendation::Block {
            reason: if !missing_proof.is_empty() {
                missing_proof[0].clone()
            } else {
                "Confidence too low to proceed".into()
            },
        }
    } else {
        let areas: Vec<String> = missing_proof
            .iter()
            .chain(remaining_alternatives.iter().map(|a| &a.ruling_out_check))
            .take(3)
            .cloned()
            .collect();
        Recommendation::Investigate { areas }
    };

    CounterfactualResult {
        confidence,
        evidence_for,
        missing_proof,
        remaining_alternatives,
        recommendation,
    }
}

/// Input data for counterfactual analysis.
/// Collected from VerificationState, WorkingMemory, and risk analysis.
#[derive(Debug, Clone, Default)]
pub struct CounterfactualInput {
    /// Whether any verification has been run.
    pub verification_ran: bool,
    /// Whether verification passed.
    pub verification_passed: bool,
    /// Whether there's a targeted test for the specific change.
    pub has_targeted_tests: bool,
    /// Number of files modified.
    pub files_modified: usize,
    /// Number of modified files covered by verification.
    pub files_verified: usize,
    /// Number of active hypotheses in working memory.
    pub active_hypotheses: usize,
    /// Number of contradicting evidence entries.
    pub contradicting_evidence: usize,
    /// Risk score from failure preview (0.0 to 1.0).
    pub risk_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_targeted_high_confidence() {
        let input = CounterfactualInput {
            verification_ran: true,
            verification_passed: true,
            has_targeted_tests: true,
            files_modified: 3,
            files_verified: 3,
            active_hypotheses: 0,
            contradicting_evidence: 0,
            risk_score: 0.3,
        };
        let result = analyze_counterfactual(&input);
        assert!(result.confidence >= 0.8);
        assert!(result.missing_proof.is_empty());
        assert_eq!(result.recommendation, Recommendation::Proceed);
        assert!(result.allows_completion(0.7));
    }

    #[test]
    fn no_verification_blocks() {
        let input = CounterfactualInput {
            verification_ran: false,
            verification_passed: false,
            files_modified: 2,
            risk_score: 0.5,
            ..Default::default()
        };
        let result = analyze_counterfactual(&input);
        assert!(result.confidence < 0.5);
        assert!(!result.missing_proof.is_empty());
        assert!(matches!(result.recommendation, Recommendation::Block { .. }));
        assert!(!result.allows_completion(0.5));
    }

    #[test]
    fn failed_verification_low_confidence() {
        let input = CounterfactualInput {
            verification_ran: true,
            verification_passed: false,
            files_modified: 1,
            files_verified: 1,
            risk_score: 0.5,
            ..Default::default()
        };
        let result = analyze_counterfactual(&input);
        assert!(result.confidence < 0.4);
        assert!(matches!(result.recommendation, Recommendation::Block { .. }));
    }

    #[test]
    fn multiple_hypotheses_reduce_confidence() {
        let input = CounterfactualInput {
            verification_ran: true,
            verification_passed: true,
            has_targeted_tests: true,
            files_modified: 1,
            files_verified: 1,
            active_hypotheses: 3,
            contradicting_evidence: 0,
            risk_score: 0.3,
        };
        let result = analyze_counterfactual(&input);
        assert!(!result.remaining_alternatives.is_empty());
        assert!(result.confidence < 0.85); // boosted by verification but reduced by hypotheses
    }

    #[test]
    fn contradicting_evidence_adds_alternative() {
        let input = CounterfactualInput {
            verification_ran: true,
            verification_passed: true,
            files_modified: 1,
            files_verified: 1,
            contradicting_evidence: 2,
            risk_score: 0.4,
            ..Default::default()
        };
        let result = analyze_counterfactual(&input);
        assert!(result.remaining_alternatives.iter().any(|a| a.description.contains("Contradicting")));
        assert!(result.needs_investigation());
    }

    #[test]
    fn high_risk_unverified_blocks() {
        let input = CounterfactualInput {
            verification_ran: false,
            verification_passed: false,
            files_modified: 5,
            risk_score: 0.85,
            ..Default::default()
        };
        let result = analyze_counterfactual(&input);
        assert!(matches!(result.recommendation, Recommendation::Block { .. }));
    }

    #[test]
    fn partial_verification_investigates() {
        let input = CounterfactualInput {
            verification_ran: true,
            verification_passed: true,
            has_targeted_tests: false,
            files_modified: 5,
            files_verified: 2,
            risk_score: 0.5,
            ..Default::default()
        };
        let result = analyze_counterfactual(&input);
        assert!(!result.missing_proof.is_empty());
        // Should recommend investigation, not outright block (verification passed)
        assert!(matches!(result.recommendation, Recommendation::Investigate { .. }));
    }

    #[test]
    fn serialization_roundtrip() {
        let result = CounterfactualResult {
            confidence: 0.62,
            evidence_for: vec![EvidencePoint::passed("typecheck", "types are correct")],
            missing_proof: vec!["no integration test".into()],
            remaining_alternatives: vec![Alternative::new(
                "middleware order issue",
                0.4,
                "trace boot order in production config",
            )],
            recommendation: Recommendation::Investigate {
                areas: vec!["middleware chain".into()],
            },
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: CounterfactualResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.confidence, 0.62);
        assert_eq!(parsed.evidence_for.len(), 1);
        assert_eq!(parsed.remaining_alternatives.len(), 1);
    }

    #[test]
    fn needs_investigation_when_alternatives_exist() {
        let result = CounterfactualResult {
            confidence: 0.7,
            evidence_for: vec![],
            missing_proof: vec![],
            remaining_alternatives: vec![Alternative::new("alt", 0.3, "check")],
            recommendation: Recommendation::Proceed,
        };
        assert!(result.needs_investigation());
    }

    #[test]
    fn allows_completion_respects_threshold() {
        let result = CounterfactualResult {
            confidence: 0.75,
            evidence_for: vec![],
            missing_proof: vec![],
            remaining_alternatives: vec![],
            recommendation: Recommendation::Proceed,
        };
        assert!(result.allows_completion(0.7));
        assert!(!result.allows_completion(0.8));
    }
}
