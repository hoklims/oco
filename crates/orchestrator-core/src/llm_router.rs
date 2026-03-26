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
use oco_shared_types::{EffortLevel, PlanStep};

/// Routing decision: which provider and effort level to use for a step.
#[derive(Clone)]
pub struct RoutingDecision {
    pub provider: Arc<dyn LlmProvider>,
    pub effort: EffortLevel,
}

impl std::fmt::Debug for RoutingDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingDecision")
            .field("model", &self.provider.model_name())
            .field("effort", &self.effort)
            .finish()
    }
}

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

    /// Select the best provider **and** effort level for a plan step.
    ///
    /// Returns a `RoutingDecision` combining model + effort. The effort is
    /// determined by: step preference → role heuristic → budget-aware downgrade.
    pub fn route_step(&self, step: &PlanStep, budget_remaining_pct: f64) -> RoutingDecision {
        let provider = self.for_step(step, budget_remaining_pct);

        // 1. Determine base effort: explicit preference or role heuristic
        let base_effort = if let Some(effort) = step.agent_role.preferred_effort {
            debug!(step = %step.name, %effort, "using preferred effort");
            effort
        } else {
            let effort = effort_for_role(&step.agent_role.name);
            debug!(step = %step.name, role = %step.agent_role.name, %effort, "heuristic effort");
            effort
        };

        // 2. Budget constraint: cap at Low when budget < 20%, regardless of preference
        if budget_remaining_pct < 0.2 && base_effort != EffortLevel::Low {
            debug!(
                step = %step.name,
                budget_pct = budget_remaining_pct,
                original_effort = %base_effort,
                "effort capped to low due to budget constraint"
            );
            return RoutingDecision {
                provider,
                effort: EffortLevel::Low,
            };
        }

        RoutingDecision {
            provider,
            effort: base_effort,
        }
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
///
/// Role names are normalized to ASCII lowercase for case-insensitive matching.
fn model_for_role(role: &str) -> &'static str {
    let normalized: String = role.to_ascii_lowercase();
    match normalized.as_str() {
        // High reasoning tasks → opus
        "architect" | "planner" | "security-reviewer" | "senior" | "security_reviewer" | "lead"
        | "principal" => "opus",
        // Fast exploration → haiku
        "explorer" | "investigator" | "analyzer" | "searcher" | "scanner" | "indexer" => "haiku",
        // Implementation → sonnet (best cost/quality)
        "coder" | "implementer" | "frontend-dev" | "backend" | "tester" | "frontend_dev"
        | "developer" | "engineer" => "sonnet",
        // Review → sonnet (needs good judgment)
        "reviewer" | "code-reviewer" | "code_reviewer" => "sonnet",
        // Simple tasks → haiku
        "formatter" | "linter" => "haiku",
        // Debug → sonnet (needs reasoning + code understanding)
        "debugger" | "refactorer" => "sonnet",
        // DevOps → sonnet
        "devops" | "sre" | "infra" => "sonnet",
        // Default
        _ => "sonnet",
    }
}

/// Heuristic: map agent role name to recommended effort level.
///
/// Maps to Claude Code's `--effort low|medium|high` flag.
/// Role names are normalized to ASCII lowercase for case-insensitive matching.
///
/// NOTE: This is a best-effort heuristic — custom roles that don't match any
/// known pattern fall back to Medium. Add aliases here as new patterns emerge.
fn effort_for_role(role: &str) -> EffortLevel {
    let normalized: String = role.to_ascii_lowercase();
    match normalized.as_str() {
        // Deep reasoning → high effort
        "architect" | "planner" | "security-reviewer" | "senior" | "security_reviewer" | "lead"
        | "principal" => EffortLevel::High,
        // Thorough review needs high effort
        "reviewer" | "code-reviewer" | "code_reviewer" => EffortLevel::High,
        // Standard implementation → medium
        "coder" | "implementer" | "frontend-dev" | "backend" | "tester" | "frontend_dev"
        | "developer" | "engineer" => EffortLevel::Medium,
        // Debug needs reasoning
        "debugger" | "refactorer" => EffortLevel::Medium,
        // DevOps → medium
        "devops" | "sre" | "infra" => EffortLevel::Medium,
        // Fast tasks → low effort
        "explorer" | "investigator" | "analyzer" | "searcher" | "scanner" | "indexer" => {
            EffortLevel::Low
        }
        "formatter" | "linter" => EffortLevel::Low,
        // Default
        _ => EffortLevel::Medium,
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
        let step =
            PlanStep::new("explore", "Search codebase").with_role(AgentRole::new("explorer"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "haiku");
    }

    #[test]
    fn heuristic_selects_sonnet_for_coder() {
        let router = make_router();
        let step = PlanStep::new("implement", "Write code").with_role(AgentRole::new("coder"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "sonnet");
    }

    #[test]
    fn heuristic_selects_opus_for_architect() {
        let router = make_router();
        let step = PlanStep::new("design", "System design").with_role(AgentRole::new("architect"));

        let provider = router.for_step(&step, 1.0);
        assert_eq!(provider.model_name(), "opus");
    }

    #[test]
    fn budget_downgrade_to_haiku() {
        let router = make_router();
        let step = PlanStep::new("implement", "Write code").with_role(AgentRole::new("coder"));

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
        let step =
            PlanStep::new("mystery", "Unknown role").with_role(AgentRole::new("some-custom-role"));

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

    // -----------------------------------------------------------------------
    // route_step — effort routing tests
    // -----------------------------------------------------------------------

    #[test]
    fn route_step_returns_high_effort_for_architect() {
        let router = make_router();
        let step = PlanStep::new("design", "System design").with_role(AgentRole::new("architect"));

        let decision = router.route_step(&step, 1.0);
        assert_eq!(decision.provider.model_name(), "opus");
        assert_eq!(decision.effort, EffortLevel::High);
    }

    #[test]
    fn route_step_returns_low_effort_for_explorer() {
        let router = make_router();
        let step =
            PlanStep::new("explore", "Search codebase").with_role(AgentRole::new("explorer"));

        let decision = router.route_step(&step, 1.0);
        assert_eq!(decision.provider.model_name(), "haiku");
        assert_eq!(decision.effort, EffortLevel::Low);
    }

    #[test]
    fn route_step_returns_medium_effort_for_coder() {
        let router = make_router();
        let step = PlanStep::new("implement", "Write code").with_role(AgentRole::new("coder"));

        let decision = router.route_step(&step, 1.0);
        assert_eq!(decision.provider.model_name(), "sonnet");
        assert_eq!(decision.effort, EffortLevel::Medium);
    }

    #[test]
    fn route_step_preferred_effort_overrides_heuristic() {
        let router = make_router();
        let step = PlanStep::new("careful-impl", "Critical implementation")
            .with_role(AgentRole::new("coder").with_effort(EffortLevel::High));

        let decision = router.route_step(&step, 1.0);
        assert_eq!(decision.provider.model_name(), "sonnet");
        assert_eq!(decision.effort, EffortLevel::High);
    }

    #[test]
    fn route_step_budget_downgrades_effort() {
        let router = make_router();
        let step = PlanStep::new("design", "System design").with_role(AgentRole::new("architect"));

        // Normal budget → high effort (architect heuristic)
        assert_eq!(router.route_step(&step, 0.8).effort, EffortLevel::High);
        // Very low budget → low effort (override)
        assert_eq!(router.route_step(&step, 0.15).effort, EffortLevel::Low);
    }

    #[test]
    fn route_step_budget_caps_preferred_effort() {
        let router = make_router();
        let step = PlanStep::new("careful-impl", "Critical implementation")
            .with_role(AgentRole::new("coder").with_effort(EffortLevel::High));

        // With sufficient budget, preferred effort is honoured
        assert_eq!(router.route_step(&step, 0.5).effort, EffortLevel::High);
        // With very low budget, effort is capped to Low regardless of preference
        assert_eq!(router.route_step(&step, 0.15).effort, EffortLevel::Low);
    }

    #[test]
    fn effort_for_role_case_insensitive() {
        assert_eq!(effort_for_role("Architect"), EffortLevel::High);
        assert_eq!(effort_for_role("CODER"), EffortLevel::Medium);
        assert_eq!(effort_for_role("Explorer"), EffortLevel::Low);
    }

    #[test]
    fn model_for_role_case_insensitive() {
        assert_eq!(model_for_role("Architect"), "opus");
        assert_eq!(model_for_role("CODER"), "sonnet");
        assert_eq!(model_for_role("Explorer"), "haiku");
    }

    #[test]
    fn effort_for_role_coverage() {
        assert_eq!(effort_for_role("architect"), EffortLevel::High);
        assert_eq!(effort_for_role("security-reviewer"), EffortLevel::High);
        assert_eq!(effort_for_role("reviewer"), EffortLevel::High);
        assert_eq!(effort_for_role("coder"), EffortLevel::Medium);
        assert_eq!(effort_for_role("debugger"), EffortLevel::Medium);
        assert_eq!(effort_for_role("explorer"), EffortLevel::Low);
        assert_eq!(effort_for_role("formatter"), EffortLevel::Low);
        assert_eq!(effort_for_role("unknown"), EffortLevel::Medium);
    }
}
