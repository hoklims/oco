use std::path::Path;

use oco_shared_types::{Budget, RepoProfile};
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
