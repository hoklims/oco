//! Decision affordances (#66) — shape the agent's decision space.
//!
//! Instead of returning raw data, OCO returns what is *rational to do next*
//! and what is *blocked*. This guides Claude Code without constraining it.
//!
//! Combined with compact response format (#68), affordances make OCO
//! the path of least resistance — not the most virtuous path.

use serde::{Deserialize, Serialize};

/// Affordances returned alongside tool results.
/// Shapes what the agent perceives as possible next actions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecisionAffordance {
    /// Actions that are rational to take next, ranked by priority.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub best_next_actions: Vec<SuggestedAction>,
    /// Actions that are explicitly blocked and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_actions: Vec<BlockedAction>,
    /// Current completion status.
    #[serde(default)]
    pub completion_status: CompletionStatus,
}

impl DecisionAffordance {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn suggest(mut self, action: SuggestedAction) -> Self {
        self.best_next_actions.push(action);
        self
    }

    pub fn block(mut self, action: BlockedAction) -> Self {
        self.blocked_actions.push(action);
        self
    }

    pub fn with_status(mut self, status: CompletionStatus) -> Self {
        self.completion_status = status;
        self
    }

    /// Whether completion is currently blocked.
    pub fn is_completion_blocked(&self) -> bool {
        self.blocked_actions
            .iter()
            .any(|a| a.action == "declare_complete")
    }
}

/// A suggested next action with rationale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    /// Machine-readable action identifier.
    pub action: String,
    /// Why this action is recommended now.
    pub reason: String,
    /// Optional: specific tool to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Optional: arguments for the tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

impl SuggestedAction {
    pub fn new(action: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            reason: reason.into(),
            tool: None,
            args: None,
        }
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.args = Some(args);
        self
    }
}

/// An action that is explicitly blocked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedAction {
    /// Machine-readable action identifier.
    pub action: String,
    /// Why this action is blocked.
    pub reason: String,
}

impl BlockedAction {
    pub fn new(action: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            reason: reason.into(),
        }
    }

    /// Block completion because verification hasn't passed.
    pub fn block_completion_unverified() -> Self {
        Self::new(
            "declare_complete",
            "verification contract not satisfied",
        )
    }

    /// Block completion because required outputs are missing.
    pub fn block_completion_missing_output(field: &str) -> Self {
        Self::new(
            "declare_complete",
            format!("required output missing: {field}"),
        )
    }
}

/// Whether the task can be declared complete.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    /// Not yet determinable.
    #[default]
    InProgress,
    /// All requirements met — safe to complete.
    Ready,
    /// Blocked by unmet requirements.
    Blocked,
    /// Partially done — some outputs present but not all.
    Partial,
}

// ---------------------------------------------------------------------------
// Compact response envelope (#68)
// ---------------------------------------------------------------------------

/// Compact response format for all OCO MCP tools.
/// Target: < 500 tokens per response. Directly consumable by Claude Code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResponse {
    /// One-line summary of what happened.
    pub summary: String,
    /// Structured evidence (compact, no prose).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<serde_json::Value>,
    /// Risks identified.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    /// Decision affordances: what to do next, what's blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affordances: Option<DecisionAffordance>,
    /// Confidence score (0.0 to 1.0).
    #[serde(default)]
    pub confidence: f64,
}

impl CompactResponse {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            evidence: Vec::new(),
            risks: Vec::new(),
            affordances: None,
            confidence: 0.5,
        }
    }

    pub fn with_evidence(mut self, evidence: serde_json::Value) -> Self {
        self.evidence.push(evidence);
        self
    }

    pub fn with_risk(mut self, risk: impl Into<String>) -> Self {
        self.risks.push(risk.into());
        self
    }

    pub fn with_affordances(mut self, affordances: DecisionAffordance) -> Self {
        self.affordances = Some(affordances);
        self
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Estimated token count of this response when serialized.
    pub fn estimated_tokens(&self) -> usize {
        // Rough heuristic: 1 token ≈ 4 characters in JSON
        let json = serde_json::to_string(self).unwrap_or_default();
        json.len() / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affordance_builder() {
        let aff = DecisionAffordance::new()
            .suggest(SuggestedAction::new(
                "trace_symbol_flow",
                "patch touches shared contract used by 4 callers",
            ))
            .suggest(
                SuggestedAction::new(
                    "run_targeted_verify",
                    "implementation step completed",
                )
                .with_tool("oco.verify_patch"),
            )
            .block(BlockedAction::block_completion_unverified())
            .with_status(CompletionStatus::Blocked);

        assert_eq!(aff.best_next_actions.len(), 2);
        assert_eq!(aff.blocked_actions.len(), 1);
        assert!(aff.is_completion_blocked());
        assert_eq!(aff.completion_status, CompletionStatus::Blocked);
    }

    #[test]
    fn affordance_not_blocked_when_verified() {
        let aff = DecisionAffordance::new()
            .suggest(SuggestedAction::new("proceed", "all clear"))
            .with_status(CompletionStatus::Ready);

        assert!(!aff.is_completion_blocked());
    }

    #[test]
    fn compact_response_builder() {
        let resp = CompactResponse::new("Found 3 callers of AuthMiddleware.handle")
            .with_evidence(serde_json::json!({"callers": ["api/routes.rs", "web/app.rs", "admin/setup.rs"]}))
            .with_risk("Modifying signature will break all 3 callers")
            .with_affordances(
                DecisionAffordance::new()
                    .suggest(SuggestedAction::new("impact_scan", "check all callers before modifying"))
                    .block(BlockedAction::block_completion_unverified()),
            )
            .with_confidence(0.85);

        assert_eq!(resp.evidence.len(), 1);
        assert_eq!(resp.risks.len(), 1);
        assert!(resp.affordances.is_some());
        assert_eq!(resp.confidence, 0.85);
    }

    #[test]
    fn compact_response_stays_under_500_tokens() {
        let resp = CompactResponse::new("Verification PASS: 3 passed, 0 failed")
            .with_evidence(serde_json::json!({"verdict": "PASS", "checks": {"build": "pass", "test": "pass", "lint": "pass"}}))
            .with_affordances(
                DecisionAffordance::new()
                    .suggest(SuggestedAction::new("proceed", "all checks passed"))
                    .with_status(CompletionStatus::Ready),
            )
            .with_confidence(1.0);

        assert!(resp.estimated_tokens() < 500);
    }

    #[test]
    fn serialization_roundtrip() {
        let aff = DecisionAffordance::new()
            .suggest(SuggestedAction::new("investigate", "need more context")
                .with_tool("oco.search_codebase")
                .with_args(serde_json::json!({"query": "AuthMiddleware"})))
            .block(BlockedAction::new("complete", "not verified"));

        let json = serde_json::to_string(&aff).unwrap();
        let parsed: DecisionAffordance = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.best_next_actions.len(), 1);
        assert_eq!(parsed.blocked_actions.len(), 1);
        assert_eq!(parsed.best_next_actions[0].tool.as_deref(), Some("oco.search_codebase"));
    }

    #[test]
    fn blocked_action_helpers() {
        let unverified = BlockedAction::block_completion_unverified();
        assert_eq!(unverified.action, "declare_complete");

        let missing = BlockedAction::block_completion_missing_output("patch_summary");
        assert!(missing.reason.contains("patch_summary"));
    }
}
