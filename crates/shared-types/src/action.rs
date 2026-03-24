use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The six possible actions the orchestrator can select at each step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorAction {
    /// Respond directly to the user with generated content.
    Respond { content: String },
    /// Retrieve additional context (code, docs, search results).
    Retrieve {
        query: String,
        sources: Vec<RetrievalSource>,
        max_results: u32,
    },
    /// Call an external tool (shell, LSP, file ops, etc.).
    ToolCall {
        tool_name: String,
        arguments: serde_json::Value,
    },
    /// Verify a hypothesis or result (run tests, build, lint).
    Verify {
        strategy: VerificationStrategy,
        target: Option<String>,
    },
    /// Mutate working memory (promote, invalidate, link, plan).
    UpdateMemory { operation: MemoryOperation },
    /// Stop the current orchestration loop.
    Stop { reason: StopReason },
}

/// Operations the LLM can perform on working memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MemoryOperation {
    /// Promote a finding to a verified fact.
    PromoteToFact { entry_id: Uuid },
    /// Invalidate an entry with a reason.
    Invalidate { entry_id: Uuid, reason: String },
    /// Mark an entry as superseded by another.
    Supersede { old_id: Uuid, new_id: Uuid },
    /// Link evidence: `evidence_id` supports or contradicts `target_id`.
    LinkEvidence {
        target_id: Uuid,
        evidence_id: Uuid,
        supports: bool,
    },
    /// Add a new hypothesis.
    AddHypothesis { content: String, confidence: f64 },
    /// Add an open question.
    AddQuestion { content: String },
    /// Resolve (remove) a question by ID.
    ResolveQuestion { question_id: Uuid },
    /// Replace the current plan.
    UpdatePlan { steps: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalSource {
    CodeSearch,
    FullTextSearch,
    SemanticSearch,
    FileRead,
    SymbolLookup,
    Documentation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStrategy {
    RunTests,
    Build,
    Lint,
    TypeCheck,
    Custom { command: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    TaskComplete,
    BudgetExhausted,
    UserCancelled,
    Error { message: String },
    MaxStepsReached,
    NeedsUserInput { question: String },
}

/// Complexity classification for incoming tasks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TaskComplexity {
    /// Simple lookup or direct answer.
    Trivial,
    /// Requires some context but straightforward.
    Low,
    /// Multi-step, needs retrieval and possibly tool calls.
    Medium,
    /// Complex refactoring, debugging, or multi-file changes.
    High,
    /// Architectural changes, large-scale refactors.
    Critical,
}
