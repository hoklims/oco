use dashmap::DashMap;
use oco_shared_types::ToolDescriptor;
use tracing::debug;

/// Concurrent tool registry backed by DashMap.
///
/// Allows registering and looking up tool descriptors by name
/// from multiple threads without external synchronization.
pub struct ToolRegistry {
    tools: DashMap<String, ToolDescriptor>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
        }
    }

    /// Register a tool descriptor. Overwrites any previous descriptor with the
    /// same name.
    pub fn register(&self, descriptor: ToolDescriptor) {
        debug!(tool = %descriptor.name, "registering tool");
        self.tools.insert(descriptor.name.clone(), descriptor);
    }

    /// Retrieve a tool descriptor by name.
    pub fn get(&self, name: &str) -> Option<ToolDescriptor> {
        self.tools.get(name).map(|entry| entry.value().clone())
    }

    /// List all registered tool descriptors (unordered).
    pub fn list(&self) -> Vec<ToolDescriptor> {
        self.tools
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Check whether a tool with the given name is registered.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_descriptor(name: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_string(),
            description: format!("Test tool {name}"),
            input_schema: serde_json::json!({"type": "object"}),
            is_write: false,
            requires_confirmation: false,
            timeout_secs: 30,
            tags: vec!["test".to_string()],
        }
    }

    #[test]
    fn register_and_get() {
        let registry = ToolRegistry::new();
        registry.register(sample_descriptor("read_file"));

        let desc = registry.get("read_file").expect("tool should exist");
        assert_eq!(desc.name, "read_file");
    }

    #[test]
    fn has_returns_false_for_missing() {
        let registry = ToolRegistry::new();
        assert!(!registry.has("nonexistent"));
    }

    #[test]
    fn list_returns_all_registered() {
        let registry = ToolRegistry::new();
        registry.register(sample_descriptor("a"));
        registry.register(sample_descriptor("b"));
        registry.register(sample_descriptor("c"));

        let list = registry.list();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn register_overwrites() {
        let registry = ToolRegistry::new();
        registry.register(sample_descriptor("x"));

        let mut updated = sample_descriptor("x");
        updated.description = "updated".to_string();
        registry.register(updated);

        let desc = registry.get("x").unwrap();
        assert_eq!(desc.description, "updated");
    }
}
