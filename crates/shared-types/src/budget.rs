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
        // Core budgets: tokens and tool calls are always enforced.
        self.tokens_used < self.max_total_tokens
            && self.tool_calls_used < self.max_tool_calls
            // Gate budgets: a max of 0 means "this action type is forbidden",
            // not "the entire session is over". Only block when the limit is
            // positive and has been reached — individual action gates (in the
            // policy engine) still prevent retrieval/verify when at capacity.
            && (self.max_retrievals == 0 || self.retrievals_used < self.max_retrievals)
            && (self.max_verify_cycles == 0 || self.verify_cycles_used < self.max_verify_cycles)
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
            max_output_tokens: 16_000, // Provider-agnostic default; Claude Code supports up to 64k
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

impl Budget {
    /// Create a budget tuned for the given task complexity.
    ///
    /// Lower complexity → smaller budgets (faster, cheaper).
    /// Higher complexity → larger budgets (deeper analysis).
    pub fn for_complexity(complexity: crate::TaskComplexity) -> Self {
        use crate::TaskComplexity;
        match complexity {
            TaskComplexity::Trivial => Self {
                max_context_tokens: 32_000,
                max_output_tokens: 4_000,
                max_total_tokens: 100_000,
                max_tool_calls: 5,
                max_retrievals: 3,
                max_duration_secs: 30,
                max_verify_cycles: 1,
                ..Self::default()
            },
            TaskComplexity::Low => Self {
                max_context_tokens: 64_000,
                max_output_tokens: 8_000,
                max_total_tokens: 300_000,
                max_tool_calls: 15,
                max_retrievals: 10,
                max_duration_secs: 60,
                max_verify_cycles: 3,
                ..Self::default()
            },
            TaskComplexity::Medium => Self::default(),
            TaskComplexity::High => Self {
                max_context_tokens: 128_000,
                max_output_tokens: 16_000,
                max_total_tokens: 2_000_000,
                max_tool_calls: 80,
                max_retrievals: 50,
                max_duration_secs: 600,
                max_verify_cycles: 15,
                ..Self::default()
            },
            TaskComplexity::Critical => Self {
                max_context_tokens: 200_000,
                max_output_tokens: 32_000,
                max_total_tokens: 5_000_000,
                max_tool_calls: 150,
                max_retrievals: 100,
                max_duration_secs: 900,
                max_verify_cycles: 20,
                ..Self::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_retrieval_limit_does_not_exhaust_budget() {
        let budget = Budget {
            max_retrievals: 0,
            retrievals_used: 0,
            ..Budget::default()
        };
        assert!(
            budget.is_within_limits(),
            "max_retrievals=0 means 'no retrievals allowed', not 'budget exhausted'"
        );
    }

    #[test]
    fn zero_verify_limit_does_not_exhaust_budget() {
        let budget = Budget {
            max_verify_cycles: 0,
            verify_cycles_used: 0,
            ..Budget::default()
        };
        assert!(
            budget.is_within_limits(),
            "max_verify_cycles=0 means 'no verify allowed', not 'budget exhausted'"
        );
    }

    #[test]
    fn positive_retrieval_limit_enforced() {
        let budget = Budget {
            max_retrievals: 3,
            retrievals_used: 3,
            ..Budget::default()
        };
        assert!(!budget.is_within_limits());
    }
}
