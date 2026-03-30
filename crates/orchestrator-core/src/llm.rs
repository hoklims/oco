use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

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
    /// Per-request effort override from the router. Takes priority over
    /// provider-level config in `ClaudeCodeConfig::effort`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_override: Option<oco_shared_types::EffortLevel>,
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
            // Extract Retry-After header before consuming the body.
            let retry_after_ms = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(|secs| secs * 1000)
                .unwrap_or(0);

            let raw = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<AnthropicErrorEnvelope>(&raw)
                .map(|e| format!("{}: {}", e.error.error_type, e.error.message))
                .unwrap_or(raw);

            if status.as_u16() == 429 {
                warn!(retry_after_ms, "anthropic rate limit hit");
                return Err(OrchestratorError::RateLimited {
                    retry_after_ms: if retry_after_ms > 0 {
                        retry_after_ms
                    } else {
                        1000
                    },
                    message: format!("anthropic rate limit exceeded: {detail}"),
                });
            }

            let err_msg = match status.as_u16() {
                401 => format!("anthropic authentication failed: {detail}"),
                403 => format!("anthropic permission denied: {detail}"),
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
// ClaudeCodeProvider (delegates to Claude Code CLI)
// ---------------------------------------------------------------------------

/// Configuration for the Claude Code CLI provider.
#[derive(Debug, Clone)]
pub struct ClaudeCodeConfig {
    /// Model alias passed to `claude --model` (e.g. "sonnet", "opus", "haiku").
    pub model: String,
    /// Effort level passed to `claude --effort` (low/medium/high).
    pub effort: Option<oco_shared_types::EffortLevel>,
    /// Request timeout — aligned with Claude Code's 300s streaming limit.
    pub timeout: Duration,
    /// Idle watchdog timeout in ms (env: `CLAUDE_STREAM_IDLE_TIMEOUT_MS`).
    /// Set higher for build/test steps that produce intermittent output.
    pub idle_timeout_ms: Option<u64>,
    /// Scrub credentials from subprocess environment (env: `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`).
    pub scrub_env: bool,
}

impl ClaudeCodeConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            effort: None,
            timeout: Duration::from_secs(300),
            idle_timeout_ms: None,
            scrub_env: true, // Secure by default
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_effort(mut self, effort: oco_shared_types::EffortLevel) -> Self {
        self.effort = Some(effort);
        self
    }

    #[must_use]
    pub fn with_idle_timeout_ms(mut self, ms: u64) -> Self {
        self.idle_timeout_ms = Some(ms);
        self
    }
}

/// LLM provider that delegates to the Claude Code CLI (`claude --bare -p`).
///
/// This is the recommended default when OCO runs as a Claude Code plugin.
/// It uses the user's existing Claude Code authentication and configuration.
pub struct ClaudeCodeProvider {
    config: ClaudeCodeConfig,
}

impl ClaudeCodeProvider {
    pub fn new(config: ClaudeCodeConfig) -> Self {
        Self { config }
    }
}

/// JSON response from `claude -p --output-format json`.
#[derive(Deserialize)]
struct ClaudeCodeResponse {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    usage: Option<ClaudeCodeUsage>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    duration_api_ms: Option<u64>,
    #[serde(default)]
    subtype: Option<String>,
}

#[derive(Deserialize, Default)]
struct ClaudeCodeUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
        use tokio::process::Command;

        // Build the prompt from messages
        let prompt = request
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let mut cmd = Command::new("claude");
        cmd.args([
            "--bare", // Skip hooks/LSP/plugins — 14% faster startup
            "-p",
            &prompt,
            "--output-format",
            "json",
            "--model",
            &self.config.model,
            "--no-session-persistence",
        ]);

        // Per-request effort (from LlmRouter) takes priority over config default.
        let effective_effort = request.effort_override.or(self.config.effort);
        if let Some(effort) = effective_effort {
            cmd.args(["--effort", effort.as_flag()]);
        }

        if let Some(ref sys) = request.system_prompt {
            cmd.args(["--append-system-prompt", sys]);
        }

        // Security: scrub credentials from subprocesses
        if self.config.scrub_env {
            cmd.env("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB", "1");
        } else {
            // Remove any inherited value from parent process
            cmd.env_remove("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB");
        }

        // Override idle watchdog for long-running steps (builds, tests)
        if let Some(idle_ms) = self.config.idle_timeout_ms {
            cmd.env("CLAUDE_STREAM_IDLE_TIMEOUT_MS", idle_ms.to_string());
        } else {
            // Remove any inherited value from parent process
            cmd.env_remove("CLAUDE_STREAM_IDLE_TIMEOUT_MS");
        }

        debug!(
            provider = "claude-code",
            model = %self.config.model,
            effort = ?self.config.effort,
            prompt_len = prompt.len(),
            "sending request via claude CLI"
        );

        let output = tokio::time::timeout(self.config.timeout, cmd.output())
            .await
            .map_err(|_| {
                OrchestratorError::LlmError(format!(
                    "claude CLI timed out after {}s",
                    self.config.timeout.as_secs()
                ))
            })?
            .map_err(|e| {
                OrchestratorError::LlmError(format!(
                    "failed to spawn claude CLI — is it installed? {e}"
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OrchestratorError::LlmError(format!(
                "claude CLI exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        let resp: ClaudeCodeResponse = serde_json::from_str(&stdout).map_err(|e| {
            OrchestratorError::LlmError(format!(
                "failed to parse claude CLI JSON output: {e}\nRaw: {}",
                &stdout[..stdout.len().min(200)]
            ))
        })?;

        if resp.is_error {
            let subtype = resp.subtype.as_deref().unwrap_or("unknown");
            return Err(OrchestratorError::LlmError(format!(
                "claude CLI returned error ({}): {}",
                subtype,
                resp.result.as_deref().unwrap_or("no details")
            )));
        }

        let usage = resp.usage.unwrap_or_default();
        // Claude Code counts cache tokens separately — include them in total.
        let total_input =
            usage.input_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;

        Ok(LlmResponse {
            content: resp.result.unwrap_or_default(),
            input_tokens: total_input,
            output_tokens: usage.output_tokens,
            model: self.config.model.clone(),
            stop_reason: resp.stop_reason,
        })
    }

    fn provider_name(&self) -> &str {
        "claude-code"
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
// RetryingLlmProvider — wraps any provider with exponential backoff on 429
// ---------------------------------------------------------------------------

/// Wraps an [`LlmProvider`] with automatic retry on rate-limit (429) errors.
///
/// Uses exponential backoff with jitter, respecting `Retry-After` headers
/// when available. Consumes the previously-unused `max_retries` config field.
pub struct RetryingLlmProvider {
    inner: Arc<dyn LlmProvider>,
    max_retries: u32,
    /// Base delay for exponential backoff (doubled each retry).
    base_delay_ms: u64,
}

impl RetryingLlmProvider {
    /// Create a new retrying wrapper.
    ///
    /// `max_retries` = 0 means no retries (pass-through).
    pub fn new(inner: Arc<dyn LlmProvider>, max_retries: u32) -> Self {
        Self {
            inner,
            max_retries,
            base_delay_ms: 1000,
        }
    }

    /// Override the base delay (useful for tests).
    #[cfg(test)]
    pub fn with_base_delay_ms(mut self, ms: u64) -> Self {
        self.base_delay_ms = ms;
        self
    }

    /// Compute backoff delay: max(retry_after, base * 2^attempt) + jitter.
    fn backoff_ms(&self, attempt: u32, retry_after_ms: u64) -> u64 {
        let exponential = self.base_delay_ms.saturating_mul(1u64 << attempt);
        let base = exponential.max(retry_after_ms);
        // Add ~25% jitter to avoid thundering herd.
        let jitter = base / 4;
        // Deterministic jitter from attempt number (no rand dependency needed).
        let jitter_offset = (attempt as u64 * 7919) % (jitter.max(1));
        base.saturating_add(jitter_offset).min(60_000) // cap at 60s
    }
}

#[async_trait]
impl LlmProvider for RetryingLlmProvider {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
        let mut last_err = None;

        for attempt in 0..=self.max_retries {
            let req_clone = request.clone();
            match self.inner.complete(req_clone).await {
                Ok(resp) => return Ok(resp),
                Err(OrchestratorError::RateLimited {
                    retry_after_ms,
                    ref message,
                }) => {
                    if attempt == self.max_retries {
                        warn!(
                            attempt,
                            max = self.max_retries,
                            "rate limit: retries exhausted"
                        );
                        return Err(OrchestratorError::RateLimited {
                            retry_after_ms,
                            message: message.clone(),
                        });
                    }

                    let delay = self.backoff_ms(attempt, retry_after_ms);
                    info!(
                        attempt,
                        delay_ms = delay,
                        retry_after_ms,
                        "rate limited, backing off"
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    last_err = Some(OrchestratorError::RateLimited {
                        retry_after_ms,
                        message: message.clone(),
                    });
                }
                Err(e) => return Err(e), // Non-retryable error.
            }
        }

        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LlmError("retry loop exited unexpectedly".into())
        }))
    }

    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }

    fn model_name(&self) -> &str {
        self.inner.model_name()
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
            effort_override: None,
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

    // -- RetryingLlmProvider tests --

    use std::sync::atomic::{AtomicU32, Ordering};

    /// Mock provider that returns RateLimited for the first N calls, then succeeds.
    struct RateLimitMock {
        fail_count: AtomicU32,
        remaining_failures: AtomicU32,
    }

    impl RateLimitMock {
        fn new(failures: u32) -> Self {
            Self {
                fail_count: AtomicU32::new(0),
                remaining_failures: AtomicU32::new(failures),
            }
        }

        fn calls(&self) -> u32 {
            self.fail_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmProvider for RateLimitMock {
        async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
            self.fail_count.fetch_add(1, Ordering::SeqCst);
            let remaining = self.remaining_failures.fetch_sub(1, Ordering::SeqCst);
            if remaining > 0 {
                Err(OrchestratorError::RateLimited {
                    retry_after_ms: 100,
                    message: "rate limited".into(),
                })
            } else {
                Ok(LlmResponse {
                    content: "success after retry".into(),
                    input_tokens: 10,
                    output_tokens: 5,
                    model: "mock".into(),
                    stop_reason: Some("end_turn".into()),
                })
            }
        }
        fn provider_name(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
    }

    fn test_request() -> LlmRequest {
        LlmRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: "hi".into(),
            }],
            max_tokens: 10,
            temperature: 0.0,
            system_prompt: None,
            effort_override: None,
        }
    }

    #[tokio::test]
    async fn retry_succeeds_after_rate_limit() {
        let mock = Arc::new(RateLimitMock::new(2));
        let provider = RetryingLlmProvider::new(Arc::clone(&mock) as Arc<dyn LlmProvider>, 3)
            .with_base_delay_ms(1); // Fast for tests

        let resp = provider.complete(test_request()).await.unwrap();
        assert_eq!(resp.content, "success after retry");
        assert_eq!(mock.calls(), 3); // 2 failures + 1 success
    }

    #[tokio::test]
    async fn retry_exhausted_returns_rate_limited() {
        let mock = Arc::new(RateLimitMock::new(5));
        let provider = RetryingLlmProvider::new(Arc::clone(&mock) as Arc<dyn LlmProvider>, 2)
            .with_base_delay_ms(1);

        let err = provider.complete(test_request()).await.unwrap_err();
        assert!(matches!(err, OrchestratorError::RateLimited { .. }));
        assert_eq!(mock.calls(), 3); // initial + 2 retries
    }

    #[tokio::test]
    async fn retry_zero_retries_passthrough() {
        let mock = Arc::new(RateLimitMock::new(1));
        let provider = RetryingLlmProvider::new(Arc::clone(&mock) as Arc<dyn LlmProvider>, 0)
            .with_base_delay_ms(1);

        let err = provider.complete(test_request()).await.unwrap_err();
        assert!(matches!(err, OrchestratorError::RateLimited { .. }));
        assert_eq!(mock.calls(), 1);
    }

    #[tokio::test]
    async fn retry_non_rate_limit_error_not_retried() {
        struct AlwaysFail;

        #[async_trait]
        impl LlmProvider for AlwaysFail {
            async fn complete(&self, _: LlmRequest) -> Result<LlmResponse, OrchestratorError> {
                Err(OrchestratorError::LlmError("auth failed".into()))
            }
            fn provider_name(&self) -> &str {
                "fail"
            }
            fn model_name(&self) -> &str {
                "fail"
            }
        }

        let provider = RetryingLlmProvider::new(Arc::new(AlwaysFail), 3).with_base_delay_ms(1);
        let err = provider.complete(test_request()).await.unwrap_err();
        assert!(matches!(err, OrchestratorError::LlmError(_)));
    }

    #[test]
    fn backoff_respects_retry_after() {
        let provider = RetryingLlmProvider::new(
            Arc::new(StubLlmProvider {
                model: "test".into(),
            }),
            3,
        )
        .with_base_delay_ms(100);

        // With retry_after=5000ms and attempt=0, base=100ms, so retry_after wins.
        let delay = provider.backoff_ms(0, 5000);
        assert!(delay >= 5000);

        // With retry_after=0 and attempt=2, base=400ms (100*2^2).
        let delay = provider.backoff_ms(2, 0);
        assert!(delay >= 400);
    }

    #[test]
    fn backoff_capped_at_60s() {
        let provider = RetryingLlmProvider::new(
            Arc::new(StubLlmProvider {
                model: "test".into(),
            }),
            10,
        )
        .with_base_delay_ms(1000);

        let delay = provider.backoff_ms(8, 0); // 1000 * 256 = 256000, capped
        assert!(delay <= 60_000);
    }

    #[test]
    fn retrying_delegates_name() {
        let inner = Arc::new(StubLlmProvider {
            model: "test-model".into(),
        });
        let provider = RetryingLlmProvider::new(inner, 3);
        assert_eq!(provider.provider_name(), "stub");
        assert_eq!(provider.model_name(), "test-model");
    }
}
