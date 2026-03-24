use std::collections::HashMap;
use std::sync::Mutex;

use oco_shared_types::{SessionId, SessionTelemetry};

/// Aggregates session-level metrics: steps, tokens, tool calls.
pub struct SessionMetrics {
    session_id: SessionId,
    inner: Mutex<MetricsInner>,
}

struct MetricsInner {
    total_steps: u32,
    total_duration_ms: u64,
    total_tokens: u64,
    total_tool_calls: u32,
    tool_call_counts: HashMap<String, u32>,
}

impl SessionMetrics {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            inner: Mutex::new(MetricsInner {
                total_steps: 0,
                total_duration_ms: 0,
                total_tokens: 0,
                total_tool_calls: 0,
                tool_call_counts: HashMap::new(),
            }),
        }
    }

    /// Record one orchestration step with its duration.
    pub fn record_step(&self, duration_ms: u64) {
        let mut inner = self.inner.lock().expect("metrics mutex poisoned");
        inner.total_steps += 1;
        inner.total_duration_ms += duration_ms;
    }

    /// Record token usage for an LLM call.
    pub fn record_token_usage(&self, tokens: u64) {
        let mut inner = self.inner.lock().expect("metrics mutex poisoned");
        inner.total_tokens += tokens;
    }

    /// Record a tool call by name.
    pub fn record_tool_call(&self, tool_name: &str) {
        let mut inner = self.inner.lock().expect("metrics mutex poisoned");
        inner.total_tool_calls += 1;
        *inner
            .tool_call_counts
            .entry(tool_name.to_string())
            .or_insert(0) += 1;
    }

    /// Produce a summary snapshot of the current session telemetry.
    pub fn summary(&self) -> SessionTelemetry {
        let inner = self.inner.lock().expect("metrics mutex poisoned");
        SessionTelemetry {
            session_id: self.session_id,
            total_steps: inner.total_steps,
            total_tokens: inner.total_tokens,
            total_tool_calls: inner.total_tool_calls,
            total_retrievals: 0,
            total_verify_cycles: 0,
            total_duration_ms: inner.total_duration_ms,
            outcome: String::new(),
            traces: Vec::new(),
            events: Vec::new(),
            intervention_summary: None,
        }
    }

    /// Get the per-tool call counts.
    pub fn tool_call_counts(&self) -> HashMap<String, u32> {
        let inner = self.inner.lock().expect("metrics mutex poisoned");
        inner.tool_call_counts.clone()
    }
}
