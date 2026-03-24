use oco_shared_types::{AssembledContext, ContextItem, ContextPriority, ContextSource};

use crate::assembler::{CategoryBudgets, ContextAssembler};
use crate::dedup::ContextDeduplicator;
use crate::estimator::TokenEstimator;

/// Fluent API for building an [`AssembledContext`].
///
/// ```ignore
/// let ctx = ContextBuilder::new(128_000)
///     .with_system_prompt("You are a helpful assistant.")
///     .with_user_request("Explain this code")
///     .with_retrieved_items(items)
///     .with_tool_outputs(outputs)
///     .with_pinned(pinned)
///     .build();
/// ```
pub struct ContextBuilder {
    budget_tokens: u32,
    items: Vec<ContextItem>,
    /// v2: Current orchestration step for staleness decay.
    current_step: u32,
    /// v2: Staleness half-life in steps (0 = disabled).
    staleness_half_life: u32,
    /// v2: Per-category budget caps.
    category_budgets: Option<CategoryBudgets>,
}

impl ContextBuilder {
    pub fn new(budget_tokens: u32) -> Self {
        Self {
            budget_tokens,
            items: Vec::new(),
            current_step: 0,
            staleness_half_life: 0,
            category_budgets: None,
        }
    }

    /// v2: Enable staleness-aware context assembly.
    pub fn with_staleness(mut self, current_step: u32, half_life_steps: u32) -> Self {
        self.current_step = current_step;
        self.staleness_half_life = half_life_steps;
        self
    }

    /// v2: Set per-category budget caps.
    pub fn with_category_budgets(mut self, budgets: CategoryBudgets) -> Self {
        self.category_budgets = Some(budgets);
        self
    }

    /// Add a system prompt as the highest-priority context item.
    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.items.push(ContextItem {
            key: "__system_prompt__".to_string(),
            label: "System prompt".to_string(),
            content: prompt.to_string(),
            token_estimate: TokenEstimator::estimate_tokens(prompt),
            priority: ContextPriority::System,
            source: ContextSource::UserRequest,
            pinned: true,
            relevance: 1.0,
            added_at: chrono::Utc::now(),
            added_at_step: 0, // System prompt is always step 0
        });
        self
    }

    /// Add the user's request as a high-priority pinned item.
    pub fn with_user_request(mut self, request: &str) -> Self {
        self.items.push(ContextItem {
            key: "__user_request__".to_string(),
            label: "User request".to_string(),
            content: request.to_string(),
            token_estimate: TokenEstimator::estimate_tokens(request),
            priority: ContextPriority::Pinned,
            source: ContextSource::UserRequest,
            pinned: true,
            relevance: 1.0,
            added_at: chrono::Utc::now(),
            added_at_step: 0, // User request is always step 0
        });
        self
    }

    /// Add items retrieved from search / retrieval.
    pub fn with_retrieved_items(mut self, items: Vec<ContextItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Add tool output items.
    pub fn with_tool_outputs(mut self, outputs: Vec<ContextItem>) -> Self {
        self.items.extend(outputs);
        self
    }

    /// Add explicitly pinned items (user-pinned files, notes, etc.).
    pub fn with_pinned(mut self, mut pinned: Vec<ContextItem>) -> Self {
        for item in &mut pinned {
            item.pinned = true;
            if item.priority < ContextPriority::Pinned {
                item.priority = ContextPriority::Pinned;
            }
        }
        self.items.extend(pinned);
        self
    }

    /// Deduplicate, sort, and assemble all items within the token budget.
    /// v2: Applies staleness decay and category budgets when configured.
    pub fn build(self) -> AssembledContext {
        let deduped = ContextDeduplicator::deduplicate(self.items);

        let mut assembler = ContextAssembler::new(self.budget_tokens);

        // v2: Enable staleness decay if configured.
        if self.staleness_half_life > 0 {
            assembler = assembler.with_staleness(self.current_step, self.staleness_half_life);
        }

        // v2: Apply category budgets if configured.
        if let Some(budgets) = self.category_budgets {
            assembler = assembler.with_category_budgets(budgets);
        }

        for item in deduped {
            assembler.add_item(item);
        }

        assembler.assemble()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_assembles_context() {
        let ctx = ContextBuilder::new(100_000)
            .with_system_prompt("You are helpful.")
            .with_user_request("Explain context assembly.")
            .build();

        assert_eq!(ctx.items.len(), 2);
        assert_eq!(ctx.items[0].key, "__system_prompt__");
        assert_eq!(ctx.items[1].key, "__user_request__");
        assert!(ctx.total_tokens > 0);
        assert_eq!(ctx.excluded_count, 0);
    }

    #[test]
    fn builder_respects_budget() {
        let big_content = "x".repeat(500_000); // ~125k tokens
        let big_item = ContextItem {
            key: "big".to_string(),
            label: "big".to_string(),
            content: big_content,
            token_estimate: 0,
            priority: ContextPriority::Low,
            source: ContextSource::UserRequest,
            pinned: false,
            relevance: 0.5,
            added_at: chrono::Utc::now(),
            added_at_step: 0,
        };

        let ctx = ContextBuilder::new(1000)
            .with_system_prompt("Hello")
            .with_retrieved_items(vec![big_item])
            .build();

        assert!(ctx.total_tokens <= 1000);
        assert!(ctx.excluded_count >= 1);
    }

    #[test]
    fn builder_with_staleness_activates_decay() {
        let mut old_item = ContextItem {
            key: "old".to_string(),
            label: "old".to_string(),
            content: "old content".to_string(),
            token_estimate: 10,
            priority: ContextPriority::High,
            source: ContextSource::SearchResult { query: "q".into() },
            pinned: false,
            relevance: 1.0,
            added_at: chrono::Utc::now(),
            added_at_step: 0, // Very old
        };

        let mut new_item = ContextItem {
            key: "new".to_string(),
            label: "new".to_string(),
            content: "new content".to_string(),
            token_estimate: 10,
            priority: ContextPriority::High,
            source: ContextSource::SearchResult { query: "q".into() },
            pinned: false,
            relevance: 0.5,
            added_at: chrono::Utc::now(),
            added_at_step: 9, // Recent
        };

        // With staleness at step 10, half_life=5:
        // old: decay = 0.5^(10/5) = 0.25, effective = 1.0 * 0.25 = 0.25
        // new: decay = 0.5^(1/5) = 0.87, effective = 0.5 * 0.87 = 0.435
        let ctx = ContextBuilder::new(1000)
            .with_staleness(10, 5)
            .with_retrieved_items(vec![old_item, new_item])
            .build();

        // New item should come first (higher decayed relevance)
        assert_eq!(ctx.items[0].key, "new");
        assert_eq!(ctx.items[1].key, "old");
    }
}
