use std::collections::HashMap;

use oco_shared_types::{AssembledContext, ContextItem, ContextSource};
use tracing::debug;

use crate::estimator::TokenEstimator;

/// Per-category token budget caps.
#[derive(Debug, Clone, Default)]
pub struct CategoryBudgets {
    /// Max tokens for search/retrieval results.
    pub search_results: Option<u32>,
    /// Max tokens for tool outputs.
    pub tool_outputs: Option<u32>,
    /// Max tokens for verification results.
    pub verification: Option<u32>,
}

/// Assembles a set of [`ContextItem`]s into a budget-respecting context window.
pub struct ContextAssembler {
    budget_tokens: u32,
    items: Vec<ContextItem>,
    /// v2: Current orchestration step for staleness decay.
    current_step: u32,
    /// v2: Half-life in steps for relevance decay (0 = no decay).
    staleness_half_life: u32,
    /// v2: Per-category budget caps.
    category_budgets: CategoryBudgets,
}

impl ContextAssembler {
    pub fn new(budget_tokens: u32) -> Self {
        Self {
            budget_tokens,
            items: Vec::new(),
            current_step: 0,
            staleness_half_life: 0,
            category_budgets: CategoryBudgets::default(),
        }
    }

    /// Set the current step for staleness-aware assembly.
    pub fn with_staleness(mut self, current_step: u32, half_life_steps: u32) -> Self {
        self.current_step = current_step;
        self.staleness_half_life = half_life_steps;
        self
    }

    /// Set per-category budget caps.
    pub fn with_category_budgets(mut self, budgets: CategoryBudgets) -> Self {
        self.category_budgets = budgets;
        self
    }

    /// Add a context item to the internal collection.
    pub fn add_item(&mut self, item: ContextItem) {
        self.items.push(item);
    }

    /// Classify a context item into a category for budget tracking.
    fn category(item: &ContextItem) -> &'static str {
        match &item.source {
            ContextSource::SearchResult { .. } => "search_results",
            ContextSource::ToolOutput { .. } => "tool_outputs",
            ContextSource::VerificationResult => "verification",
            _ => "other",
        }
    }

    /// Get the budget cap for a category, if any.
    fn category_cap(&self, category: &str) -> Option<u32> {
        match category {
            "search_results" => self.category_budgets.search_results,
            "tool_outputs" => self.category_budgets.tool_outputs,
            "verification" => self.category_budgets.verification,
            _ => None,
        }
    }

    /// Assemble the final context window.
    ///
    /// 1. Apply staleness decay to relevance scores.
    /// 2. Pinned items are included first (sorted by priority desc, relevance desc).
    /// 3. Remaining items are sorted by priority desc, then decayed relevance desc.
    /// 4. Items are greedily included until global or category budget is exhausted.
    pub fn assemble(&self) -> AssembledContext {
        // Apply staleness decay.
        let items_with_decay: Vec<(ContextItem, f64)> = self
            .items
            .iter()
            .map(|item| {
                let effective_relevance = if self.staleness_half_life > 0 {
                    item.decayed_relevance(self.current_step, self.staleness_half_life)
                } else {
                    item.relevance
                };
                (item.clone(), effective_relevance)
            })
            .collect();

        let mut pinned: Vec<&(ContextItem, f64)> =
            items_with_decay.iter().filter(|(i, _)| i.pinned).collect();
        let mut unpinned: Vec<&(ContextItem, f64)> =
            items_with_decay.iter().filter(|(i, _)| !i.pinned).collect();

        let sort_key = |a: &&(ContextItem, f64), b: &&(ContextItem, f64)| {
            b.0.priority
                .cmp(&a.0.priority)
                .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        };

        pinned.sort_by(sort_key);
        unpinned.sort_by(sort_key);

        let mut included: Vec<ContextItem> = Vec::new();
        let mut total_tokens: u32 = 0;
        let mut excluded_count: u32 = 0;
        let mut category_usage: HashMap<&str, u32> = HashMap::new();

        let mut try_include = |item: &ContextItem| -> bool {
            let tokens = TokenEstimator::estimate_item(item);

            // Check global budget.
            if total_tokens.saturating_add(tokens) > self.budget_tokens {
                excluded_count += 1;
                return false;
            }

            // Check category budget.
            let cat = Self::category(item);
            if let Some(cap) = self.category_cap(cat) {
                let used = category_usage.get(cat).copied().unwrap_or(0);
                if used.saturating_add(tokens) > cap {
                    excluded_count += 1;
                    debug!(
                        key = %item.key,
                        category = cat,
                        "excluded — category budget exhausted"
                    );
                    return false;
                }
            }

            total_tokens += tokens;
            *category_usage.entry(cat).or_insert(0) += tokens;
            included.push(item.clone());
            true
        };

        for (item, _) in &pinned {
            if !try_include(item) {
                debug!(
                    key = %item.key,
                    "pinned item excluded — budget exhausted"
                );
            }
        }

        for (item, _) in &unpinned {
            try_include(item);
        }

        AssembledContext {
            items: included,
            total_tokens,
            budget_tokens: self.budget_tokens,
            excluded_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{ContextPriority, ContextSource};

    fn make_item(key: &str, tokens: u32, priority: ContextPriority, pinned: bool) -> ContextItem {
        ContextItem {
            key: key.to_string(),
            label: key.to_string(),
            content: "x".repeat((tokens * 4) as usize),
            token_estimate: tokens,
            priority,
            source: ContextSource::UserRequest,
            pinned,
            relevance: 0.5,
            added_at: chrono::Utc::now(),
            added_at_step: 0,
        }
    }

    fn make_search_item(key: &str, tokens: u32, step: u32) -> ContextItem {
        ContextItem {
            key: key.to_string(),
            label: key.to_string(),
            content: "x".repeat((tokens * 4) as usize),
            token_estimate: tokens,
            priority: ContextPriority::High,
            source: ContextSource::SearchResult {
                query: "test".into(),
            },
            pinned: false,
            relevance: 0.8,
            added_at: chrono::Utc::now(),
            added_at_step: step,
        }
    }

    #[test]
    fn pinned_items_come_first() {
        let mut asm = ContextAssembler::new(200);
        asm.add_item(make_item("low", 50, ContextPriority::Low, false));
        asm.add_item(make_item("pinned", 50, ContextPriority::Low, true));
        asm.add_item(make_item("high", 50, ContextPriority::High, false));

        let ctx = asm.assemble();
        assert_eq!(ctx.items[0].key, "pinned");
    }

    #[test]
    fn budget_is_respected() {
        let mut asm = ContextAssembler::new(100);
        asm.add_item(make_item("a", 60, ContextPriority::High, false));
        asm.add_item(make_item("b", 60, ContextPriority::Medium, false));

        let ctx = asm.assemble();
        assert_eq!(ctx.items.len(), 1);
        assert_eq!(ctx.excluded_count, 1);
        assert!(ctx.total_tokens <= 100);
    }

    #[test]
    fn sorted_by_priority_then_relevance() {
        let mut asm = ContextAssembler::new(1000);
        let mut a = make_item("a", 10, ContextPriority::High, false);
        a.relevance = 0.8;
        let mut b = make_item("b", 10, ContextPriority::High, false);
        b.relevance = 0.9;
        asm.add_item(a);
        asm.add_item(b);

        let ctx = asm.assemble();
        assert_eq!(ctx.items[0].key, "b");
        assert_eq!(ctx.items[1].key, "a");
    }

    #[test]
    fn staleness_decay_reorders_items() {
        let mut asm = ContextAssembler::new(1000).with_staleness(10, 5); // current_step=10, half_life=5

        // Old item (step 0, staleness=10, decay = 0.5^2 = 0.25)
        let mut old = make_search_item("old", 10, 0);
        old.relevance = 1.0; // After decay: 0.25

        // Recent item (step 8, staleness=2, decay = 0.5^0.4 ≈ 0.76)
        let mut recent = make_search_item("recent", 10, 8);
        recent.relevance = 0.5; // After decay: ~0.38

        asm.add_item(old);
        asm.add_item(recent);

        let ctx = asm.assemble();
        // Recent item should come first due to higher decayed relevance.
        assert_eq!(ctx.items[0].key, "recent");
        assert_eq!(ctx.items[1].key, "old");
    }

    #[test]
    fn category_budget_limits_search_results() {
        let budgets = CategoryBudgets {
            search_results: Some(50),
            ..Default::default()
        };
        let mut asm = ContextAssembler::new(1000).with_category_budgets(budgets);

        asm.add_item(make_search_item("s1", 30, 0));
        asm.add_item(make_search_item("s2", 30, 0)); // exceeds category cap
        asm.add_item(make_item("other", 30, ContextPriority::Medium, false)); // no cap

        let ctx = asm.assemble();
        let search_count = ctx
            .items
            .iter()
            .filter(|i| matches!(i.source, ContextSource::SearchResult { .. }))
            .count();
        assert_eq!(search_count, 1); // Only 1 search result fits category budget
        assert!(ctx.items.len() >= 2); // Other item included
    }
}
