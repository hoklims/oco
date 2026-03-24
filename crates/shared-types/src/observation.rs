use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A structured observation produced by executing an action.
/// All tool outputs and retrieval results are normalized into this format
/// before entering the orchestration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub source: ObservationSource,
    pub kind: ObservationKind,
    /// Estimated token cost of including this observation in context.
    pub token_estimate: u32,
    /// Relevance score (0.0 to 1.0) assigned during retrieval/reranking.
    pub relevance_score: Option<f64>,
}

impl Observation {
    pub fn new(source: ObservationSource, kind: ObservationKind, token_estimate: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            source,
            kind,
            token_estimate,
            relevance_score: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    ToolExecution { tool_name: String },
    Retrieval { source_type: String },
    Verification { strategy: String },
    LlmResponse,
    UserInput,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObservationKind {
    /// Raw text content (file contents, search results, etc.)
    Text {
        content: String,
        metadata: Option<serde_json::Value>,
    },
    /// Code snippet with location information.
    CodeSnippet {
        file_path: String,
        start_line: u32,
        end_line: u32,
        content: String,
        language: Option<String>,
    },
    /// Structured data (JSON output from tools).
    Structured { data: serde_json::Value },
    /// Verification result (pass/fail with details).
    VerificationResult {
        passed: bool,
        output: String,
        failures: Vec<String>,
    },
    /// Error observation.
    Error { message: String, recoverable: bool },
    /// Symbol information from code intelligence.
    Symbol {
        name: String,
        kind: String,
        file_path: String,
        line: u32,
        signature: Option<String>,
    },
}
