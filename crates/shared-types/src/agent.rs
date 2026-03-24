//! Agent registry types: capabilities, heartbeat, load-aware dispatch.
//!
//! Supports multi-agent orchestration with:
//! - Capability-based routing (each agent declares what it can do)
//! - Heartbeat tracking with configurable timeout and `unresponsive_since`
//! - Load-aware dispatching (greedy min-load selection)
//! - Agent lifecycle management (spawn → active → unresponsive → dead)
//!
//! **Concurrency note**: `AgentRegistry` uses `&mut self` and is NOT thread-safe.
//! For async/multi-thread usage, wrap in `Mutex<AgentRegistry>` or use the
//! `reserve_agent()` method which atomically selects and increments load.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for an agent instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);

impl AgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Agent lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent is starting up.
    Spawning,
    /// Agent is ready and accepting tasks.
    Active,
    /// Agent missed heartbeat but hasn't timed out yet.
    Unresponsive,
    /// Agent is confirmed dead (timed out or crashed).
    Dead,
    /// Agent completed its work and shut down gracefully.
    Completed,
}

/// A capability that an agent can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// Capability name (e.g., "code_review", "shell_exec", "file_search").
    pub name: String,
    /// Optional proficiency level (0.0 to 1.0). Higher = better at this skill.
    pub proficiency: Option<f64>,
}

impl PartialEq for Capability {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Capability {}

impl std::hash::Hash for Capability {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Capability {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            proficiency: None,
        }
    }

    pub fn with_proficiency(mut self, proficiency: f64) -> Self {
        self.proficiency = Some(proficiency.clamp(0.0, 1.0));
        self
    }
}

/// Descriptor for a registered agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub id: AgentId,
    /// Human-readable name.
    pub name: String,
    /// Agent type (e.g., "code-reviewer", "debugger", "researcher").
    pub agent_type: String,
    /// What this agent can do.
    pub capabilities: Vec<Capability>,
    /// Current lifecycle status.
    pub status: AgentStatus,
    /// Last heartbeat timestamp.
    pub last_heartbeat: DateTime<Utc>,
    /// When this agent was spawned.
    pub spawned_at: DateTime<Utc>,
    /// When the agent became unresponsive (None if responsive).
    pub unresponsive_since: Option<DateTime<Utc>>,
    /// Number of tasks currently assigned.
    pub current_load: u32,
    /// Maximum concurrent tasks this agent can handle.
    pub max_load: u32,
    /// Cumulative tasks completed.
    pub tasks_completed: u32,
    /// Cumulative tasks failed.
    pub tasks_failed: u32,
}

impl AgentDescriptor {
    pub fn new(name: impl Into<String>, agent_type: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AgentId::new(),
            name: name.into(),
            agent_type: agent_type.into(),
            capabilities: Vec::new(),
            status: AgentStatus::Spawning,
            last_heartbeat: now,
            spawned_at: now,
            unresponsive_since: None,
            current_load: 0,
            max_load: 5,
            tasks_completed: 0,
            tasks_failed: 0,
        }
    }

    pub fn with_capabilities(mut self, capabilities: Vec<Capability>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_max_load(mut self, max_load: u32) -> Self {
        self.max_load = max_load;
        self
    }

    /// Record a heartbeat.
    pub fn heartbeat(&mut self) {
        self.last_heartbeat = Utc::now();
        if self.status == AgentStatus::Unresponsive {
            self.status = AgentStatus::Active;
            self.unresponsive_since = None;
        }
    }

    /// Check if the agent is responsive given a timeout in seconds.
    pub fn is_responsive(&self, timeout_secs: i64) -> bool {
        let elapsed = (Utc::now() - self.last_heartbeat).num_seconds();
        elapsed < timeout_secs
    }

    /// Whether this agent can accept more tasks.
    pub fn has_capacity(&self) -> bool {
        self.status == AgentStatus::Active && self.current_load < self.max_load
    }

    /// Whether this agent has a specific capability.
    pub fn can_execute(&self, capability_name: &str) -> bool {
        self.capabilities.iter().any(|c| c.name == capability_name)
    }

    /// Get proficiency for a capability (defaults to 0.5 if not specified).
    pub fn proficiency_for(&self, capability_name: &str) -> f64 {
        self.capabilities
            .iter()
            .find(|c| c.name == capability_name)
            .and_then(|c| c.proficiency)
            .unwrap_or(0.5)
    }

    /// Success rate = completed / (completed + failed). Returns 1.0 if no tasks.
    pub fn success_rate(&self) -> f64 {
        let total = self.tasks_completed + self.tasks_failed;
        if total == 0 {
            1.0
        } else {
            self.tasks_completed as f64 / total as f64
        }
    }
}

/// Agent registry — manages all known agents and provides dispatch.
///
/// **Not thread-safe.** Wrap in `Mutex<AgentRegistry>` for concurrent access.
/// Use `reserve_agent()` instead of `select_agent()` + manual load increment
/// to avoid TOCTOU race conditions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRegistry {
    agents: HashMap<AgentId, AgentDescriptor>,
    /// Heartbeat timeout in seconds. Default: 60.
    pub heartbeat_timeout_secs: i64,
    /// Grace period after becoming unresponsive before marking dead. Default: 60.
    pub dead_grace_period_secs: i64,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            heartbeat_timeout_secs: 60,
            dead_grace_period_secs: 60,
        }
    }

    /// Register a new agent. Returns its ID.
    pub fn register(&mut self, mut agent: AgentDescriptor) -> AgentId {
        agent.status = AgentStatus::Active;
        agent.heartbeat();
        let id = agent.id;
        self.agents.insert(id, agent);
        id
    }

    /// Remove an agent from the registry.
    pub fn unregister(&mut self, id: AgentId) -> Option<AgentDescriptor> {
        self.agents.remove(&id)
    }

    /// Get an agent by ID.
    pub fn get(&self, id: AgentId) -> Option<&AgentDescriptor> {
        self.agents.get(&id)
    }

    /// Get a mutable reference to an agent.
    pub fn get_mut(&mut self, id: AgentId) -> Option<&mut AgentDescriptor> {
        self.agents.get_mut(&id)
    }

    /// Record a heartbeat for an agent.
    pub fn record_heartbeat(&mut self, id: AgentId) -> bool {
        if let Some(agent) = self.agents.get_mut(&id) {
            agent.heartbeat();
            true
        } else {
            false
        }
    }

    /// Check all agents for heartbeat timeout. Marks unresponsive agents.
    /// Uses `dead_grace_period_secs` to determine when Unresponsive → Dead.
    /// Returns IDs of agents that transitioned to Dead.
    pub fn check_health(&mut self) -> Vec<AgentId> {
        let timeout = self.heartbeat_timeout_secs;
        let grace = self.dead_grace_period_secs;
        let now = Utc::now();
        let mut dead_agents = Vec::new();

        for agent in self.agents.values_mut() {
            if !agent.is_responsive(timeout) {
                match agent.status {
                    AgentStatus::Active => {
                        agent.status = AgentStatus::Unresponsive;
                        agent.unresponsive_since = Some(now);
                    }
                    AgentStatus::Unresponsive => {
                        // Only transition to Dead after grace period
                        if let Some(since) = agent.unresponsive_since {
                            if (now - since).num_seconds() >= grace {
                                agent.status = AgentStatus::Dead;
                                dead_agents.push(agent.id);
                            }
                        } else {
                            // Edge case: unresponsive without timestamp — mark now
                            agent.unresponsive_since = Some(now);
                        }
                    }
                    _ => {}
                }
            }
        }

        dead_agents
    }

    /// Find the best agent for a required capability (read-only selection).
    ///
    /// Selection criteria (in order):
    /// 1. Must have the capability and be Active with capacity
    /// 2. Prefer higher proficiency
    /// 3. Prefer lower current load
    /// 4. Prefer higher success rate
    ///
    /// **Warning**: For concurrent access, use `reserve_agent()` instead to
    /// atomically select and increment load.
    pub fn select_agent(&self, capability: &str) -> Option<AgentId> {
        self.agents
            .values()
            .filter(|a| a.has_capacity() && a.can_execute(capability))
            .max_by(|a, b| {
                let score_a = Self::agent_score(a, capability);
                let score_b = Self::agent_score(b, capability);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|a| a.id)
    }

    /// Atomically select the best agent and increment its load.
    /// Returns the agent ID if a suitable agent was found.
    /// This prevents TOCTOU issues where two callers select the same agent.
    pub fn reserve_agent(&mut self, capability: &str) -> Option<AgentId> {
        let id = self.select_agent(capability)?;
        if let Some(agent) = self.agents.get_mut(&id) {
            agent.current_load += 1;
            Some(id)
        } else {
            None
        }
    }

    /// Release an agent slot (decrement load after task completion).
    pub fn release_agent(&mut self, id: AgentId, succeeded: bool) {
        if let Some(agent) = self.agents.get_mut(&id) {
            agent.current_load = agent.current_load.saturating_sub(1);
            if succeeded {
                agent.tasks_completed += 1;
            } else {
                agent.tasks_failed += 1;
            }
        }
    }

    /// Get all active agent IDs.
    pub fn active_agents(&self) -> Vec<AgentId> {
        self.agents
            .values()
            .filter(|a| a.status == AgentStatus::Active)
            .map(|a| a.id)
            .collect()
    }

    /// Total number of registered agents.
    pub fn count(&self) -> usize {
        self.agents.len()
    }

    /// Get all tasks currently assigned to dead agents (for requeueing).
    pub fn dead_agent_load(&self) -> Vec<(AgentId, u32)> {
        self.agents
            .values()
            .filter(|a| a.status == AgentStatus::Dead && a.current_load > 0)
            .map(|a| (a.id, a.current_load))
            .collect()
    }

    /// Score = proficiency * 0.5 + (1 - load_ratio) * 0.3 + success_rate * 0.2
    fn agent_score(agent: &AgentDescriptor, capability: &str) -> f64 {
        agent.proficiency_for(capability) * 0.5
            + (1.0 - agent.current_load as f64 / agent.max_load.max(1) as f64) * 0.3
            + agent.success_rate() * 0.2
    }
}

/// Namespace-isolated shared memory entry for cross-agent communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMemoryEntry {
    /// Namespaced key: `shared:{from_agent}:{key}`.
    pub key: String,
    /// The actual value.
    pub value: serde_json::Value,
    /// Agent that originally created this entry (preserved across snapshots).
    pub origin_agent: AgentId,
    /// Agent that owns this copy (may differ from origin if snapshotted).
    pub owner_agent: AgentId,
    /// Agent(s) this was explicitly shared with.
    pub to_agents: Vec<AgentId>,
    /// Whether other agents can read this without explicit sharing.
    pub shareable: bool,
    /// If snapshotted from another entry, its key.
    pub derived_from: Option<String>,
    /// TTL: when this entry expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// When this entry was created.
    pub created_at: DateTime<Utc>,
}

impl SharedMemoryEntry {
    pub fn new(from_agent: AgentId, key: impl Into<String>, value: serde_json::Value) -> Self {
        let key_str = key.into();
        Self {
            key: format!("shared:{from_agent}:{key_str}"),
            value,
            origin_agent: from_agent,
            owner_agent: from_agent,
            to_agents: Vec::new(),
            shareable: false,
            derived_from: None,
            expires_at: None,
            created_at: Utc::now(),
        }
    }

    pub fn shareable(mut self) -> Self {
        self.shareable = true;
        self
    }

    pub fn with_ttl_secs(mut self, secs: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::seconds(secs));
        self
    }

    pub fn share_with(mut self, agent_id: AgentId) -> Self {
        if !self.to_agents.contains(&agent_id) {
            self.to_agents.push(agent_id);
        }
        self
    }

    /// Check if this entry is still valid (not expired).
    pub fn is_valid(&self) -> bool {
        self.expires_at.map(|exp| Utc::now() < exp).unwrap_or(true)
    }

    /// Check if a given agent can read this entry.
    pub fn is_readable_by(&self, agent_id: AgentId) -> bool {
        self.shareable || self.owner_agent == agent_id || self.to_agents.contains(&agent_id)
    }
}

/// Shared memory bus for cross-agent communication.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SharedMemoryBus {
    entries: Vec<SharedMemoryEntry>,
}

impl SharedMemoryBus {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Store a shared memory entry.
    pub fn put(&mut self, entry: SharedMemoryEntry) {
        if let Some(pos) = self.entries.iter().position(|e| e.key == entry.key) {
            self.entries[pos] = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Get entries readable by a specific agent, filtering expired ones.
    pub fn get_for_agent(&self, agent_id: AgentId) -> Vec<&SharedMemoryEntry> {
        self.entries
            .iter()
            .filter(|e| e.is_valid() && e.is_readable_by(agent_id))
            .collect()
    }

    /// Create a snapshot of an entry from one agent to another.
    ///
    /// This is an explicit copy — updates to the original do NOT propagate.
    /// The `origin_agent` is preserved for audit, while `owner_agent` is set
    /// to the target agent.
    ///
    /// Only works if the entry is marked `shareable`. Owners cannot bypass
    /// the shareable flag to share non-shareable entries.
    pub fn share_snapshot(
        &mut self,
        source_key: &str,
        from_agent: AgentId,
        to_agent: AgentId,
    ) -> bool {
        let source = self
            .entries
            .iter()
            .find(|e| e.key == source_key && e.owner_agent == from_agent);

        if let Some(source) = source.cloned() {
            // Strict: only shareable entries can be snapshotted
            if !source.shareable {
                return false;
            }
            let derived = SharedMemoryEntry {
                key: source.key.replace(
                    &format!("shared:{from_agent}:"),
                    &format!("shared:{to_agent}:"),
                ),
                value: source.value.clone(),
                origin_agent: source.origin_agent, // preserve original creator
                owner_agent: to_agent,
                to_agents: vec![],
                shareable: source.shareable,
                derived_from: Some(source.key.clone()),
                expires_at: source.expires_at,
                created_at: Utc::now(),
            };
            self.put(derived);
            true
        } else {
            false
        }
    }

    /// Remove expired entries.
    pub fn gc(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.is_valid());
        before - self.entries.len()
    }

    /// Total entries (including expired).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_lifecycle() {
        let agent = AgentDescriptor::new("reviewer", "code-reviewer")
            .with_capabilities(vec![
                Capability::new("code_review").with_proficiency(0.9),
                Capability::new("security_scan"),
            ])
            .with_max_load(3);

        assert!(agent.can_execute("code_review"));
        assert!(!agent.can_execute("refactoring"));
        assert_eq!(agent.proficiency_for("code_review"), 0.9);
        assert_eq!(agent.proficiency_for("security_scan"), 0.5); // default
    }

    #[test]
    fn registry_select_best_agent() {
        let mut registry = AgentRegistry::new();

        let mut a1 = AgentDescriptor::new("fast", "worker")
            .with_capabilities(vec![Capability::new("search").with_proficiency(0.7)]);
        a1.current_load = 3;
        a1.max_load = 5;

        let a2 = AgentDescriptor::new("expert", "worker")
            .with_capabilities(vec![Capability::new("search").with_proficiency(0.95)]);

        registry.register(a1);
        let expert_id = registry.register(a2);

        let selected = registry.select_agent("search");
        assert_eq!(selected, Some(expert_id));
    }

    #[test]
    fn registry_no_capable_agent() {
        let mut registry = AgentRegistry::new();
        let a = AgentDescriptor::new("worker", "generic")
            .with_capabilities(vec![Capability::new("file_ops")]);
        registry.register(a);

        assert!(registry.select_agent("code_review").is_none());
    }

    #[test]
    fn reserve_and_release_agent() {
        let mut registry = AgentRegistry::new();
        let agent = AgentDescriptor::new("worker", "generic")
            .with_capabilities(vec![Capability::new("task")])
            .with_max_load(2);
        registry.register(agent);

        // Reserve increments load atomically
        let id = registry.reserve_agent("task").unwrap();
        assert_eq!(registry.get(id).unwrap().current_load, 1);

        let id2 = registry.reserve_agent("task").unwrap();
        assert_eq!(registry.get(id2).unwrap().current_load, 2);

        // Max load reached — no more reservations
        assert!(registry.reserve_agent("task").is_none());

        // Release decrements load
        registry.release_agent(id, true);
        assert_eq!(registry.get(id).unwrap().current_load, 1);
        assert_eq!(registry.get(id).unwrap().tasks_completed, 1);
    }

    #[test]
    fn heartbeat_timeout_with_grace_period() {
        let mut registry = AgentRegistry::new();
        registry.heartbeat_timeout_secs = 1;
        registry.dead_grace_period_secs = 0; // immediate for testing

        let mut agent = AgentDescriptor::new("worker", "generic")
            .with_capabilities(vec![Capability::new("task")]);
        agent.last_heartbeat = Utc::now() - chrono::Duration::seconds(5);
        let id = registry.register(agent);

        // Reset heartbeat to simulate timeout
        registry.get_mut(id).unwrap().last_heartbeat = Utc::now() - chrono::Duration::seconds(5);

        // First check: Active → Unresponsive
        let dead = registry.check_health();
        assert!(dead.is_empty());
        assert_eq!(registry.get(id).unwrap().status, AgentStatus::Unresponsive);
        assert!(registry.get(id).unwrap().unresponsive_since.is_some());

        // Second check: Unresponsive → Dead (grace period = 0)
        let dead = registry.check_health();
        assert_eq!(dead.len(), 1);
        assert_eq!(registry.get(id).unwrap().status, AgentStatus::Dead);
    }

    #[test]
    fn shared_memory_isolation() {
        let mut bus = SharedMemoryBus::new();
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        let private = SharedMemoryEntry::new(agent_a, "secret", serde_json::json!("data"));
        bus.put(private);

        assert!(bus.get_for_agent(agent_b).is_empty());
        assert_eq!(bus.get_for_agent(agent_a).len(), 1);
    }

    #[test]
    fn shared_memory_shareable_flag() {
        let mut bus = SharedMemoryBus::new();
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        let public =
            SharedMemoryEntry::new(agent_a, "finding", serde_json::json!("bug")).shareable();
        bus.put(public);

        assert_eq!(bus.get_for_agent(agent_a).len(), 1);
        assert_eq!(bus.get_for_agent(agent_b).len(), 1);
    }

    #[test]
    fn share_snapshot_requires_shareable() {
        let mut bus = SharedMemoryBus::new();
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        // Non-shareable entry — owner cannot share it
        let private = SharedMemoryEntry::new(agent_a, "secret", serde_json::json!("data"));
        let key = private.key.clone();
        bus.put(private);

        assert!(!bus.share_snapshot(&key, agent_a, agent_b));
        assert!(bus.get_for_agent(agent_b).is_empty());
    }

    #[test]
    fn share_snapshot_preserves_origin() {
        let mut bus = SharedMemoryBus::new();
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        let entry =
            SharedMemoryEntry::new(agent_a, "context", serde_json::json!("important")).shareable();
        let key = entry.key.clone();
        bus.put(entry);

        assert!(bus.share_snapshot(&key, agent_a, agent_b));

        let b_entries = bus.get_for_agent(agent_b);
        assert!(!b_entries.is_empty());
        let derived = b_entries
            .iter()
            .find(|e| e.derived_from.is_some())
            .expect("should have derived entry");
        assert_eq!(derived.derived_from.as_ref().unwrap(), &key);
        // Origin preserved, owner changed
        assert_eq!(derived.origin_agent, agent_a);
        assert_eq!(derived.owner_agent, agent_b);
    }

    #[test]
    fn shared_memory_ttl_expiry() {
        let mut bus = SharedMemoryBus::new();
        let agent_a = AgentId::new();

        let mut entry = SharedMemoryEntry::new(agent_a, "temp", serde_json::json!("data"));
        entry.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
        bus.put(entry);

        assert!(bus.get_for_agent(agent_a).is_empty());
        assert_eq!(bus.gc(), 1);
        assert!(bus.is_empty());
    }

    #[test]
    fn success_rate_tracking() {
        let mut agent = AgentDescriptor::new("worker", "generic");
        agent.tasks_completed = 8;
        agent.tasks_failed = 2;
        assert!((agent.success_rate() - 0.8).abs() < f64::EPSILON);
    }
}
