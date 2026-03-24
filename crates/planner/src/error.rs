/// Errors that can occur during plan generation.
#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("LLM call failed: {0}")]
    LlmError(String),

    #[error("failed to parse LLM output as plan: {0}")]
    ParseError(String),

    #[error("generated plan is invalid: {0}")]
    ValidationError(String),

    #[error("planning budget exceeded: used {used} tokens, max {max}")]
    BudgetExceeded { used: u32, max: u32 },

    #[error("no capabilities available for required role: {0}")]
    NoCapabilities(String),

    #[error("replan failed: {0}")]
    ReplanFailed(String),
}
