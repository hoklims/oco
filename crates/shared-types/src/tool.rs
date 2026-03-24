use serde::{Deserialize, Serialize};

/// Descriptor for a tool that can be called by the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: serde_json::Value,
    /// Whether this tool performs write/destructive operations.
    pub is_write: bool,
    /// Whether this tool requires user confirmation before execution.
    pub requires_confirmation: bool,
    /// Maximum expected execution time in seconds.
    pub timeout_secs: u32,
    /// Categories/tags for tool classification.
    pub tags: Vec<String>,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Policy gate decision for tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGateDecision {
    Allow,
    RequireConfirmation { reason: String },
    Deny { reason: String },
}
