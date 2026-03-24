use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A context item that can be included in the LLM prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    /// Unique key for deduplication.
    pub key: String,
    /// Human-readable label.
    pub label: String,
    /// The actual content.
    pub content: String,
    /// Estimated token count.
    pub token_estimate: u32,
    /// Priority (higher = more important, included first).
    pub priority: ContextPriority,
    /// Source of this context item.
    pub source: ContextSource,
    /// Whether this item is pinned (always included if budget allows).
    pub pinned: bool,
    /// Relevance score from retrieval/reranking (0.0 to 1.0).
    pub relevance: f64,
    /// When this context item was added (for staleness tracking).
    #[serde(default = "Utc::now")]
    pub added_at: DateTime<Utc>,
    /// The orchestration step at which this item was added.
    #[serde(default)]
    pub added_at_step: u32,
}

impl ContextItem {
    /// Compute staleness: how many steps ago this item was added.
    pub fn staleness(&self, current_step: u32) -> u32 {
        current_step.saturating_sub(self.added_at_step)
    }

    /// Apply a time-based relevance decay.
    /// Returns the adjusted relevance after decay based on staleness.
    pub fn decayed_relevance(&self, current_step: u32, half_life_steps: u32) -> f64 {
        if half_life_steps == 0 {
            return self.relevance;
        }
        let age = self.staleness(current_step) as f64;
        let decay = 0.5_f64.powf(age / half_life_steps as f64);
        self.relevance * decay
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPriority {
    /// System instructions, always included.
    System = 100,
    /// User request and pinned items.
    Pinned = 90,
    /// Directly relevant code/docs.
    High = 70,
    /// Supporting context.
    Medium = 50,
    /// Nice-to-have background.
    Low = 30,
    /// Compressed summaries.
    Summary = 20,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    UserRequest,
    FileContent { path: String },
    SearchResult { query: String },
    SymbolDefinition { symbol: String },
    Documentation { source: String },
    SessionSummary,
    ToolOutput { tool_name: String },
    VerificationResult,
    PinnedByUser,
}

/// A fully assembled context window ready to send to an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembledContext {
    pub items: Vec<ContextItem>,
    pub total_tokens: u32,
    pub budget_tokens: u32,
    /// Items that were excluded due to budget constraints.
    pub excluded_count: u32,
}

impl AssembledContext {
    pub fn utilization(&self) -> f64 {
        if self.budget_tokens == 0 {
            return 0.0;
        }
        self.total_tokens as f64 / self.budget_tokens as f64
    }
}
