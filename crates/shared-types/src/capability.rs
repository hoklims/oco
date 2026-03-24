//! Unified capability registry — the "menu" of everything the orchestrator can use.
//!
//! Aggregates local tools, MCP tools, registered agents, skills, and LLM providers
//! into a single queryable catalog. The Planner reads this to generate `ExecutionPlan`
//! steps with appropriate tool/agent assignments.
//!
//! **Design principles:**
//! - Single source of truth for "what can I do right now?"
//! - Query by required capabilities, constraints, and cost tier
//! - Import from existing `ToolDescriptor`, `AgentDescriptor`, and MCP discovery
//! - Scoring: proficiency × (1 / cost) for `best_for` selection

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agent::{AgentDescriptor, AgentId};
use crate::tool::ToolDescriptor;

// ---------------------------------------------------------------------------
// CapabilityKind — what type of capability this is
// ---------------------------------------------------------------------------

/// The backing implementation behind a capability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapabilityKind {
    /// A local tool (shell, file_write, file_read, etc.).
    Tool {
        /// Executor identifier (e.g., "shell", "file", "lsp").
        executor: String,
    },
    /// An MCP-provided tool.
    McpTool {
        /// MCP server name or URL.
        server: String,
        /// Tool name on that server.
        tool_name: String,
    },
    /// A registered agent from AgentRegistry.
    Agent {
        /// Agent type key (e.g., "code-reviewer", "debugger").
        agent_type: String,
        /// Specific agent instance, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<AgentId>,
    },
    /// A skill (Claude Code skill or OCO skill).
    Skill {
        /// Skill identifier (e.g., "oco-investigate-bug", "code-review").
        skill_name: String,
    },
    /// An LLM provider.
    LlmProvider {
        /// Provider name (e.g., "anthropic", "ollama", "stub").
        provider: String,
        /// Model name (e.g., "opus", "sonnet", "haiku").
        model: String,
    },
}

// ---------------------------------------------------------------------------
// CostTier / CapabilityCost
// ---------------------------------------------------------------------------

/// Rough cost classification for budget-aware selection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CostTier {
    /// Free or negligible cost (local file read, cached lookup).
    Free,
    /// Low cost (fast LLM call, simple shell command).
    Cheap,
    /// Moderate cost (medium LLM call, MCP round-trip).
    Moderate,
    /// High cost (frontier model call, long-running process).
    Expensive,
}

/// Estimated cost of using a capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityCost {
    /// Estimated token consumption (input + output).
    pub estimated_tokens: u32,
    /// Estimated wall-clock time in milliseconds.
    pub estimated_duration_ms: u64,
    /// Cost classification.
    pub tier: CostTier,
}

impl Default for CapabilityCost {
    fn default() -> Self {
        Self {
            estimated_tokens: 0,
            estimated_duration_ms: 100,
            tier: CostTier::Free,
        }
    }
}

impl CapabilityCost {
    pub fn new(tier: CostTier) -> Self {
        match tier {
            CostTier::Free => Self {
                estimated_tokens: 0,
                estimated_duration_ms: 50,
                tier,
            },
            CostTier::Cheap => Self {
                estimated_tokens: 500,
                estimated_duration_ms: 500,
                tier,
            },
            CostTier::Moderate => Self {
                estimated_tokens: 2000,
                estimated_duration_ms: 3000,
                tier,
            },
            CostTier::Expensive => Self {
                estimated_tokens: 8000,
                estimated_duration_ms: 15000,
                tier,
            },
        }
    }

    /// Numeric cost score (0.0 = free, 1.0 = most expensive). Used in scoring.
    pub fn score(&self) -> f64 {
        match self.tier {
            CostTier::Free => 0.0,
            CostTier::Cheap => 0.25,
            CostTier::Moderate => 0.5,
            CostTier::Expensive => 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// CapabilityDescriptor — a single entry in the registry
// ---------------------------------------------------------------------------

/// A capability available to the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    /// Unique identifier (e.g., "tool:shell", "mcp:yoyo:search", "agent:code-reviewer").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What kind of capability this is.
    pub kind: CapabilityKind,
    /// What this capability can do (e.g., "code_search", "file_edit", "security_review").
    pub capabilities: Vec<String>,
    /// Estimated cost of invocation.
    pub cost: CapabilityCost,
    /// Constraints (e.g., "read_only", "requires_confirmation", "destructive").
    pub constraints: Vec<String>,
    /// Proficiency score (0.0 to 1.0) — how good at its declared capabilities.
    pub proficiency: f64,
    /// Whether this capability is currently reachable/healthy.
    pub available: bool,
}

impl CapabilityDescriptor {
    /// Composite score: proficiency weighted against cost. Higher = better choice.
    /// Formula: proficiency * 0.7 + (1.0 - cost_score) * 0.3
    pub fn selection_score(&self) -> f64 {
        if !self.available {
            return 0.0;
        }
        self.proficiency * 0.7 + (1.0 - self.cost.score()) * 0.3
    }

    /// Whether this descriptor satisfies ALL required capabilities.
    pub fn satisfies(&self, required: &[String]) -> bool {
        required.iter().all(|req| self.capabilities.contains(req))
    }

    /// Whether this descriptor respects ALL given constraints.
    /// A constraint like "read_only" means the capability must have "read_only" in its constraints.
    pub fn respects_constraints(&self, required_constraints: &[String]) -> bool {
        required_constraints
            .iter()
            .all(|c| self.constraints.contains(c))
    }
}

// ---------------------------------------------------------------------------
// CapabilityRegistry — the unified catalog
// ---------------------------------------------------------------------------

/// Unified catalog of all available capabilities (tools, MCP, agents, skills, LLMs).
///
/// **Not thread-safe.** Wrap in `Mutex` for concurrent access.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityRegistry {
    entries: HashMap<String, CapabilityDescriptor>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a capability. Overwrites if the ID already exists.
    pub fn register(&mut self, descriptor: CapabilityDescriptor) {
        self.entries.insert(descriptor.id.clone(), descriptor);
    }

    /// Remove a capability by ID.
    pub fn unregister(&mut self, id: &str) -> Option<CapabilityDescriptor> {
        self.entries.remove(id)
    }

    /// Get a capability by ID.
    pub fn get(&self, id: &str) -> Option<&CapabilityDescriptor> {
        self.entries.get(id)
    }

    /// Number of registered capabilities.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All registered capabilities.
    pub fn all(&self) -> Vec<&CapabilityDescriptor> {
        self.entries.values().collect()
    }

    /// Query capabilities that match ALL required capabilities and constraints.
    /// Returns matches sorted by selection_score descending.
    pub fn query(
        &self,
        required_capabilities: &[String],
        required_constraints: &[String],
    ) -> Vec<&CapabilityDescriptor> {
        let mut matches: Vec<&CapabilityDescriptor> = self
            .entries
            .values()
            .filter(|d| {
                d.available
                    && d.satisfies(required_capabilities)
                    && d.respects_constraints(required_constraints)
            })
            .collect();

        matches.sort_by(|a, b| {
            b.selection_score()
                .partial_cmp(&a.selection_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        matches
    }

    /// Find the single best capability for a given capability name.
    /// Shorthand for querying with a single required capability.
    pub fn best_for(&self, capability: &str) -> Option<&CapabilityDescriptor> {
        self.query(&[capability.to_string()], &[]).first().copied()
    }

    /// Query by kind — find all capabilities of a specific type.
    pub fn by_kind(&self, kind_type: &str) -> Vec<&CapabilityDescriptor> {
        self.entries
            .values()
            .filter(|d| {
                d.available
                    && match &d.kind {
                        CapabilityKind::Tool { .. } => kind_type == "tool",
                        CapabilityKind::McpTool { .. } => kind_type == "mcp",
                        CapabilityKind::Agent { .. } => kind_type == "agent",
                        CapabilityKind::Skill { .. } => kind_type == "skill",
                        CapabilityKind::LlmProvider { .. } => kind_type == "llm",
                    }
            })
            .collect()
    }

    /// Mark a capability as unavailable (e.g., MCP server went down).
    pub fn mark_unavailable(&mut self, id: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.available = false;
            true
        } else {
            false
        }
    }

    /// Mark a capability as available again.
    pub fn mark_available(&mut self, id: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.available = true;
            true
        } else {
            false
        }
    }

    // -- Discovery: import from existing registries --

    /// Import local tools from a slice of `ToolDescriptor`s.
    pub fn discover_tools(&mut self, tools: &[ToolDescriptor]) {
        for tool in tools {
            let mut capabilities = tool.tags.clone();
            if tool.is_write {
                capabilities.push("file_write".into());
            }
            if capabilities.is_empty() {
                capabilities.push(tool.name.clone());
            }

            let mut constraints = Vec::new();
            if !tool.is_write {
                constraints.push("read_only".into());
            }
            if tool.requires_confirmation {
                constraints.push("requires_confirmation".into());
            }

            let cost = if tool.is_write {
                CapabilityCost::new(CostTier::Cheap)
            } else {
                CapabilityCost::new(CostTier::Free)
            };

            self.register(CapabilityDescriptor {
                id: format!("tool:{}", tool.name),
                name: tool.name.clone(),
                kind: CapabilityKind::Tool {
                    executor: tool.name.clone(),
                },
                capabilities,
                cost,
                constraints,
                proficiency: 1.0, // local tools are maximally proficient
                available: true,
            });
        }
    }

    /// Import agents from a slice of `AgentDescriptor`s.
    pub fn discover_agents(&mut self, agents: &[AgentDescriptor]) {
        for agent in agents {
            let capabilities: Vec<String> =
                agent.capabilities.iter().map(|c| c.name.clone()).collect();

            let proficiency = if capabilities.is_empty() {
                0.5
            } else {
                // Average proficiency across all capabilities
                let sum: f64 = agent
                    .capabilities
                    .iter()
                    .map(|c| c.proficiency.unwrap_or(0.5))
                    .sum();
                sum / agent.capabilities.len() as f64
            };

            self.register(CapabilityDescriptor {
                id: format!("agent:{}", agent.agent_type),
                name: agent.name.clone(),
                kind: CapabilityKind::Agent {
                    agent_type: agent.agent_type.clone(),
                    agent_id: Some(agent.id),
                },
                capabilities,
                cost: CapabilityCost::new(CostTier::Moderate),
                constraints: Vec::new(),
                proficiency,
                available: agent.status == crate::agent::AgentStatus::Active,
            });
        }
    }

    /// Register an MCP tool manually (called during MCP server discovery).
    pub fn register_mcp_tool(
        &mut self,
        server: &str,
        tool_name: &str,
        capabilities: Vec<String>,
        description: &str,
    ) {
        self.register(CapabilityDescriptor {
            id: format!("mcp:{server}:{tool_name}"),
            name: format!("{server}/{tool_name}"),
            kind: CapabilityKind::McpTool {
                server: server.into(),
                tool_name: tool_name.into(),
            },
            capabilities,
            cost: CapabilityCost::new(CostTier::Cheap),
            constraints: Vec::new(),
            proficiency: 0.8, // MCP tools are generally reliable but not perfect
            available: true,
        });
        // Silence unused variable — description is for future use (e.g., semantic matching)
        let _ = description;
    }

    /// Register a skill.
    pub fn register_skill(
        &mut self,
        skill_name: &str,
        capabilities: Vec<String>,
        cost_tier: CostTier,
    ) {
        self.register(CapabilityDescriptor {
            id: format!("skill:{skill_name}"),
            name: skill_name.into(),
            kind: CapabilityKind::Skill {
                skill_name: skill_name.into(),
            },
            capabilities,
            cost: CapabilityCost::new(cost_tier),
            constraints: Vec::new(),
            proficiency: 0.9, // skills are curated, high proficiency
            available: true,
        });
    }

    /// Register an LLM provider.
    pub fn register_llm(
        &mut self,
        provider: &str,
        model: &str,
        cost_tier: CostTier,
    ) {
        self.register(CapabilityDescriptor {
            id: format!("llm:{provider}:{model}"),
            name: format!("{provider}/{model}"),
            kind: CapabilityKind::LlmProvider {
                provider: provider.into(),
                model: model.into(),
            },
            capabilities: vec!["llm_completion".into(), "reasoning".into()],
            cost: CapabilityCost::new(cost_tier),
            constraints: Vec::new(),
            proficiency: match model {
                "opus" => 1.0,
                "sonnet" => 0.85,
                "haiku" => 0.65,
                _ => 0.7,
            },
            available: true,
        });
    }

    /// Summary for context injection: list of capability IDs grouped by kind.
    /// Used by the Planner to know what's available.
    pub fn summary(&self) -> RegistrySummary {
        let mut tools = Vec::new();
        let mut mcp = Vec::new();
        let mut agents = Vec::new();
        let mut skills = Vec::new();
        let mut llms = Vec::new();

        for entry in self.entries.values() {
            if !entry.available {
                continue;
            }
            let item = SummaryItem {
                id: entry.id.clone(),
                name: entry.name.clone(),
                capabilities: entry.capabilities.clone(),
            };
            match &entry.kind {
                CapabilityKind::Tool { .. } => tools.push(item),
                CapabilityKind::McpTool { .. } => mcp.push(item),
                CapabilityKind::Agent { .. } => agents.push(item),
                CapabilityKind::Skill { .. } => skills.push(item),
                CapabilityKind::LlmProvider { .. } => llms.push(item),
            }
        }

        RegistrySummary {
            tools,
            mcp,
            agents,
            skills,
            llms,
        }
    }
}

/// Compact summary of available capabilities, for Planner context injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySummary {
    pub tools: Vec<SummaryItem>,
    pub mcp: Vec<SummaryItem>,
    pub agents: Vec<SummaryItem>,
    pub skills: Vec<SummaryItem>,
    pub llms: Vec<SummaryItem>,
}

impl RegistrySummary {
    /// Total count of available capabilities.
    pub fn total(&self) -> usize {
        self.tools.len() + self.mcp.len() + self.agents.len() + self.skills.len() + self.llms.len()
    }
}

/// A single entry in the registry summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryItem {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentStatus, Capability as AgentCapability};

    fn make_tool(name: &str, is_write: bool) -> ToolDescriptor {
        ToolDescriptor {
            name: name.into(),
            description: format!("{name} tool"),
            input_schema: serde_json::json!({}),
            is_write,
            requires_confirmation: is_write,
            timeout_secs: 30,
            tags: vec![name.into()],
        }
    }

    fn make_agent(name: &str, agent_type: &str, caps: &[(&str, f64)]) -> AgentDescriptor {
        let mut agent = AgentDescriptor::new(name, agent_type).with_capabilities(
            caps.iter()
                .map(|(n, p)| AgentCapability::new(*n).with_proficiency(*p))
                .collect(),
        );
        agent.status = AgentStatus::Active;
        agent
    }

    // -- Basic CRUD --

    #[test]
    fn register_and_get() {
        let mut reg = CapabilityRegistry::new();
        assert!(reg.is_empty());

        reg.register(CapabilityDescriptor {
            id: "test:1".into(),
            name: "Test".into(),
            kind: CapabilityKind::Tool {
                executor: "test".into(),
            },
            capabilities: vec!["testing".into()],
            cost: CapabilityCost::default(),
            constraints: Vec::new(),
            proficiency: 1.0,
            available: true,
        });

        assert_eq!(reg.len(), 1);
        assert!(reg.get("test:1").is_some());
        assert_eq!(reg.get("test:1").unwrap().name, "Test");
    }

    #[test]
    fn unregister() {
        let mut reg = CapabilityRegistry::new();
        reg.register(CapabilityDescriptor {
            id: "x".into(),
            name: "X".into(),
            kind: CapabilityKind::Skill {
                skill_name: "x".into(),
            },
            capabilities: vec![],
            cost: CapabilityCost::default(),
            constraints: Vec::new(),
            proficiency: 0.5,
            available: true,
        });

        assert!(reg.unregister("x").is_some());
        assert!(reg.is_empty());
        assert!(reg.unregister("x").is_none());
    }

    #[test]
    fn overwrite_on_duplicate_id() {
        let mut reg = CapabilityRegistry::new();
        reg.register(CapabilityDescriptor {
            id: "dup".into(),
            name: "V1".into(),
            kind: CapabilityKind::Tool {
                executor: "a".into(),
            },
            capabilities: vec![],
            cost: CapabilityCost::default(),
            constraints: Vec::new(),
            proficiency: 0.5,
            available: true,
        });
        reg.register(CapabilityDescriptor {
            id: "dup".into(),
            name: "V2".into(),
            kind: CapabilityKind::Tool {
                executor: "b".into(),
            },
            capabilities: vec![],
            cost: CapabilityCost::default(),
            constraints: Vec::new(),
            proficiency: 0.9,
            available: true,
        });

        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("dup").unwrap().name, "V2");
    }

    // -- Query --

    #[test]
    fn query_by_capability() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill("review", vec!["code_review".into(), "security".into()], CostTier::Moderate);
        reg.register_skill("tdd", vec!["testing".into()], CostTier::Cheap);

        let results = reg.query(&["code_review".into()], &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "skill:review");
    }

    #[test]
    fn query_multiple_capabilities() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill(
            "full-review",
            vec!["code_review".into(), "security".into()],
            CostTier::Expensive,
        );
        reg.register_skill("sec-only", vec!["security".into()], CostTier::Cheap);

        // Query requires BOTH — only full-review matches
        let results = reg.query(&["code_review".into(), "security".into()], &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "skill:full-review");
    }

    #[test]
    fn query_with_constraints() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false), make_tool("write_file", true)]);

        let results = reg.query(&[], &["read_only".into()]);
        assert_eq!(results.len(), 1);
        assert!(results[0].id.contains("grep"));
    }

    #[test]
    fn query_excludes_unavailable() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill("alive", vec!["test".into()], CostTier::Free);
        reg.register_skill("dead", vec!["test".into()], CostTier::Free);
        reg.mark_unavailable("skill:dead");

        let results = reg.query(&["test".into()], &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "skill:alive");
    }

    // -- best_for --

    #[test]
    fn best_for_prefers_high_proficiency_low_cost() {
        let mut reg = CapabilityRegistry::new();

        // High proficiency, moderate cost
        reg.register(CapabilityDescriptor {
            id: "a".into(),
            name: "Expert".into(),
            kind: CapabilityKind::Agent {
                agent_type: "expert".into(),
                agent_id: None,
            },
            capabilities: vec!["code_search".into()],
            cost: CapabilityCost::new(CostTier::Moderate),
            constraints: Vec::new(),
            proficiency: 0.95,
            available: true,
        });

        // Low proficiency, free cost
        reg.register(CapabilityDescriptor {
            id: "b".into(),
            name: "Basic".into(),
            kind: CapabilityKind::Tool {
                executor: "grep".into(),
            },
            capabilities: vec!["code_search".into()],
            cost: CapabilityCost::new(CostTier::Free),
            constraints: Vec::new(),
            proficiency: 0.5,
            available: true,
        });

        let best = reg.best_for("code_search").unwrap();
        // Expert: 0.95 * 0.7 + 0.5 * 0.3 = 0.665 + 0.15 = 0.815
        // Basic:  0.5 * 0.7 + 1.0 * 0.3 = 0.35 + 0.3 = 0.65
        assert_eq!(best.id, "a", "expert should win on proficiency");
    }

    // -- by_kind --

    #[test]
    fn by_kind_filters() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false)]);
        reg.register_skill("review", vec![], CostTier::Moderate);
        reg.register_mcp_tool("yoyo", "search", vec!["code_search".into()], "Search code");

        assert_eq!(reg.by_kind("tool").len(), 1);
        assert_eq!(reg.by_kind("skill").len(), 1);
        assert_eq!(reg.by_kind("mcp").len(), 1);
        assert_eq!(reg.by_kind("agent").len(), 0);
    }

    // -- discover_tools --

    #[test]
    fn discover_tools_imports() {
        let mut reg = CapabilityRegistry::new();
        let tools = vec![
            make_tool("shell", true),
            make_tool("file_read", false),
            make_tool("grep", false),
        ];
        reg.discover_tools(&tools);

        assert_eq!(reg.len(), 3);
        let shell = reg.get("tool:shell").unwrap();
        assert!(shell.capabilities.contains(&"file_write".to_string()));
        assert!(!shell.constraints.contains(&"read_only".to_string()));

        let grep = reg.get("tool:grep").unwrap();
        assert!(grep.constraints.contains(&"read_only".to_string()));
        assert_eq!(grep.proficiency, 1.0);
    }

    // -- discover_agents --

    #[test]
    fn discover_agents_imports() {
        let mut reg = CapabilityRegistry::new();
        let agents = vec![make_agent(
            "Reviewer",
            "code-reviewer",
            &[("code_review", 0.9), ("security_scan", 0.7)],
        )];
        reg.discover_agents(&agents);

        assert_eq!(reg.len(), 1);
        let entry = reg.get("agent:code-reviewer").unwrap();
        assert!(entry.capabilities.contains(&"code_review".to_string()));
        assert!((entry.proficiency - 0.8).abs() < f64::EPSILON); // avg(0.9, 0.7) = 0.8
        assert!(entry.available);
    }

    #[test]
    fn discover_agents_inactive_not_available() {
        let mut reg = CapabilityRegistry::new();
        let mut agent = make_agent("Dead", "dead-agent", &[("task", 0.5)]);
        agent.status = AgentStatus::Dead;
        reg.discover_agents(&[agent]);

        let entry = reg.get("agent:dead-agent").unwrap();
        assert!(!entry.available);
    }

    // -- MCP + Skill + LLM registration --

    #[test]
    fn register_mcp_tool() {
        let mut reg = CapabilityRegistry::new();
        reg.register_mcp_tool("yoyo", "search", vec!["code_search".into()], "Yoyo search");

        let entry = reg.get("mcp:yoyo:search").unwrap();
        assert_eq!(entry.proficiency, 0.8);
        assert!(entry.available);
        assert!(matches!(entry.kind, CapabilityKind::McpTool { .. }));
    }

    #[test]
    fn register_llm_providers() {
        let mut reg = CapabilityRegistry::new();
        reg.register_llm("anthropic", "opus", CostTier::Expensive);
        reg.register_llm("anthropic", "haiku", CostTier::Cheap);

        let opus = reg.get("llm:anthropic:opus").unwrap();
        assert_eq!(opus.proficiency, 1.0);
        assert_eq!(opus.cost.tier, CostTier::Expensive);

        let haiku = reg.get("llm:anthropic:haiku").unwrap();
        assert_eq!(haiku.proficiency, 0.65);
        assert_eq!(haiku.cost.tier, CostTier::Cheap);
    }

    // -- availability toggle --

    #[test]
    fn mark_unavailable_and_back() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill("x", vec!["test".into()], CostTier::Free);

        assert!(reg.mark_unavailable("skill:x"));
        assert!(!reg.get("skill:x").unwrap().available);
        assert!(reg.query(&["test".into()], &[]).is_empty());

        assert!(reg.mark_available("skill:x"));
        assert!(reg.get("skill:x").unwrap().available);
        assert_eq!(reg.query(&["test".into()], &[]).len(), 1);
    }

    #[test]
    fn mark_nonexistent_returns_false() {
        let mut reg = CapabilityRegistry::new();
        assert!(!reg.mark_unavailable("nope"));
        assert!(!reg.mark_available("nope"));
    }

    // -- summary --

    #[test]
    fn summary_groups_by_kind() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false), make_tool("shell", true)]);
        reg.register_mcp_tool("ctx7", "docs", vec![], "Get docs");
        reg.register_skill("review", vec![], CostTier::Moderate);
        reg.register_llm("anthropic", "sonnet", CostTier::Moderate);

        let summary = reg.summary();
        assert_eq!(summary.tools.len(), 2);
        assert_eq!(summary.mcp.len(), 1);
        assert_eq!(summary.skills.len(), 1);
        assert_eq!(summary.llms.len(), 1);
        assert_eq!(summary.agents.len(), 0);
        assert_eq!(summary.total(), 5);
    }

    #[test]
    fn summary_excludes_unavailable() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill("alive", vec![], CostTier::Free);
        reg.register_skill("dead", vec![], CostTier::Free);
        reg.mark_unavailable("skill:dead");

        let summary = reg.summary();
        assert_eq!(summary.skills.len(), 1);
        assert_eq!(summary.skills[0].id, "skill:alive");
    }

    // -- selection_score --

    #[test]
    fn selection_score_zero_when_unavailable() {
        let d = CapabilityDescriptor {
            id: "x".into(),
            name: "X".into(),
            kind: CapabilityKind::Tool {
                executor: "x".into(),
            },
            capabilities: vec![],
            cost: CapabilityCost::new(CostTier::Free),
            constraints: Vec::new(),
            proficiency: 1.0,
            available: false,
        };
        assert_eq!(d.selection_score(), 0.0);
    }

    // -- JSON round-trip --

    #[test]
    fn json_round_trip() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("shell", true)]);
        reg.register_mcp_tool("yoyo", "search", vec!["code_search".into()], "Search");
        reg.register_llm("anthropic", "opus", CostTier::Expensive);

        let json = serde_json::to_string_pretty(&reg).expect("serialize");
        let restored: CapabilityRegistry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.len(), 3);
    }

    // -- cost scoring --

    #[test]
    fn cost_tier_ordering() {
        assert!(CostTier::Free < CostTier::Cheap);
        assert!(CostTier::Cheap < CostTier::Moderate);
        assert!(CostTier::Moderate < CostTier::Expensive);
    }

    #[test]
    fn cost_score_values() {
        assert_eq!(CapabilityCost::new(CostTier::Free).score(), 0.0);
        assert_eq!(CapabilityCost::new(CostTier::Expensive).score(), 1.0);
    }
}
