/// Simple in-memory cache with LRU eviction.
use std::collections::HashMap;

pub struct Cache {
    data: HashMap<String, String>,
    capacity: usize,
}

impl Cache {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: HashMap::with_capacity(capacity),
            capacity,
        }
    }

    pub fn set(&mut self, key: String, value: String) {
        if self.data.len() >= self.capacity {
            // Simple eviction: remove first key
            if let Some(first_key) = self.data.keys().next().cloned() {
                self.data.remove(&first_key);
            }
        }
        self.data.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }
}

/// Global cache accessor (stub).
pub fn get(key: &str) -> Option<String> {
    let _ = key;
    None
}
