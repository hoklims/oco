use std::path::Path;

use oco_shared_types::{BaselineFreshnessCheck, Budget, EvalBaseline, GateConfig, RepoProfile};
use serde::{Deserialize, Serialize};

/// Configuration for the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestratorConfig {
    /// HTTP server bind address.
    pub bind_address: String,
    /// HTTP server port.
    pub port: u16,
    /// Default budget for new sessions.
    pub default_budget: Budget,
    /// Maximum concurrent sessions.
    pub max_concurrent_sessions: u32,
    /// LLM provider configuration.
    pub llm: LlmProviderConfig,
    /// ML worker URL (Python service).
    pub ml_worker_url: Option<String>,
    /// SQLite database path.
    pub db_path: String,
    /// Enable decision trace logging.
    pub enable_traces: bool,
    /// Custom system prompt. Falls back to a sensible default when `None`.
    pub system_prompt: Option<String>,
    /// v2: Per-repo profile (stack commands, sensitive paths, risk level).
    #[serde(default)]
    pub profile: RepoProfile,
    /// Override for session max_steps (0 = use session default).
    #[serde(default)]
    pub max_steps: u32,
    /// Q7: Per-repo gate configuration (baseline, policy, thresholds).
    #[serde(default)]
    pub gate: GateConfig,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: 3000,
            default_budget: Budget::default(),
            max_concurrent_sessions: 5,
            llm: LlmProviderConfig::default(),
            ml_worker_url: Some("http://127.0.0.1:50052".into()),
            db_path: "oco.db".into(),
            enable_traces: true,
            system_prompt: None,
            profile: RepoProfile::default(),
            max_steps: 0,
            gate: GateConfig::default(),
        }
    }
}

impl OrchestratorConfig {
    /// Validate semantic constraints beyond TOML parsing.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::ValidationError("port must be > 0".into()));
        }
        if self.max_concurrent_sessions == 0 {
            return Err(ConfigError::ValidationError(
                "max_concurrent_sessions must be > 0".into(),
            ));
        }
        let valid_providers = ["anthropic", "ollama", "stub"];
        if !valid_providers.contains(&self.llm.provider.as_str()) {
            return Err(ConfigError::ValidationError(format!(
                "unknown LLM provider '{}', expected one of: {}",
                self.llm.provider,
                valid_providers.join(", ")
            )));
        }
        if self.llm.timeout_secs == 0 {
            return Err(ConfigError::ValidationError(
                "llm.timeout_secs must be > 0".into(),
            ));
        }
        if self.llm.provider == "anthropic"
            && self.llm.api_key.is_none()
            && self.llm.api_key_env.is_empty()
        {
            return Err(ConfigError::ValidationError(
                "anthropic provider requires api_key or api_key_env".into(),
            ));
        }
        self.gate.validate().map_err(ConfigError::ValidationError)?;
        Ok(())
    }

    /// Load configuration from a TOML file, falling back to defaults for missing fields.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::IoError(path.display().to_string(), e.to_string()))?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(path.display().to_string(), e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Load config from `oco.toml` in the given directory, or use defaults.
    /// v2: Also auto-detects repo profile from manifests and merges config overrides.
    pub fn load_from_dir(dir: &Path) -> Self {
        let config_path = dir.join("oco.toml");
        let mut config = if config_path.exists() {
            match Self::from_file(&config_path) {
                Ok(config) => {
                    tracing::info!(path = %config_path.display(), "Loaded configuration");
                    config
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load config, using defaults");
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        // v2: Auto-detect repo profile from manifests.
        let mut detected = RepoProfile::detect(dir);
        // Merge any profile overrides from config file.
        detected.merge(&config.profile);
        config.profile = detected;
        tracing::info!(
            stack = %config.profile.stack,
            risk = ?config.profile.risk_level,
            "Repo profile loaded"
        );

        config
    }

    /// Generate a default config file as TOML string.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|e| ConfigError::SerializeError(e.to_string()))
    }
}

/// LLM provider configuration — provider-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmProviderConfig {
    /// Provider name (e.g., "anthropic", "openai", "ollama", "custom").
    pub provider: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// API key (read from environment if not set).
    pub api_key: Option<String>,
    /// Environment variable name for API key.
    pub api_key_env: String,
    /// Maximum retries on transient failures.
    pub max_retries: u32,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-4-6".into(),
            api_key: None,
            api_key_env: "ANTHROPIC_API_KEY".into(),
            max_retries: 3,
            timeout_secs: 60,
        }
    }
}

/// Load only the `[gate]` section from an `oco.toml` at the given workspace path.
///
/// Returns `GateConfig::default()` if no config file or no `[gate]` section exists.
pub fn load_gate_config(workspace: &Path) -> GateConfig {
    let config = OrchestratorConfig::load_from_dir(workspace);
    config.gate
}

/// Strict gate config loader: returns an error if `oco.toml` exists but is
/// invalid.  Returns `GateConfig::default()` only when no config file is
/// present (which is a legitimate "use defaults" signal).
///
/// Use this in commands that *depend* on the gate contract (`eval-gate`,
/// `baseline-save`) so that a broken config is never silently swallowed.
pub fn load_gate_config_strict(workspace: &Path) -> Result<GateConfig, ConfigError> {
    let config_path = workspace.join("oco.toml");
    if !config_path.exists() {
        return Ok(GateConfig::default());
    }
    let config = OrchestratorConfig::from_file(&config_path)?;
    Ok(config.gate)
}

/// Build a freshness check from the workspace's gate config and a loaded baseline.
///
/// Uses [`load_gate_config_strict`] — returns an error if `oco.toml` exists but
/// is invalid, consistent with the fail-closed Q7 contract.  Returns defaults
/// when no config file is present.
pub fn evaluate_baseline_freshness(
    workspace: &Path,
    baseline: &EvalBaseline,
) -> Result<BaselineFreshnessCheck, ConfigError> {
    let gate_cfg = load_gate_config_strict(workspace)?;
    Ok(BaselineFreshnessCheck::from_baseline(
        baseline,
        gate_cfg.fresh_days,
        gate_cfg.stale_days,
    ))
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {0}: {1}")]
    IoError(String, String),
    #[error("failed to parse config file {0}: {1}")]
    ParseError(String, String),
    #[error("failed to serialize config: {0}")]
    SerializeError(String),
    #[error("config validation error: {0}")]
    ValidationError(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::GateStrategy;
    use std::io::Write;

    #[test]
    fn default_config_has_default_gate() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.gate.default_policy, "balanced");
        assert_eq!(config.gate.baseline_path, ".oco/baseline.json");
    }

    #[test]
    fn gate_config_from_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
baseline_path = ".oco/my-baseline.json"
default_policy = "strict"
min_overall_score = 0.7
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert_eq!(config.gate.baseline_path, ".oco/my-baseline.json");
        assert_eq!(config.gate.default_policy, "strict");
        assert_eq!(config.gate.min_overall_score, Some(0.7));
        assert!(config.gate.max_overall_regression.is_none());
    }

    #[test]
    fn gate_config_resolves_correct_policy() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "strict"
max_overall_regression = -0.05
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        let policy = config.gate.resolve_policy();
        assert_eq!(policy.strategy, GateStrategy::Strict);
        assert!((policy.max_overall_regression - (-0.05)).abs() < 1e-10);
        // min_overall_score stays at strict default (0.6)
        assert!((policy.min_overall_score - 0.6).abs() < 1e-10);
    }

    #[test]
    fn gate_config_missing_section_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert_eq!(config.gate, GateConfig::default());
    }

    #[test]
    fn invalid_gate_policy_fails_validation() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "mega-strict"
"#
        )
        .unwrap();

        let err = OrchestratorConfig::from_file(&config_path);
        assert!(err.is_err());
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("unknown gate policy"), "got: {msg}");
    }

    #[test]
    fn load_gate_config_from_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "lenient"
baseline_path = "quality/baseline.json"
"#
        )
        .unwrap();

        let gate = load_gate_config(dir.path());
        assert_eq!(gate.default_policy, "lenient");
        assert_eq!(gate.baseline_path, "quality/baseline.json");
    }

    #[test]
    fn load_gate_config_no_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let gate = load_gate_config(dir.path());
        assert_eq!(gate, GateConfig::default());
    }

    // ── load_gate_config_strict ──

    #[test]
    fn strict_no_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let gate = load_gate_config_strict(dir.path()).unwrap();
        assert_eq!(gate, GateConfig::default());
    }

    #[test]
    fn strict_valid_file_returns_gate() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "strict"
baseline_path = "my/baseline.json"
"#
        )
        .unwrap();

        let gate = load_gate_config_strict(dir.path()).unwrap();
        assert_eq!(gate.default_policy, "strict");
        assert_eq!(gate.baseline_path, "my/baseline.json");
    }

    #[test]
    fn strict_invalid_gate_policy_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "ultra-strict"
"#
        )
        .unwrap();

        let result = load_gate_config_strict(dir.path());
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("unknown gate policy"), "got: {msg}");
    }

    #[test]
    fn strict_broken_toml_errors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("oco.toml"), "invalid [[[ toml").unwrap();

        let result = load_gate_config_strict(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn strict_invalid_min_score_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
min_overall_score = 2.0
"#
        )
        .unwrap();

        let result = load_gate_config_strict(dir.path());
        assert!(result.is_err());
    }

    // ── Q8: fresh_days / stale_days pass-through ──

    #[test]
    fn gate_config_freshness_fields_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
fresh_days = 7
stale_days = 21
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert_eq!(config.gate.fresh_days, Some(7));
        assert_eq!(config.gate.stale_days, Some(21));
    }

    #[test]
    fn gate_config_freshness_fields_default_none() {
        let config = OrchestratorConfig::default();
        assert!(config.gate.fresh_days.is_none());
        assert!(config.gate.stale_days.is_none());
    }

    #[test]
    fn gate_config_invalid_freshness_fresh_gt_stale() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
fresh_days = 30
stale_days = 7
"#
        )
        .unwrap();

        let result = OrchestratorConfig::from_file(&config_path);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("fresh_days") && msg.contains("stale_days"),
            "got: {msg}"
        );
    }

    // ── Q8: evaluate_baseline_freshness ──

    #[test]
    fn evaluate_freshness_no_config_uses_defaults() {
        use chrono::Utc;
        use oco_shared_types::{
            BaselineFreshness, BaselineFreshnessCheck, CostMetrics, EvalBaseline, RunScorecard,
        };

        let dir = tempfile::tempdir().unwrap();
        // No oco.toml — defaults apply (fresh_days=14, stale_days=30)

        let scorecard = RunScorecard {
            run_id: "test".to_string(),
            computed_at: Utc::now(),
            dimensions: vec![],
            overall_score: 0.5,
            cost: CostMetrics::default(),
        };
        let baseline = EvalBaseline::from_scorecard("test-baseline", scorecard, "test");

        let check = evaluate_baseline_freshness(dir.path(), &baseline).unwrap();
        assert_eq!(check.freshness, BaselineFreshness::Fresh);
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
    fn evaluate_freshness_with_custom_thresholds() {
        use chrono::{Duration, Utc};
        use oco_shared_types::{BaselineFreshness, CostMetrics, EvalBaseline, RunScorecard};

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
fresh_days = 3
stale_days = 7
"#
        )
        .unwrap();

        // Create a baseline that is 5 days old (between fresh=3 and stale=7 => Aging)
        let scorecard = RunScorecard {
            run_id: "test".to_string(),
            computed_at: Utc::now() - Duration::days(5),
            dimensions: vec![],
            overall_score: 0.5,
            cost: CostMetrics::default(),
        };
        let mut baseline = EvalBaseline::from_scorecard("old-baseline", scorecard, "test");
        baseline.created_at = Utc::now() - Duration::days(5);

        let check = evaluate_baseline_freshness(dir.path(), &baseline).unwrap();
        assert_eq!(check.freshness, BaselineFreshness::Aging);
        assert_eq!(check.fresh_threshold_days, 3);
        assert_eq!(check.stale_threshold_days, 7);
    }

    #[test]
    fn evaluate_freshness_stale_baseline() {
        use chrono::{Duration, Utc};
        use oco_shared_types::{BaselineFreshness, CostMetrics, EvalBaseline, RunScorecard};

        let dir = tempfile::tempdir().unwrap();
        // No config — default thresholds (fresh=14, stale=30)

        // Create a baseline that is 45 days old (> 30 => Stale)
        let scorecard = RunScorecard {
            run_id: "old".to_string(),
            computed_at: Utc::now() - Duration::days(45),
            dimensions: vec![],
            overall_score: 0.5,
            cost: CostMetrics::default(),
        };
        let mut baseline = EvalBaseline::from_scorecard("stale-baseline", scorecard, "test");
        baseline.created_at = Utc::now() - Duration::days(45);

        let check = evaluate_baseline_freshness(dir.path(), &baseline).unwrap();
        assert_eq!(check.freshness, BaselineFreshness::Stale);
        assert!(check.freshness.warrants_warning());
    }

    #[test]
    fn evaluate_freshness_aging_baseline() {
        use chrono::{Duration, Utc};
        use oco_shared_types::{BaselineFreshness, CostMetrics, EvalBaseline, RunScorecard};

        let dir = tempfile::tempdir().unwrap();
        // No config — defaults: fresh=14, stale=30

        // Baseline is 20 days old (between 14 and 30 => Aging)
        let scorecard = RunScorecard {
            run_id: "aging".to_string(),
            computed_at: Utc::now() - Duration::days(20),
            dimensions: vec![],
            overall_score: 0.5,
            cost: CostMetrics::default(),
        };
        let mut baseline = EvalBaseline::from_scorecard("aging-baseline", scorecard, "test");
        baseline.created_at = Utc::now() - Duration::days(20);

        let check = evaluate_baseline_freshness(dir.path(), &baseline).unwrap();
        assert_eq!(check.freshness, BaselineFreshness::Aging);
        assert!(check.freshness.warrants_warning());
    }

    #[test]
    fn evaluate_freshness_invalid_config_errors() {
        use chrono::Utc;
        use oco_shared_types::{CostMetrics, EvalBaseline, RunScorecard};

        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "nonexistent"
"#
        )
        .unwrap();

        let scorecard = RunScorecard {
            run_id: "test".to_string(),
            computed_at: Utc::now(),
            dimensions: vec![],
            overall_score: 0.5,
            cost: CostMetrics::default(),
        };
        let baseline = EvalBaseline::from_scorecard("test", scorecard, "test");

        let result = evaluate_baseline_freshness(dir.path(), &baseline);
        assert!(result.is_err(), "expected error for invalid config");
    }
}
