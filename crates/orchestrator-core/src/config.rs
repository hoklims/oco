use std::path::Path;

use oco_shared_types::{
    BaselineDiffSummary, BaselineFreshness, BaselineFreshnessCheck, BaselineHistory, Budget,
    EvalBaseline, GateConfig, GateResult, PromotionRecommendation, PromotionRecord, RepoProfile,
    ReviewConfig, RunScorecard,
};
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
    /// Q10: Per-repo review packet configuration (format, auto-save, output dir).
    #[serde(default)]
    pub review: ReviewConfig,
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
            review: ReviewConfig::default(),
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
        self.review
            .validate()
            .map_err(ConfigError::ValidationError)?;
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

/// Load the `[review]` section from `oco.toml` at the given workspace path.
///
/// Returns `ReviewConfig::default()` if no config file exists.
/// Returns an error if `oco.toml` exists but is invalid (fail-closed).
pub fn load_review_config_strict(workspace: &Path) -> Result<ReviewConfig, ConfigError> {
    let config_path = workspace.join("oco.toml");
    if !config_path.exists() {
        return Ok(ReviewConfig::default());
    }
    let config = OrchestratorConfig::from_file(&config_path)?;
    Ok(config.review)
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

// ---------------------------------------------------------------------------
// Q11: Baseline promotion operations
// ---------------------------------------------------------------------------

/// Default path for the baseline history file, relative to workspace root.
pub const DEFAULT_HISTORY_PATH: &str = ".oco/baseline-history.json";

/// Promote a candidate scorecard to be the new baseline.
///
/// This function:
/// 1. Loads the current baseline (if it exists) from `gate.baseline_path`.
/// 2. Backs up the current baseline to `<baseline_path>.bak`.
/// 3. Saves the new baseline to `gate.baseline_path`.
/// 4. Appends a [`PromotionRecord`] to the baseline history.
///
/// Returns the promotion record on success.
pub fn promote_baseline(
    workspace: &Path,
    new_scorecard: RunScorecard,
    new_name: String,
    source: String,
    reason: Option<String>,
    description: Option<String>,
) -> Result<PromotionRecord, ConfigError> {
    let gate_cfg = load_gate_config_strict(workspace)?;
    let baseline_path = workspace.join(&gate_cfg.baseline_path);

    // Load old baseline (if any)
    let old_baseline = if baseline_path.exists() {
        Some(
            EvalBaseline::load_from(&baseline_path)
                .map_err(|e| ConfigError::IoError(baseline_path.display().to_string(), e))?,
        )
    } else {
        None
    };

    // Compute diff
    let old_name = old_baseline
        .as_ref()
        .map(|b| b.name.clone())
        .unwrap_or_else(|| "(none)".to_string());

    let diff = if let Some(ref old) = old_baseline {
        BaselineDiffSummary::compute(&old.scorecard, &new_scorecard)
    } else {
        // No old baseline — diff is trivially "everything is new"
        BaselineDiffSummary::compute(&new_scorecard, &new_scorecard)
    };

    // Evaluate freshness + gate verdict for recommendation
    let (gate_verdict, baseline_freshness) = if let Some(ref old) = old_baseline {
        let freshness_check =
            BaselineFreshnessCheck::from_baseline(old, gate_cfg.fresh_days, gate_cfg.stale_days);
        let policy = gate_cfg.resolve_policy();
        let gate_result = GateResult::evaluate(&old.scorecard, &new_scorecard, &policy);
        (Some(gate_result.verdict), Some(freshness_check.freshness))
    } else {
        (None, None)
    };

    let recommendation = match gate_verdict {
        Some(gv) => PromotionRecommendation::from_gate_and_freshness(
            gv,
            baseline_freshness.unwrap_or(BaselineFreshness::Unknown),
        ),
        None => PromotionRecommendation::Promote, // No old baseline = first promotion
    };

    // Build promotion record
    let record = PromotionRecord {
        promoted_at: chrono::Utc::now(),
        old_baseline_name: old_name,
        new_baseline_name: new_name.clone(),
        source,
        reason,
        recommendation,
        gate_verdict,
        baseline_freshness,
        diff,
    };

    // Backup old baseline (atomic-ish: rename then write)
    if baseline_path.exists() {
        let backup_path = baseline_path.with_extension("json.bak");
        std::fs::copy(&baseline_path, &backup_path).map_err(|e| {
            ConfigError::IoError(
                format!("backup to {}", backup_path.display()),
                e.to_string(),
            )
        })?;
    }

    // Ensure parent directory exists
    if let Some(parent) = baseline_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ConfigError::IoError(parent.display().to_string(), e.to_string()))?;
    }

    // Save new baseline
    let mut new_baseline = EvalBaseline::from_scorecard(new_name, new_scorecard, &record.source);
    if let Some(desc) = description {
        new_baseline = new_baseline.with_description(desc);
    }
    new_baseline
        .save_to(&baseline_path)
        .map_err(|e| ConfigError::IoError(baseline_path.display().to_string(), e))?;

    // Append to history
    let history_path = workspace.join(DEFAULT_HISTORY_PATH);
    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ConfigError::IoError(parent.display().to_string(), e.to_string()))?;
    }
    let mut history = BaselineHistory::load_from(&history_path)
        .map_err(|e| ConfigError::IoError(history_path.display().to_string(), e))?;
    history.append(record.clone());
    history
        .save_to(&history_path)
        .map_err(|e| ConfigError::IoError(history_path.display().to_string(), e))?;

    Ok(record)
}

/// Load the baseline history for a workspace.
///
/// Returns an empty history if the file doesn't exist.
pub fn load_baseline_history(workspace: &Path) -> Result<BaselineHistory, ConfigError> {
    let history_path = workspace.join(DEFAULT_HISTORY_PATH);
    BaselineHistory::load_from(&history_path)
        .map_err(|e| ConfigError::IoError(history_path.display().to_string(), e))
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

    // ── Q10: ReviewConfig in OrchestratorConfig ──

    #[test]
    fn default_config_has_default_review() {
        let config = OrchestratorConfig::default();
        assert!(!config.review.auto_save);
        assert_eq!(config.review.default_format, "terminal");
        assert!(config.review.output_dir.is_none());
    }

    #[test]
    fn review_config_from_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[review]
auto_save = true
default_format = "markdown"
output_dir = ".oco/reviews"
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert!(config.review.auto_save);
        assert_eq!(config.review.default_format, "markdown");
        assert_eq!(config.review.output_dir.as_deref(), Some(".oco/reviews"));
    }

    #[test]
    fn review_config_missing_section_uses_defaults() {
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
        assert_eq!(config.review, oco_shared_types::ReviewConfig::default());
    }

    #[test]
    fn review_config_invalid_format_fails_validation() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[review]
default_format = "html"
"#
        )
        .unwrap();

        let err = OrchestratorConfig::from_file(&config_path);
        assert!(err.is_err());
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("unknown review format"), "got: {msg}");
    }

    #[test]
    fn load_review_config_strict_no_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let review = load_review_config_strict(dir.path()).unwrap();
        assert_eq!(review, oco_shared_types::ReviewConfig::default());
    }

    #[test]
    fn load_review_config_strict_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[review]
auto_save = true
default_format = "json"
"#
        )
        .unwrap();

        let review = load_review_config_strict(dir.path()).unwrap();
        assert!(review.auto_save);
        assert_eq!(review.default_format, "json");
    }

    #[test]
    fn load_review_config_strict_invalid_format_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[review]
default_format = "xml"
"#
        )
        .unwrap();

        let result = load_review_config_strict(dir.path());
        assert!(result.is_err());
    }

    // ── Q10: Smoke / integration tests — repo-centric review packet flow ──

    #[test]
    fn review_config_auto_save_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[review]
auto_save = true
default_format = "json"
output_dir = ".oco/reviews"
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert!(config.review.auto_save);
        assert_eq!(config.review.default_format, "json");
        assert_eq!(config.review.output_dir.as_deref(), Some(".oco/reviews"));
    }

    #[test]
    fn review_config_partial_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[review]
auto_save = true
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert!(config.review.auto_save);
        // Missing fields fall back to defaults.
        assert_eq!(config.review.default_format, "terminal");
        assert!(config.review.output_dir.is_none());
    }

    #[test]
    fn review_packet_with_review_config_flow() {
        use chrono::Utc;
        use oco_shared_types::{
            CostMetrics, DimensionScore, MergeReadiness, ReviewPacket, RunScorecard,
            ScorecardDimension,
        };

        // 1. Create a temp workspace with oco.toml containing [review] and [gate].
        let workspace = tempfile::tempdir().unwrap();
        let config_path = workspace.path().join("oco.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "balanced"

[review]
auto_save = true
default_format = "json"
output_dir = ".oco/reviews"
"#
        )
        .unwrap();

        // Load and validate the config.
        let config = OrchestratorConfig::from_file(&config_path).unwrap();
        assert!(config.review.auto_save);
        assert_eq!(config.review.default_format, "json");

        // 2. Create a .oco/runs/test-run/ directory with a scorecard.json.
        let run_dir = workspace.path().join(".oco").join("runs").join("test-run");
        std::fs::create_dir_all(&run_dir).unwrap();

        let dimensions: Vec<DimensionScore> = ScorecardDimension::all()
            .iter()
            .map(|d| DimensionScore {
                dimension: *d,
                score: 0.8,
                detail: "test".to_string(),
            })
            .collect();
        let overall = RunScorecard::compute_overall(&dimensions);
        let scorecard = RunScorecard {
            run_id: "test-run".to_string(),
            computed_at: Utc::now(),
            dimensions,
            overall_score: overall,
            cost: CostMetrics::default(),
        };
        let sc_json = serde_json::to_string_pretty(&scorecard).unwrap();
        std::fs::write(run_dir.join("scorecard.json"), &sc_json).unwrap();

        // 3. Build the review packet using the repo gate config.
        let packet = crate::review_packet::build_review_packet(
            &run_dir,
            "test-run",
            &config.gate,
            workspace.path(),
        )
        .unwrap();

        // 4. Verify the packet is structurally valid.
        assert_eq!(packet.run_id, "test-run");
        assert!(packet.scorecard.is_some());
        let sc = packet.scorecard.as_ref().unwrap();
        assert!((sc.overall_score - overall).abs() < 1e-10);
        // No baseline on disk => no gate result, merge_readiness reflects that.
        assert!(packet.gate_result.is_none());
        // MergeReadiness should not be Ready (no gate evidence).
        assert_ne!(packet.merge_readiness, MergeReadiness::Ready);

        // 5. Simulate auto_save: persist to the configured output_dir.
        let output_dir = workspace
            .path()
            .join(config.review.output_dir.as_deref().unwrap());
        std::fs::create_dir_all(&output_dir).unwrap();

        let json_path = output_dir.join("test-run.json");
        packet.save_to(&json_path).unwrap();
        assert!(json_path.exists());

        // Verify saved JSON is valid and round-trips.
        let loaded_json = std::fs::read_to_string(&json_path).unwrap();
        let loaded: ReviewPacket = serde_json::from_str(&loaded_json).unwrap();
        assert_eq!(loaded.run_id, "test-run");
        assert!(loaded.scorecard.is_some());

        // Also save markdown.
        let md_path = output_dir.join("test-run.md");
        packet.save_markdown(&md_path).unwrap();
        assert!(md_path.exists());

        let md_content = std::fs::read_to_string(&md_path).unwrap();
        assert!(md_content.contains("OCO Review Packet"));
        assert!(md_content.contains("test-run"));
    }

    #[test]
    fn review_config_combined_with_gate() {
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
baseline_path = ".oco/my-baseline.json"
min_overall_score = 0.75
fresh_days = 7
stale_days = 21

[review]
auto_save = true
default_format = "markdown"
output_dir = "reviews"
"#
        )
        .unwrap();

        let config = OrchestratorConfig::from_file(&config_path).unwrap();

        // Gate config assertions.
        assert_eq!(config.gate.default_policy, "strict");
        assert_eq!(config.gate.baseline_path, ".oco/my-baseline.json");
        assert_eq!(config.gate.min_overall_score, Some(0.75));
        assert_eq!(config.gate.fresh_days, Some(7));
        assert_eq!(config.gate.stale_days, Some(21));

        // Review config assertions.
        assert!(config.review.auto_save);
        assert_eq!(config.review.default_format, "markdown");
        assert_eq!(config.review.output_dir.as_deref(), Some("reviews"));

        // Both sections resolve correctly in isolation.
        let policy = config.gate.resolve_policy();
        assert_eq!(policy.strategy, GateStrategy::Strict);
        assert!((policy.min_overall_score - 0.75).abs() < 1e-10);
        assert!(config.review.is_markdown());
        assert!(!config.review.is_json());
    }

    // ── Q10 consolidation: path resolution and error attribution ──

    #[test]
    fn save_dir_relative_resolves_from_workspace() {
        // Proves that `ws.join("reviews")` produces a path inside the workspace,
        // not relative to the process cwd.  This is the contract that the CLI
        // fix (PathBuf → ws.join) depends on.
        let workspace = tempfile::tempdir().unwrap();
        let ws = workspace.path();

        let relative = "reviews";
        let resolved = ws.join(relative);
        assert!(
            resolved.starts_with(ws),
            "resolved path must be inside workspace"
        );
        assert!(resolved.ends_with("reviews"));

        // Absolute paths pass through unchanged.
        let absolute = if cfg!(windows) {
            std::path::PathBuf::from("C:\\absolute\\path")
        } else {
            std::path::PathBuf::from("/absolute/path")
        };
        assert!(absolute.is_absolute());
    }

    #[test]
    fn invalid_review_section_does_not_blame_gate() {
        // A broken [review] section must not surface as a gate config error.
        // Both load_gate_config_strict and load_review_config_strict parse the
        // whole file, so the error message from from_file() must mention the
        // actual problem (the review format), not the gate.
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "balanced"

[review]
default_format = "invalid-format"
"#
        )
        .unwrap();

        // load_gate_config_strict will also fail because from_file validates
        // the whole config.  The error message must mention the actual problem.
        let gate_err = load_gate_config_strict(dir.path()).unwrap_err();
        let gate_msg = format!("{gate_err}");
        assert!(
            gate_msg.contains("review format") || gate_msg.contains("unknown review format"),
            "gate loader error should mention the actual review format problem, got: {gate_msg}"
        );

        // load_review_config_strict should also fail with the same root cause.
        let review_err = load_review_config_strict(dir.path()).unwrap_err();
        let review_msg = format!("{review_err}");
        assert!(
            review_msg.contains("review format") || review_msg.contains("unknown review format"),
            "review loader error should mention the actual review format problem, got: {review_msg}"
        );

        // Now test a broken [gate] section — the error must mention gate, not review.
        let mut f2 = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f2,
            r#"
[llm]
provider = "stub"

[gate]
default_policy = "nonexistent"

[review]
default_format = "json"
"#
        )
        .unwrap();

        let gate_err2 = load_gate_config_strict(dir.path()).unwrap_err();
        let gate_msg2 = format!("{gate_err2}");
        assert!(
            gate_msg2.contains("gate policy") || gate_msg2.contains("unknown gate policy"),
            "error should mention the actual gate problem, got: {gate_msg2}"
        );
    }

    // ── Q11: Promotion operations ──

    fn make_test_scorecard(run_id: &str, base: f64) -> RunScorecard {
        use oco_shared_types::{CostMetrics, DimensionScore, ScorecardDimension};
        let dimensions: Vec<DimensionScore> = ScorecardDimension::all()
            .iter()
            .map(|dim| DimensionScore {
                dimension: *dim,
                score: base,
                detail: "test".to_string(),
            })
            .collect();
        let overall = RunScorecard::compute_overall(&dimensions);
        RunScorecard {
            run_id: run_id.to_string(),
            computed_at: chrono::Utc::now(),
            dimensions,
            overall_score: overall,
            cost: CostMetrics::default(),
        }
    }

    #[test]
    fn promote_baseline_first_time() {
        let dir = tempfile::tempdir().unwrap();
        // Create oco.toml with gate config
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
baseline_path = ".oco/baseline.json"
default_policy = "balanced"
"#
        )
        .unwrap();

        let scorecard = make_test_scorecard("run-001", 0.8);
        let record = promote_baseline(
            dir.path(),
            scorecard,
            "v1-stable".to_string(),
            "run:run-001".to_string(),
            Some("first baseline".to_string()),
            None,
        )
        .unwrap();

        assert_eq!(record.new_baseline_name, "v1-stable");
        assert_eq!(record.old_baseline_name, "(none)");
        assert_eq!(
            record.recommendation,
            oco_shared_types::PromotionRecommendation::Promote
        );

        // Baseline file should exist
        let baseline_path = dir.path().join(".oco/baseline.json");
        assert!(baseline_path.exists());

        // History file should exist with 1 entry
        let history_path = dir.path().join(DEFAULT_HISTORY_PATH);
        assert!(history_path.exists());
        let history = oco_shared_types::BaselineHistory::load_from(&history_path).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history.latest().unwrap().sequence, 1);
    }

    #[test]
    fn promote_baseline_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("oco.toml")).unwrap();
        writeln!(
            f,
            r#"
[llm]
provider = "stub"

[gate]
baseline_path = ".oco/baseline.json"
default_policy = "balanced"
"#
        )
        .unwrap();

        // First promotion
        let sc1 = make_test_scorecard("run-001", 0.7);
        promote_baseline(
            dir.path(),
            sc1,
            "v1".to_string(),
            "run:run-001".to_string(),
            None,
            None,
        )
        .unwrap();

        // Second promotion
        let sc2 = make_test_scorecard("run-002", 0.85);
        let record = promote_baseline(
            dir.path(),
            sc2,
            "v2".to_string(),
            "run:run-002".to_string(),
            Some("improved".to_string()),
            None,
        )
        .unwrap();

        assert_eq!(record.old_baseline_name, "v1");
        assert_eq!(record.new_baseline_name, "v2");
        assert!(record.gate_verdict.is_some());

        // Backup should exist
        let backup_path = dir.path().join(".oco/baseline.json.bak");
        assert!(backup_path.exists());

        // New baseline should be v2
        let baseline = EvalBaseline::load_from(&dir.path().join(".oco/baseline.json")).unwrap();
        assert_eq!(baseline.name, "v2");

        // History should have 2 entries
        let history = load_baseline_history(dir.path()).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history.latest().unwrap().sequence, 2);
    }

    #[test]
    fn load_baseline_history_empty_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let history = load_baseline_history(dir.path()).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn promote_baseline_no_config_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        // No oco.toml — should use default gate config
        let scorecard = make_test_scorecard("run-001", 0.8);
        let record = promote_baseline(
            dir.path(),
            scorecard,
            "v1".to_string(),
            "test".to_string(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(record.new_baseline_name, "v1");
        // Default baseline_path
        assert!(dir.path().join(".oco/baseline.json").exists());
    }
}
