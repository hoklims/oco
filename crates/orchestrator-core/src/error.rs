use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),

    #[error("max steps reached: {0}")]
    MaxStepsReached(u32),

    #[error("policy error: {0}")]
    PolicyError(String),

    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),

    #[error("retrieval failed: {0}")]
    RetrievalFailed(String),

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("context assembly failed: {0}")]
    ContextAssemblyFailed(String),

    #[error("LLM provider error: {0}")]
    LlmError(String),

    #[error("rate limited (retry after {retry_after_ms}ms): {message}")]
    RateLimited {
        retry_after_ms: u64,
        message: String,
    },

    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("planning failed: {0}")]
    PlanningFailed(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
