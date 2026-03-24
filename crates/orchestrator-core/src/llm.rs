use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;
use tracing::{debug, warn};

use crate::error::OrchestratorError;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Provider-agnostic LLM interface.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a completion request and get a response.
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, OrchestratorError>;

    /// Get the provider name.
    fn provider_name(&self) -> &str;

    /// Get the model name.
    fn model_name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub max_tokens: u32,
    pub temperature: f64,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub model: String,
    pub stop_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// StubLlmProvider (testing / development)
// ---------------------------------------------------------------------------

/// Stub LLM provider for testing and development.
pub struct StubLlmProvider {
    pub model: String,
}

#[async_trait]
impl LlmProvider for StubLlmProvider {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
        let last_message = request
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(LlmResponse {
            content: format!(
                "[Stub response to: {}]",
                &last_message[..last_message.len().min(100)]
            ),
            input_tokens: 100,
            output_tokens: 50,
            model: self.model.clone(),
            stop_reason: Some("end_turn".into()),
        })
    }

    fn provider_name(&self) -> &str {
        "stub"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

// ---------------------------------------------------------------------------
// AnthropicProvider
// ---------------------------------------------------------------------------

/// Configuration for the Anthropic provider.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// API key. Read from `api_key_env` at construction time.
    api_key: String,
    /// Model identifier (e.g. "claude-sonnet-4-20250514").
    pub model: String,
    /// Base URL for the API. Defaults to `https://api.anthropic.com`.
    pub base_url: String,
    /// Anthropic-Version header value.
    pub api_version: String,
    /// Request timeout.
    pub timeout: Duration,
}

impl AnthropicConfig {
    /// Create a new config, reading the API key from the given env var.
    ///
    /// Returns `Err` if the env var is missing or empty.
    pub fn from_env(
        model: impl Into<String>,
        api_key_env: Option<&str>,
    ) -> Result<Self, OrchestratorError> {
        let env_var = api_key_env.unwrap_or("ANTHROPIC_API_KEY");
        let api_key = env::var(env_var).map_err(|_| {
            OrchestratorError::ConfigError(format!(
                "missing environment variable `{env_var}` for Anthropic API key"
            ))
        })?;
        if api_key.is_empty() {
            return Err(OrchestratorError::ConfigError(format!(
                "environment variable `{env_var}` is empty"
            )));
        }
        Ok(Self {
            api_key,
            model: model.into(),
            base_url: "https://api.anthropic.com".into(),
            api_version: "2023-06-01".into(),
            timeout: Duration::from_secs(120),
        })
    }

    /// Override the base URL (useful for proxies / custom endpoints).
    #[must_use]
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the request timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Production Anthropic Messages API provider.
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Result<Self, OrchestratorError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                OrchestratorError::LlmError(format!("failed to build HTTP client: {e}"))
            })?;
        Ok(Self { config, client })
    }
}

// -- Anthropic wire types (private) -----------------------------------------

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    model: String,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicErrorEnvelope {
    error: AnthropicErrorBody,
}

#[derive(Deserialize)]
struct AnthropicErrorBody {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
        let url = format!("{}/v1/messages", self.config.base_url.trim_end_matches('/'));

        // Filter out system messages — Anthropic uses a top-level `system` field.
        let messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .filter(|m| m.role != LlmRole::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    LlmRole::User => "user".into(),
                    LlmRole::Assistant => "assistant".into(),
                    LlmRole::System => unreachable!(), // filtered above
                },
                content: m.content.clone(),
            })
            .collect();

        // Merge explicit system_prompt with any System-role messages.
        let system_parts: Vec<&str> = request
            .system_prompt
            .as_deref()
            .into_iter()
            .chain(
                request
                    .messages
                    .iter()
                    .filter(|m| m.role == LlmRole::System)
                    .map(|m| m.content.as_str()),
            )
            .collect();
        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        let body = AnthropicRequest {
            model: self.config.model.clone(),
            max_tokens: request.max_tokens,
            system,
            messages,
            temperature: Some(request.temperature),
        };

        debug!(
            provider = "anthropic",
            model = %self.config.model,
            url = %url,
            message_count = body.messages.len(),
            "sending completion request"
        );

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.api_version)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    OrchestratorError::LlmError("anthropic request timed out".into())
                } else if e.is_connect() {
                    OrchestratorError::LlmError(format!(
                        "failed to connect to Anthropic API at {url}: {e}"
                    ))
                } else {
                    OrchestratorError::LlmError(format!("anthropic request failed: {e}"))
                }
            })?;

        let status = response.status();

        if !status.is_success() {
            let raw = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<AnthropicErrorEnvelope>(&raw)
                .map(|e| format!("{}: {}", e.error.error_type, e.error.message))
                .unwrap_or(raw);

            let err_msg = match status.as_u16() {
                401 => format!("anthropic authentication failed: {detail}"),
                403 => format!("anthropic permission denied: {detail}"),
                429 => format!("anthropic rate limit exceeded: {detail}"),
                500..=599 => format!("anthropic server error ({status}): {detail}"),
                _ => format!("anthropic API error ({status}): {detail}"),
            };
            warn!(status = %status, "anthropic API returned error");
            return Err(OrchestratorError::LlmError(err_msg));
        }

        let api_resp: AnthropicResponse = response.json().await.map_err(|e| {
            OrchestratorError::LlmError(format!("failed to parse Anthropic response: {e}"))
        })?;

        let content = api_resp
            .content
            .first()
            .map(|b| b.text.clone())
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
            input_tokens: api_resp.usage.input_tokens,
            output_tokens: api_resp.usage.output_tokens,
            model: api_resp.model,
            stop_reason: api_resp.stop_reason,
        })
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

// ---------------------------------------------------------------------------
// OllamaProvider (local-first, no API key)
// ---------------------------------------------------------------------------

/// Configuration for an Ollama-compatible local provider.
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    /// Model name as known by the Ollama server (e.g. "llama3", "mistral").
    pub model: String,
    /// Base URL of the Ollama server. Defaults to `http://localhost:11434`.
    pub base_url: String,
    /// Request timeout.
    pub timeout: Duration,
}

impl OllamaConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            base_url: "http://localhost:11434".into(),
            timeout: Duration::from_secs(300), // local models can be slow
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Ollama chat-completion provider for local-first usage.
pub struct OllamaProvider {
    config: OllamaConfig,
    client: Client,
}

impl OllamaProvider {
    pub fn new(config: OllamaConfig) -> Result<Self, OrchestratorError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                OrchestratorError::LlmError(format!("failed to build HTTP client: {e}"))
            })?;
        Ok(Self { config, client })
    }
}

// -- Ollama wire types (private) --------------------------------------------

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
    model: String,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct OllamaErrorResponse {
    #[serde(default)]
    error: String,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
        let url = format!("{}/api/chat", self.config.base_url.trim_end_matches('/'));

        // Build the messages list. Ollama natively supports a "system" role.
        let mut messages: Vec<OllamaChatMessage> = Vec::new();

        // Prepend system prompt if provided.
        if let Some(ref sys) = request.system_prompt {
            messages.push(OllamaChatMessage {
                role: "system".into(),
                content: sys.clone(),
            });
        }

        for m in &request.messages {
            messages.push(OllamaChatMessage {
                role: match m.role {
                    LlmRole::System => "system".into(),
                    LlmRole::User => "user".into(),
                    LlmRole::Assistant => "assistant".into(),
                },
                content: m.content.clone(),
            });
        }

        let body = OllamaChatRequest {
            model: self.config.model.clone(),
            messages,
            stream: false,
            options: OllamaOptions {
                temperature: Some(request.temperature),
                num_predict: Some(request.max_tokens),
            },
        };

        debug!(
            provider = "ollama",
            model = %self.config.model,
            url = %url,
            "sending chat request"
        );

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    OrchestratorError::LlmError("ollama request timed out".into())
                } else if e.is_connect() {
                    OrchestratorError::LlmError(format!(
                        "failed to connect to Ollama at {url} — is the server running? {e}"
                    ))
                } else {
                    OrchestratorError::LlmError(format!("ollama request failed: {e}"))
                }
            })?;

        let status = response.status();

        if !status.is_success() {
            let raw = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<OllamaErrorResponse>(&raw)
                .map(|e| e.error)
                .unwrap_or(raw);

            let err_msg = match status.as_u16() {
                404 => format!(
                    "ollama model `{}` not found — try `ollama pull {}`",
                    self.config.model, self.config.model
                ),
                _ => format!("ollama API error ({status}): {detail}"),
            };
            warn!(status = %status, "ollama API returned error");
            return Err(OrchestratorError::LlmError(err_msg));
        }

        let api_resp: OllamaChatResponse = response.json().await.map_err(|e| {
            OrchestratorError::LlmError(format!("failed to parse Ollama response: {e}"))
        })?;

        Ok(LlmResponse {
            content: api_resp.message.content,
            input_tokens: api_resp.prompt_eval_count.unwrap_or(0),
            output_tokens: api_resp.eval_count.unwrap_or(0),
            model: api_resp.model,
            stop_reason: api_resp.done_reason,
        })
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_provider_returns_response() {
        let provider = StubLlmProvider {
            model: "test-model".into(),
        };
        let req = LlmRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: "hello".into(),
            }],
            max_tokens: 100,
            temperature: 0.0,
            system_prompt: None,
        };
        let resp = provider.complete(req).await.unwrap();
        assert!(resp.content.contains("hello"));
        assert_eq!(provider.provider_name(), "stub");
        assert_eq!(provider.model_name(), "test-model");
    }

    #[test]
    fn anthropic_config_missing_env_returns_error() {
        // Use an env var name that is guaranteed to not exist.
        let result = AnthropicConfig::from_env(
            "claude-sonnet-4-20250514",
            Some("__NONEXISTENT_KEY_FOR_TEST__"),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("__NONEXISTENT_KEY_FOR_TEST__"));
    }

    #[test]
    fn anthropic_config_with_overrides() {
        // Temporarily set the env var for this test.
        // SAFETY: This test is single-threaded and the var is removed immediately after.
        unsafe { env::set_var("__TEST_ANTHROPIC_KEY__", "sk-test-key") };
        let config =
            AnthropicConfig::from_env("claude-sonnet-4-20250514", Some("__TEST_ANTHROPIC_KEY__"))
                .unwrap()
                .with_base_url("https://custom.proxy.example.com")
                .with_timeout(Duration::from_secs(30));
        assert_eq!(config.base_url, "https://custom.proxy.example.com");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        unsafe { env::remove_var("__TEST_ANTHROPIC_KEY__") };
    }

    #[test]
    fn ollama_config_defaults() {
        let config = OllamaConfig::new("llama3");
        assert_eq!(config.base_url, "http://localhost:11434");
        assert_eq!(config.model, "llama3");
    }

    #[test]
    fn ollama_config_with_overrides() {
        let config = OllamaConfig::new("mistral")
            .with_base_url("http://gpu-server:11434")
            .with_timeout(Duration::from_secs(600));
        assert_eq!(config.base_url, "http://gpu-server:11434");
        assert_eq!(config.timeout, Duration::from_secs(600));
    }

    #[test]
    fn llm_role_serialization() {
        let user = serde_json::to_string(&LlmRole::User).unwrap();
        assert_eq!(user, "\"user\"");
        let assistant = serde_json::to_string(&LlmRole::Assistant).unwrap();
        assert_eq!(assistant, "\"assistant\"");
        let system = serde_json::to_string(&LlmRole::System).unwrap();
        assert_eq!(system, "\"system\"");
    }
}
