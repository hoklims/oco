use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Explicit budget constraints for an orchestration session.
/// All budgets are enforced by the policy engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// Maximum tokens for LLM input context.
    pub max_context_tokens: u32,
    /// Maximum tokens for LLM output generation.
    pub max_output_tokens: u32,
    /// Total token budget across all LLM calls in this session.
    pub max_total_tokens: u64,
    /// Tokens consumed so far.
    pub tokens_used: u64,
    /// Maximum number of tool calls in this session.
    pub max_tool_calls: u32,
    /// Tool calls made so far.
    pub tool_calls_used: u32,
    /// Maximum number of retrieval operations.
    pub max_retrievals: u32,
    /// Retrievals performed so far.
    pub retrievals_used: u32,
    /// Maximum wall-clock time for the session.
    pub max_duration_secs: u64,
    /// Maximum number of verification cycles.
    pub max_verify_cycles: u32,
    /// Verification cycles used so far.
    pub verify_cycles_used: u32,
}

impl Budget {
    pub fn is_within_limits(&self) -> bool {
        self.tokens_used < self.max_total_tokens
            && self.tool_calls_used < self.max_tool_calls
            && self.retrievals_used < self.max_retrievals
            && self.verify_cycles_used < self.max_verify_cycles
    }

    pub fn remaining_tokens(&self) -> u64 {
        self.max_total_tokens.saturating_sub(self.tokens_used)
    }

    pub fn remaining_tool_calls(&self) -> u32 {
        self.max_tool_calls.saturating_sub(self.tool_calls_used)
    }

    pub fn record_token_usage(&mut self, tokens: u64) {
        self.tokens_used += tokens;
    }

    pub fn record_tool_call(&mut self) {
        self.tool_calls_used += 1;
    }

    pub fn record_retrieval(&mut self) {
        self.retrievals_used += 1;
    }

    pub fn record_verify_cycle(&mut self) {
        self.verify_cycles_used += 1;
    }

    pub fn max_duration(&self) -> Duration {
        Duration::from_secs(self.max_duration_secs)
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_context_tokens: 128_000,
            max_output_tokens: 16_000,
            max_total_tokens: 1_000_000,
            tokens_used: 0,
            max_tool_calls: 50,
            tool_calls_used: 0,
            max_retrievals: 30,
            retrievals_used: 0,
            max_duration_secs: 300,
            max_verify_cycles: 10,
            verify_cycles_used: 0,
        }
    }
}
