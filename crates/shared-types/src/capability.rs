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

// ---------------------------------------------------------------------------
// ExecutionPhase — phase-aware tool palette (#63)
// ---------------------------------------------------------------------------

/// Current phase of task execution. Controls which tools are surfaced.
/// Improving an agent = removing bad options at the right moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPhase {
    /// Exploration: searching, reading, understanding the codebase.
    Explore,
    /// Planning: designing the approach, impact analysis.
    Plan,
    /// Implementation: writing code, editing files.
    Implement,
    /// Verification: running tests, builds, lints.
    Verify,
    /// Review: checking work quality, security scanning.
    Review,
}

impl ExecutionPhase {
    /// Capability categories relevant to this phase.
    pub fn relevant_capabilities(&self) -> &[&str] {
        match self {
            Self::Explore => &[
                "code_search",
                "file_read",
                "symbol_lookup",
                "dependency_trace",
            ],
            Self::Plan => &["code_search", "file_read", "impact_analysis", "code_review"],
            Self::Implement => &["file_edit", "shell_exec", "code_search", "file_read"],
            Self::Verify => &["test_run", "build", "lint", "typecheck", "shell_exec"],
            Self::Review => &["code_review", "security_scan", "file_read", "code_search"],
        }
    }

    /// Capability categories explicitly excluded in this phase.
    pub fn excluded_capabilities(&self) -> &[&str] {
        match self {
            Self::Explore => &["file_edit", "destructive"],
            Self::Plan => &["file_edit", "destructive"],
            Self::Implement => &["destructive"],
            Self::Verify => &["file_edit", "destructive"],
            Self::Review => &["file_edit", "destructive"],
        }
    }
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

    /// Generate a JSON Schema tool definition compatible with Claude Code's ToolSearch.
    ///
    /// Maps OCO capabilities to the MCP tool schema format used by deferred tools.
    /// The schema is minimal (name + description + empty input) — the full schema
    /// is resolved when the tool is actually fetched via ToolSearch.
    ///
    /// **ID encoding**: `:` → `__` (double underscore), single `_` preserved as-is.
    /// This is bijective as long as IDs don't contain `__` (which they shouldn't by convention).
    pub fn to_tool_schema(&self) -> serde_json::Value {
        debug_assert!(
            !self.id.contains("__"),
            "Capability ID must not contain `__` — reserved as separator: {}",
            self.id
        );
        let tool_name = format!("oco_{}", self.id.replace(':', "__"));
        let description = format!(
            "{} — {}",
            sanitize_for_prompt(&self.name),
            self.capabilities
                .iter()
                .map(|c| sanitize_for_prompt(c))
                .collect::<Vec<_>>()
                .join(", ")
        );

        serde_json::json!({
            "name": tool_name,
            "description": description,
            "inputSchema": self.input_schema_for_kind(),
        })
    }

    /// Generate a kind-specific input schema for tool definitions.
    ///
    /// All interpolated values are sanitized before inclusion in the schema
    /// to prevent prompt injection via external metadata (MCP server names, etc.).
    fn input_schema_for_kind(&self) -> serde_json::Value {
        match &self.kind {
            CapabilityKind::Tool { .. } => serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Input for the tool" }
                },
                "required": ["input"],
                "additionalProperties": false
            }),
            CapabilityKind::McpTool { server, tool_name } => serde_json::json!({
                "type": "object",
                "properties": {
                    "arguments": {
                        "type": "object",
                        "description": format!(
                            "Arguments for {}:{}",
                            sanitize_for_prompt(server),
                            sanitize_for_prompt(tool_name)
                        )
                    }
                },
                "required": ["arguments"],
                "additionalProperties": false
            }),
            CapabilityKind::Agent { agent_type, .. } => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": format!(
                            "Task for {} agent",
                            sanitize_for_prompt(agent_type)
                        )
                    }
                },
                "required": ["task"],
                "additionalProperties": false
            }),
            CapabilityKind::Skill { skill_name } => serde_json::json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "string",
                        "description": format!(
                            "Arguments for /{}",
                            sanitize_for_prompt(skill_name)
                        )
                    }
                },
                "additionalProperties": false
            }),
            CapabilityKind::LlmProvider { provider, model } => serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": format!(
                            "Prompt for {}/{}",
                            sanitize_for_prompt(provider),
                            sanitize_for_prompt(model)
                        )
                    }
                },
                "required": ["prompt"],
                "additionalProperties": false
            }),
        }
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

        // Deterministic sort: score desc → cost tier asc → id asc (fix #20)
        matches.sort_by(|a, b| {
            b.selection_score()
                .partial_cmp(&a.selection_score())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cost.tier.cmp(&b.cost.tier))
                .then_with(|| a.id.cmp(&b.id))
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

    /// Return capabilities filtered for a specific execution phase (#63).
    /// Includes capabilities relevant to the phase, excludes those that shouldn't
    /// be available (e.g., file_edit during explore phase).
    pub fn for_phase(&self, phase: ExecutionPhase) -> Vec<&CapabilityDescriptor> {
        let relevant = phase.relevant_capabilities();
        let excluded = phase.excluded_capabilities();

        self.entries
            .values()
            .filter(|d| {
                d.available
                    && d.capabilities
                        .iter()
                        .any(|c| relevant.contains(&c.as_str()))
                    && !d
                        .capabilities
                        .iter()
                        .any(|c| excluded.contains(&c.as_str()))
            })
            .collect()
    }

    /// Return capability IDs for a phase — suitable for tool palette injection.
    pub fn phase_tool_ids(&self, phase: ExecutionPhase) -> Vec<String> {
        self.for_phase(phase).iter().map(|d| d.id.clone()).collect()
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
    pub fn register_llm(&mut self, provider: &str, model: &str, cost_tier: CostTier) {
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

    /// List tool names for deferred tool registration (Claude Code ToolSearch).
    ///
    /// Returns just the tool names — the full schemas are fetched on demand
    /// via `to_tool_schemas()` when ToolSearch resolves a match.
    pub fn deferred_tool_names(&self) -> Vec<String> {
        self.entries
            .values()
            .filter(|d| d.available)
            .map(|d| format!("oco_{}", d.id.replace(':', "__")))
            .collect()
    }

    /// Generate full JSON Schema definitions for all available capabilities.
    ///
    /// Used by the mcp-server's `/tools/deferred` endpoint to resolve
    /// deferred tool schemas on demand.
    pub fn to_tool_schemas(&self) -> Vec<serde_json::Value> {
        self.entries
            .values()
            .filter(|d| d.available)
            .map(|d| d.to_tool_schema())
            .collect()
    }

    /// Resolve a single deferred tool by name, returning its full schema.
    ///
    /// Returns `None` if the name doesn't start with `oco_` — all OCO deferred
    /// tools use this prefix by convention.
    pub fn resolve_deferred_tool(&self, tool_name: &str) -> Option<serde_json::Value> {
        let suffix = tool_name.strip_prefix("oco_")?;
        let target_id = suffix.replace("__", ":");

        self.entries
            .values()
            .find(|d| d.available && d.id == target_id)
            .map(|d| d.to_tool_schema())
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
                id: sanitize_for_prompt(&entry.id),
                name: sanitize_for_prompt(&entry.name),
                capabilities: entry
                    .capabilities
                    .iter()
                    .map(|c| sanitize_for_prompt(c))
                    .collect(),
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
///
/// **Security note**: all fields are sanitized before injection into LLM prompts.
/// External metadata (MCP tool names, agent descriptions) is treated as untrusted.
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

    /// Check if any item across all categories has a matching capability string.
    ///
    /// Performs case-insensitive substring matching against item IDs, names,
    /// and declared capabilities. Broad search — use `has_tool_capability()`
    /// for precise gating on executable tools only.
    pub fn has_capability(&self, cap: &str) -> bool {
        let cap_lower = cap.to_lowercase();
        self.all_items().any(|item| {
            item.id.to_lowercase().contains(&cap_lower)
                || item.name.to_lowercase().contains(&cap_lower)
                || item
                    .capabilities
                    .iter()
                    .any(|c| c.to_lowercase().contains(&cap_lower))
        })
    }

    /// Check if any executable tool (local tool or MCP tool) declares a
    /// capability matching the given string exactly (case-insensitive).
    ///
    /// Unlike `has_capability()`, this:
    /// - Only checks `tools` and `mcp` categories (not agents/skills/llms).
    /// - Only matches the declared `capabilities` field (not id/name).
    /// - Uses exact equality, not substring matching.
    ///
    /// Use this for hard gating decisions (e.g., "can we actually run a search?").
    pub fn has_tool_capability(&self, cap: &str) -> bool {
        let cap_lower = cap.to_lowercase();
        self.tools.iter().chain(&self.mcp).any(|item| {
            item.capabilities
                .iter()
                .any(|c| c.to_lowercase() == cap_lower)
        })
    }

    /// Return the first matching capability string found in tools/MCP.
    /// Useful to discover the actual capability name for prompt injection.
    pub fn find_tool_capability(&self, candidates: &[&str]) -> Option<String> {
        for cap in candidates {
            let cap_lower = cap.to_lowercase();
            for item in self.tools.iter().chain(&self.mcp) {
                if let Some(found) = item
                    .capabilities
                    .iter()
                    .find(|c| c.to_lowercase() == cap_lower)
                {
                    return Some(found.clone());
                }
            }
        }
        None
    }

    /// Iterate over all summary items across every category.
    fn all_items(&self) -> impl Iterator<Item = &SummaryItem> {
        self.tools
            .iter()
            .chain(&self.mcp)
            .chain(&self.agents)
            .chain(&self.skills)
            .chain(&self.llms)
    }
}

/// A single entry in the registry summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryItem {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

/// Max length for a single field injected into planner prompts.
const SUMMARY_FIELD_MAX_LEN: usize = 100;

/// Sanitize a string for safe injection into LLM prompts.
/// Strips control characters, backtick fences, and truncates to max length.
pub fn sanitize_for_prompt(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    // Strip markdown code fences and system-like tags
    let cleaned = cleaned
        .replace("```", "")
        .replace("<system", "&lt;system")
        .replace("</system", "&lt;/system");
    if cleaned.len() > SUMMARY_FIELD_MAX_LEN {
        format!("{}…", &cleaned[..SUMMARY_FIELD_MAX_LEN])
    } else {
        cleaned
    }
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
        reg.register_skill(
            "review",
            vec!["code_review".into(), "security".into()],
            CostTier::Moderate,
        );
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

    #[test]
    fn best_for_deterministic_on_tied_scores() {
        let mut reg = CapabilityRegistry::new();

        // Two capabilities with identical proficiency and cost → same score
        for id in ["beta", "alpha"] {
            reg.register(CapabilityDescriptor {
                id: id.into(),
                name: id.into(),
                kind: CapabilityKind::Tool {
                    executor: id.into(),
                },
                capabilities: vec!["search".into()],
                cost: CapabilityCost::new(CostTier::Free),
                constraints: Vec::new(),
                proficiency: 0.8,
                available: true,
            });
        }

        // Must always return "alpha" (lexicographic tie-break on id)
        for _ in 0..10 {
            let best = reg.best_for("search").unwrap();
            assert_eq!(best.id, "alpha", "tie-breaking must be deterministic by id");
        }
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

    // -- tool schema generation --

    #[test]
    fn to_tool_schema_for_tool() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false)]);

        let schema = reg.get("tool:grep").unwrap().to_tool_schema();
        assert_eq!(schema["name"], "oco_tool__grep");
        assert!(schema["description"].as_str().unwrap().contains("grep"));
        assert!(schema["inputSchema"]["type"] == "object");
    }

    #[test]
    fn to_tool_schema_for_mcp_tool() {
        let mut reg = CapabilityRegistry::new();
        reg.register_mcp_tool("yoyo", "search", vec!["code_search".into()], "Yoyo search");

        let schema = reg.get("mcp:yoyo:search").unwrap().to_tool_schema();
        assert_eq!(schema["name"], "oco_mcp__yoyo__search");
    }

    #[test]
    fn deferred_tool_names_lists_available() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false)]);
        reg.register_skill("review", vec![], CostTier::Moderate);
        reg.register_skill("dead", vec![], CostTier::Free);
        reg.mark_unavailable("skill:dead");

        let names = reg.deferred_tool_names();
        assert_eq!(names.len(), 2); // grep + review (not dead)
        assert!(names.contains(&"oco_tool__grep".to_string()));
        assert!(names.contains(&"oco_skill__review".to_string()));
    }

    #[test]
    fn to_tool_schemas_generates_all() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false)]);
        reg.register_mcp_tool("yoyo", "search", vec![], "Search");

        let schemas = reg.to_tool_schemas();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.iter().all(|s| s["name"].is_string()));
    }

    #[test]
    fn resolve_deferred_tool_by_name() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill("review", vec!["code_review".into()], CostTier::Moderate);

        let schema = reg.resolve_deferred_tool("oco_skill__review");
        assert!(schema.is_some());
        assert_eq!(schema.unwrap()["name"], "oco_skill__review");
    }

    #[test]
    fn resolve_deferred_tool_missing_returns_none() {
        let reg = CapabilityRegistry::new();
        assert!(reg.resolve_deferred_tool("oco_nonexistent").is_none());
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

    // -- bijective ID mapping --

    #[test]
    fn id_mapping_round_trip_bijective() {
        // IDs that previously collided with single `_` separator
        let ids = vec![
            "tool:grep",
            "tool:file_read",  // contains underscore
            "mcp:yoyo:search", // multiple colons
            "llm:anthropic:opus",
            "skill:oco-verify-fix", // contains hyphen
        ];

        for id in &ids {
            // Encode: `:` → `__`
            let encoded = format!("oco_{}", id.replace(':', "__"));
            // Decode: `__` → `:`
            let decoded = encoded.strip_prefix("oco_").unwrap().replace("__", ":");
            assert_eq!(
                &decoded, id,
                "Round-trip failed for ID '{id}': encoded='{encoded}', decoded='{decoded}'"
            );
        }
    }

    #[test]
    fn id_mapping_no_collision_underscore_vs_colon() {
        // `tool:file_read` and a hypothetical `tool:file:read` must produce different names
        let id_a = "tool:file_read";
        let id_b = "tool:file:read";
        let encoded_a = format!("oco_{}", id_a.replace(':', "__"));
        let encoded_b = format!("oco_{}", id_b.replace(':', "__"));
        // oco_tool__file_read vs oco_tool__file__read — different!
        assert_ne!(
            encoded_a, encoded_b,
            "Collision detected: '{id_a}' and '{id_b}' both encode to '{encoded_a}'"
        );
    }

    #[test]
    fn resolve_deferred_tool_rejects_no_prefix() {
        let mut reg = CapabilityRegistry::new();
        reg.register_skill("review", vec!["code_review".into()], CostTier::Moderate);

        // Without `oco_` prefix → None
        assert!(reg.resolve_deferred_tool("skill__review").is_none());
        assert!(reg.resolve_deferred_tool("review").is_none());
        assert!(reg.resolve_deferred_tool("").is_none());
    }

    #[test]
    fn resolve_deferred_tool_round_trip_with_registry() {
        let mut reg = CapabilityRegistry::new();
        reg.register_mcp_tool("yoyo", "search", vec!["code_search".into()], "Search");
        reg.discover_tools(&[make_tool("file_read", false)]);

        // Every deferred name must resolve back to a valid schema
        for name in reg.deferred_tool_names() {
            let schema = reg.resolve_deferred_tool(&name);
            assert!(
                schema.is_some(),
                "deferred name '{name}' did not resolve back to a schema"
            );
            assert_eq!(
                schema.unwrap()["name"].as_str().unwrap(),
                name,
                "schema name mismatch for deferred name '{name}'"
            );
        }
    }

    #[test]
    fn schema_has_additional_properties_false() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false)]);
        reg.register_mcp_tool("yoyo", "search", vec![], "Search");
        reg.register_skill("review", vec![], CostTier::Moderate);
        reg.register_llm("anthropic", "opus", CostTier::Expensive);

        // Agent
        reg.register(CapabilityDescriptor {
            id: "agent:reviewer".into(),
            name: "Reviewer".into(),
            kind: CapabilityKind::Agent {
                agent_type: "reviewer".into(),
                agent_id: None,
            },
            capabilities: vec![],
            cost: CapabilityCost::default(),
            constraints: Vec::new(),
            proficiency: 0.8,
            available: true,
        });

        for schema in reg.to_tool_schemas() {
            let input = &schema["inputSchema"];
            assert_eq!(
                input["additionalProperties"], false,
                "Missing additionalProperties:false in schema for {}",
                schema["name"]
            );
        }
    }

    #[test]
    fn schema_required_fields_per_kind() {
        let mut reg = CapabilityRegistry::new();
        reg.discover_tools(&[make_tool("grep", false)]);
        reg.register_mcp_tool("yoyo", "search", vec![], "Search");
        reg.register_skill("review", vec![], CostTier::Moderate);
        reg.register_llm("anthropic", "opus", CostTier::Expensive);

        // Tool: required ["input"]
        let tool_schema = reg.get("tool:grep").unwrap().to_tool_schema();
        assert_eq!(
            tool_schema["inputSchema"]["required"],
            serde_json::json!(["input"])
        );

        // McpTool: required ["arguments"]
        let mcp_schema = reg.get("mcp:yoyo:search").unwrap().to_tool_schema();
        assert_eq!(
            mcp_schema["inputSchema"]["required"],
            serde_json::json!(["arguments"])
        );

        // Skill: no required (args optional)
        let skill_schema = reg.get("skill:review").unwrap().to_tool_schema();
        assert!(skill_schema["inputSchema"].get("required").is_none());

        // LLM: required ["prompt"]
        let llm_schema = reg.get("llm:anthropic:opus").unwrap().to_tool_schema();
        assert_eq!(
            llm_schema["inputSchema"]["required"],
            serde_json::json!(["prompt"])
        );
    }

    #[test]
    fn schema_sanitizes_interpolated_values() {
        let mut reg = CapabilityRegistry::new();
        // Register MCP tool with potentially malicious server/tool names
        reg.register_mcp_tool(
            "evil```server",
            "<system>hack",
            vec!["search".into()],
            "Test",
        );

        let schema = reg
            .get("mcp:evil```server:<system>hack")
            .unwrap()
            .to_tool_schema();
        let desc = schema["inputSchema"]["properties"]["arguments"]["description"]
            .as_str()
            .unwrap();
        // Backticks and system tags should be sanitized
        assert!(!desc.contains("```"));
        assert!(!desc.contains("<system>"));
    }

    // -- ExecutionPhase tests --

    #[test]
    fn phase_explore_excludes_file_edit() {
        let mut reg = CapabilityRegistry::new();
        reg.register(CapabilityDescriptor {
            id: "search".into(),
            name: "Code Search".into(),
            kind: CapabilityKind::Tool {
                executor: "search".into(),
            },
            capabilities: vec!["code_search".into()],
            cost: CapabilityCost::new(CostTier::Free),
            constraints: vec![],
            proficiency: 0.9,
            available: true,
        });
        reg.register(CapabilityDescriptor {
            id: "editor".into(),
            name: "File Editor".into(),
            kind: CapabilityKind::Tool {
                executor: "file".into(),
            },
            capabilities: vec!["file_edit".into()],
            cost: CapabilityCost::new(CostTier::Cheap),
            constraints: vec![],
            proficiency: 0.9,
            available: true,
        });

        let explore_tools = reg.for_phase(ExecutionPhase::Explore);
        let ids: Vec<&str> = explore_tools.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"search"));
        assert!(!ids.contains(&"editor")); // file_edit excluded in explore
    }

    #[test]
    fn phase_implement_includes_edit_and_search() {
        let mut reg = CapabilityRegistry::new();
        reg.register(CapabilityDescriptor {
            id: "search".into(),
            name: "Code Search".into(),
            kind: CapabilityKind::Tool {
                executor: "search".into(),
            },
            capabilities: vec!["code_search".into()],
            cost: CapabilityCost::new(CostTier::Free),
            constraints: vec![],
            proficiency: 0.9,
            available: true,
        });
        reg.register(CapabilityDescriptor {
            id: "editor".into(),
            name: "File Editor".into(),
            kind: CapabilityKind::Tool {
                executor: "file".into(),
            },
            capabilities: vec!["file_edit".into()],
            cost: CapabilityCost::new(CostTier::Cheap),
            constraints: vec![],
            proficiency: 0.9,
            available: true,
        });

        let impl_tools = reg.for_phase(ExecutionPhase::Implement);
        let ids: Vec<&str> = impl_tools.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"search"));
        assert!(ids.contains(&"editor"));
    }

    #[test]
    fn phase_verify_excludes_edit() {
        let mut reg = CapabilityRegistry::new();
        reg.register(CapabilityDescriptor {
            id: "test_runner".into(),
            name: "Test Runner".into(),
            kind: CapabilityKind::Tool {
                executor: "shell".into(),
            },
            capabilities: vec!["test_run".into(), "shell_exec".into()],
            cost: CapabilityCost::new(CostTier::Moderate),
            constraints: vec![],
            proficiency: 0.9,
            available: true,
        });
        reg.register(CapabilityDescriptor {
            id: "editor".into(),
            name: "File Editor".into(),
            kind: CapabilityKind::Tool {
                executor: "file".into(),
            },
            capabilities: vec!["file_edit".into()],
            cost: CapabilityCost::new(CostTier::Cheap),
            constraints: vec![],
            proficiency: 0.9,
            available: true,
        });

        let verify_tools = reg.for_phase(ExecutionPhase::Verify);
        let ids: Vec<&str> = verify_tools.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"test_runner"));
        assert!(!ids.contains(&"editor"));
    }

    #[test]
    fn phase_tool_ids_returns_strings() {
        let reg = CapabilityRegistry::new();
        let ids = reg.phase_tool_ids(ExecutionPhase::Explore);
        assert!(ids.is_empty()); // empty registry
    }

    #[test]
    fn execution_phase_relevant_capabilities() {
        assert!(
            ExecutionPhase::Explore
                .relevant_capabilities()
                .contains(&"code_search")
        );
        assert!(
            ExecutionPhase::Implement
                .relevant_capabilities()
                .contains(&"file_edit")
        );
        assert!(
            ExecutionPhase::Verify
                .relevant_capabilities()
                .contains(&"test_run")
        );
    }

    // -- RegistrySummary::has_capability --

    #[test]
    fn has_capability_matches_item_id() {
        let summary = RegistrySummary {
            tools: vec![SummaryItem {
                id: "web_search".into(),
                name: "Search".into(),
                capabilities: vec![],
            }],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(summary.has_capability("web_search"));
        assert!(summary.has_capability("Web_Search")); // case-insensitive
    }

    #[test]
    fn has_capability_matches_item_name() {
        let summary = RegistrySummary {
            tools: vec![SummaryItem {
                id: "tool:1".into(),
                name: "Web Search Tool".into(),
                capabilities: vec![],
            }],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(summary.has_capability("search")); // substring match: "Web Search Tool" contains "search"
        assert!(summary.has_capability("Web Search")); // case-insensitive substring
    }

    #[test]
    fn has_capability_matches_declared_capabilities() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![SummaryItem {
                id: "mcp:perplexity".into(),
                name: "Perplexity".into(),
                capabilities: vec!["web_search".into(), "research".into()],
            }],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(summary.has_capability("web_search"));
        assert!(summary.has_capability("research"));
    }

    #[test]
    fn has_capability_returns_false_when_absent() {
        let summary = RegistrySummary {
            tools: vec![SummaryItem {
                id: "file_read".into(),
                name: "File Read".into(),
                capabilities: vec!["read".into()],
            }],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(!summary.has_capability("web_search"));
    }

    #[test]
    fn has_capability_empty_registry() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(!summary.has_capability("anything"));
    }

    #[test]
    fn has_capability_searches_all_categories() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![],
            agents: vec![],
            skills: vec![SummaryItem {
                id: "research-deep".into(),
                name: "Deep Research".into(),
                capabilities: vec!["search".into()],
            }],
            llms: vec![],
        };
        assert!(summary.has_capability("search"));
    }

    // -- RegistrySummary::has_tool_capability --

    #[test]
    fn has_tool_capability_exact_match_on_tools() {
        let summary = RegistrySummary {
            tools: vec![SummaryItem {
                id: "perplexity".into(),
                name: "Perplexity Search".into(),
                capabilities: vec!["web_search".into()],
            }],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(summary.has_tool_capability("web_search"));
        assert!(summary.has_tool_capability("Web_Search")); // case-insensitive
        assert!(!summary.has_tool_capability("search")); // no substring: "web_search" != "search"
    }

    #[test]
    fn has_tool_capability_checks_mcp() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![SummaryItem {
                id: "mcp:perplexity".into(),
                name: "Perplexity".into(),
                capabilities: vec!["web_search".into(), "search".into()],
            }],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert!(summary.has_tool_capability("web_search"));
        assert!(summary.has_tool_capability("search"));
    }

    #[test]
    fn has_tool_capability_ignores_agents_skills_llms() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![],
            agents: vec![SummaryItem {
                id: "researcher".into(),
                name: "Deep Research Agent".into(),
                capabilities: vec!["search".into()],
            }],
            skills: vec![SummaryItem {
                id: "research-deep".into(),
                name: "Research".into(),
                capabilities: vec!["search".into()],
            }],
            llms: vec![],
        };
        // has_capability (broad) finds it
        assert!(summary.has_capability("search"));
        // has_tool_capability (strict) does NOT
        assert!(!summary.has_tool_capability("search"));
    }

    #[test]
    fn has_tool_capability_no_name_substring_match() {
        let summary = RegistrySummary {
            tools: vec![SummaryItem {
                id: "tool:1".into(),
                name: "Web Search Tool".into(),
                capabilities: vec!["file_read".into()],
            }],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        // has_capability (broad) matches on name
        assert!(summary.has_capability("search"));
        // has_tool_capability (strict) does NOT — only checks capabilities field
        assert!(!summary.has_tool_capability("search"));
    }

    // -- RegistrySummary::find_tool_capability --

    #[test]
    fn find_tool_capability_returns_first_match() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![SummaryItem {
                id: "mcp:perplexity".into(),
                name: "Perplexity".into(),
                capabilities: vec!["web_search".into()],
            }],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert_eq!(
            summary.find_tool_capability(&["web_search", "search"]),
            Some("web_search".into())
        );
    }

    #[test]
    fn find_tool_capability_falls_through_to_second() {
        let summary = RegistrySummary {
            tools: vec![SummaryItem {
                id: "tool:search".into(),
                name: "Search".into(),
                capabilities: vec!["search".into()],
            }],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        // "web_search" not found, falls through to "search"
        assert_eq!(
            summary.find_tool_capability(&["web_search", "search"]),
            Some("search".into())
        );
    }

    #[test]
    fn find_tool_capability_returns_none_when_absent() {
        let summary = RegistrySummary {
            tools: vec![],
            mcp: vec![],
            agents: vec![],
            skills: vec![],
            llms: vec![],
        };
        assert_eq!(
            summary.find_tool_capability(&["web_search", "search"]),
            None
        );
    }
}
