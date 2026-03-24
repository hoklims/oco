use serde::{Deserialize, Serialize};

/// A replay scenario for evaluation purposes.
///
/// Scenarios define a user request, workspace, expected outcomes,
/// and configuration overrides for comparative testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayScenario {
    /// Unique scenario name.
    pub name: String,
    /// Description of what this scenario tests.
    pub description: String,
    /// The user request to replay.
    pub user_request: String,
    /// Workspace root path (relative to scenario file).
    pub workspace: String,
    /// Expected action sequence (for validation, not strict matching).
    #[serde(default)]
    pub expected_actions: Vec<String>,
    /// Configuration overrides for this scenario.
    #[serde(default)]
    pub config_overrides: ScenarioConfig,
    /// Tags for filtering scenarios.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Configuration overrides for a replay scenario.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScenarioConfig {
    /// LLM provider override ("stub", "anthropic", "ollama").
    pub llm_provider: Option<String>,
    /// Max steps override.
    pub max_steps: Option<u32>,
    /// Verification strictness: "strict", "relaxed", "none".
    pub verify_mode: Option<String>,
    /// Whether to enable subagents.
    pub subagents: Option<bool>,
    /// Max token budget override.
    pub max_total_tokens: Option<u64>,
}

/// Result of running a replay scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// Scenario name.
    pub scenario_name: String,
    /// Whether the scenario completed successfully.
    pub success: bool,
    /// Total steps taken.
    pub step_count: u32,
    /// Total tokens consumed.
    pub total_tokens: u64,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
    /// Whether verification passed (if applicable).
    pub verification_passed: Option<bool>,
    /// Actions taken (as type strings).
    pub actions: Vec<String>,
    /// Errors encountered.
    pub errors: Vec<String>,
    /// Whether the expected action sequence was matched.
    pub expected_match: bool,
}

impl ScenarioResult {
    /// Compute evaluation metrics for comparison.
    pub fn metrics(&self) -> EvaluationMetrics {
        EvaluationMetrics {
            scenario_name: self.scenario_name.clone(),
            success: self.success,
            step_count: self.step_count,
            total_tokens: self.total_tokens,
            duration_ms: self.duration_ms,
            verification_passed: self.verification_passed,
            token_per_step: if self.step_count > 0 {
                self.total_tokens as f64 / self.step_count as f64
            } else {
                0.0
            },
            error_rate: if self.step_count > 0 {
                self.errors.len() as f64 / self.step_count as f64
            } else {
                0.0
            },
        }
    }
}

/// Comparison-ready evaluation metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationMetrics {
    pub scenario_name: String,
    pub success: bool,
    pub step_count: u32,
    pub total_tokens: u64,
    pub duration_ms: u64,
    pub verification_passed: Option<bool>,
    pub token_per_step: f64,
    pub error_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_result_metrics() {
        let result = ScenarioResult {
            scenario_name: "test_scenario".into(),
            success: true,
            step_count: 5,
            total_tokens: 10000,
            duration_ms: 5000,
            verification_passed: Some(true),
            actions: vec!["retrieve".into(), "respond".into()],
            errors: vec![],
            expected_match: true,
        };

        let metrics = result.metrics();
        assert_eq!(metrics.token_per_step, 2000.0);
        assert_eq!(metrics.error_rate, 0.0);
        assert!(metrics.success);
    }

    #[test]
    fn scenario_config_defaults() {
        let config = ScenarioConfig::default();
        assert!(config.llm_provider.is_none());
        assert!(config.max_steps.is_none());
        assert!(config.verify_mode.is_none());
    }
}
