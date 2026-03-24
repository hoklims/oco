use thiserror::Error;

/// Errors that can occur during tool registration and execution.
#[derive(Debug, Error)]
pub enum ToolRuntimeError {
    #[error("tool not found: {name}")]
    ToolNotFound { name: String },

    #[error("execution timed out after {timeout_secs}s for tool: {tool_name}")]
    ExecutionTimeout { tool_name: String, timeout_secs: u64 },

    #[error("execution failed for tool `{tool_name}`: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },

    #[error("permission denied for tool `{tool_name}`: {reason}")]
    PermissionDenied { tool_name: String, reason: String },

    #[error("invalid arguments for tool `{tool_name}`: {reason}")]
    InvalidArguments { tool_name: String, reason: String },

    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}
