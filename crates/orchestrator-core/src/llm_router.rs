//! Multi-model routing — select the best LLM for each plan step.
//!
//! Factory AI's key differentiator: each agent uses the optimal model for its task.
//! Claude Code subagents support per-agent `model` overrides (opus/sonnet/haiku).
//!
//! The `LlmRouter` holds multiple providers and selects one per step based on:
//! 1. `AgentRole.preferred_model` (explicit override from planner)
//! 2. Step type heuristics (planning→opus, investigation→haiku, impl→sonnet)
//! 3. Budget-aware downgrade (if remaining budget < 30%, use cheaper model)

use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use crate::llm::LlmProvider;
use oco_shared_types::PlanStep;

/// Routes LLM calls to the best provider/model for a given step.
pub struct LlmRouter {
    /// Named providers: "opus", "sonnet", "haiku", or custom names.
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    /// Default provider name when no preference is specified.
    default: String,
}

impl LlmRouter {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            providers: HashMap::new(),
            default: default.into(),
        }
    }

    /// Register a provider under a name (e.g., "opus", "sonnet", "haiku").
    pub fn with_provider(
        mut self,
        name: impl Into<String>,
        provider: Arc<dyn LlmProvider>,
    ) -> Self {
        self.providers.insert(name.into(), provider);
        self
    }

    /// Register a single provider as the default (for simple setups).
    pub fn single(provider: Arc<dyn LlmProvider>) -> Self {
        let name = provider.model_name().to_string();
        Self::new(&name).with_provider(name, provider)
    }

    /// Select the best provider for a plan step.
    ///
    /// Priority:
    /// 1. Step's `agent_role.preferred_model` if set and available
    /// 2. Step type heuristic
    /// 3. Budget-aware fallback
    /// 4. Default provider
    pub fn for_step(&self, step: &PlanStep, budget_remaining_pct: f64) -> Arc<dyn LlmProvider> {
        // 1. Explicit model preference from planner
        if let Some(ref preferred) = step.agent_role.preferred_model
            && let Some(provider) = self.providers.get(preferred)
        {
            debug!(
                step = %step.name,
                model = preferred,
                "using preferred model"
            );
            return provider.clone();
        }

        // 2. Budget-aware downgrade: if budget is tight, use cheapest
        if budget_remaining_pct < 0.3
            && let Some(provider) = self
                .providers
                .get("haiku")
                .or_else(|| self.providers.get("sonnet"))
        {
            debug!(
                step = %step.name,
                budget_pct = budget_remaining_pct,
                "budget-aware downgrade"
            );
            return provider.clone();
        }

        // 3. Step type heuristic based on role
        let heuristic_model = model_for_role(&step.agent_role.name);
        if let Some(provider) = self.providers.get(heuristic_model) {
            debug!(
                step = %step.name,
                role = %step.agent_role.name,
                model = heuristic_model,
                "heuristic model selection"
            );
            return provider.clone();
        }

        // 4. Default
        self.providers
            .get(&self.default)
            .or_else(|| self.providers.values().next())
            .expect("LlmRouter has no providers registered")
            .clone()
    }

    /// Select a provider by name directly (for non-step uses like planning).
    pub fn get(&self, name: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers.get(name).cloned()
    }

    /// Get the default provider.
    pub fn default_provider(&self) -> Arc<dyn LlmProvider> {
        self.providers
            .get(&self.default)
            .or_else(|| self.providers.values().next())
            .expect("LlmRouter has no providers registered")
            .clone()
    }

    /// Number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

/// Heuristic: map agent role name to recommended model tier.
fn model_for_role(role: &str) -> &'static str {
    match role {
        // High reasoning tasks → opus
        "architect" | "planner" | "security-reviewer" | "senior" => "opus",
        // Fast exploration → haiku
        "explorer" | "investigator" | "analyzer" | "searcher" => "haiku",
        // Implementation → sonnet (best cost/quality)
        "coder" | "implementer" | "frontend-dev" | "backend" | "tester" => "sonnet",
        // Review → sonnet (needs good judgment)
        "reviewer" | "code-reviewer" => "sonnet",
        // Simple tasks → haiku
        "formatter" | "linter" => "haiku",
        // Debug → sonnet (needs reasoning + code understanding)
        "debugger" | "refactorer" => "sonnet",
        // DevOps → sonnet
        "devops" => "sonnet",
        // Default
        _ => "sonnet",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::StubLlmProvider;
    use oco_shared_types::{AgentRole, PlanStep};

    fn make_router() -> LlmRouter {
        LlmRouter::new("sonnet")
            .with_provider(
                "opus",
                Arc::new(StubLlmProvider {
                    model: "opus".into(),
                }),
            )
            .with_provider(
                "sonnet",
                Arc::new(StubLlmProvider {
                    model: "sonnet".into(),
                }),
            )
            .with_provider(
                "haiku",
                Arc::new(StubLlmProvider {
                    model: "haiku".into(),
                }),
            )
    }

    #[test]
    fn preferred_model_takes_priority() {
        let router = make_router();
        let step = PlanStep::new("review", "Security review")
            .with_role(AgentRole::new("reviewer").with_model("opus"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "opus");
    }

    #[test]
    fn heuristic_selects_haiku_for_explorer() {
        let router = make_router();
        let step = PlanStep::new("explore", "Search codebase")
            .with_role(AgentRole::new("explorer"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "haiku");
    }

    #[test]
    fn heuristic_selects_sonnet_for_coder() {
        let router = make_router();
        let step =
            PlanStep::new("implement", "Write code").with_role(AgentRole::new("coder"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "sonnet");
    }

    #[test]
    fn heuristic_selects_opus_for_architect() {
        let router = make_router();
        let step =
            PlanStep::new("design", "System design").with_role(AgentRole::new("architect"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "opus");
    }

    #[test]
    fn budget_downgrade_to_haiku() {
        let router = make_router();
        let step =
            PlanStep::new("implement", "Write code").with_role(AgentRole::new("coder"));

        // Normal budget → sonnet
        assert_eq!(router.for_step(&step, 0.8).model_name(), "sonnet");
        // Low budget → haiku
        assert_eq!(router.for_step(&step, 0.2).model_name(), "haiku");
    }

    #[test]
    fn preferred_model_overrides_budget_downgrade() {
        let router = make_router();
        let step = PlanStep::new("critical", "Must use opus")
            .with_role(AgentRole::new("coder").with_model("opus"));

        // Even with low budget, preferred model wins
        let provider = router.for_step(&step, 0.1);
        assert_eq!(provider.model_name(), "opus");
    }

    #[test]
    fn unknown_role_defaults_to_sonnet() {
        let router = make_router();
        let step = PlanStep::new("mystery", "Unknown role")
            .with_role(AgentRole::new("some-custom-role"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "sonnet");
    }

    #[test]
    fn single_provider_setup() {
        let provider = Arc::new(StubLlmProvider {
            model: "only-model".into(),
        });
        let router = LlmRouter::single(provider);

        assert_eq!(router.provider_count(), 1);
        assert_eq!(router.default_provider().model_name(), "only-model");
    }

    #[test]
    fn get_by_name() {
        let router = make_router();
        assert!(router.get("opus").is_some());
        assert!(router.get("nonexistent").is_none());
    }

    #[test]
    fn model_for_role_coverage() {
        // Ensure all documented roles map correctly
        assert_eq!(model_for_role("architect"), "opus");
        assert_eq!(model_for_role("security-reviewer"), "opus");
        assert_eq!(model_for_role("explorer"), "haiku");
        assert_eq!(model_for_role("investigator"), "haiku");
        assert_eq!(model_for_role("coder"), "sonnet");
        assert_eq!(model_for_role("tester"), "sonnet");
        assert_eq!(model_for_role("debugger"), "sonnet");
        assert_eq!(model_for_role("formatter"), "haiku");
        assert_eq!(model_for_role("unknown"), "sonnet");
    }
}
