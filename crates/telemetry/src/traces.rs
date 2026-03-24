use std::sync::Mutex;

use oco_shared_types::{
    DecisionTrace, InterventionOutcome, InterventionSummary, SessionId,
    TelemetryEvent, TelemetryEventType,
};

/// Thread-safe collector for decision traces and telemetry events.
pub struct DecisionTraceCollector {
    traces: Mutex<Vec<DecisionTrace>>,
    /// v2: Fine-grained telemetry events.
    events: Mutex<Vec<TelemetryEvent>>,
}

impl DecisionTraceCollector {
    /// Create a new empty trace collector.
    pub fn new() -> Self {
        Self {
            traces: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
        }
    }

    /// Record a decision trace.
    pub fn record(&self, trace: DecisionTrace) {
        let mut traces = self.traces.lock().expect("trace mutex poisoned");
        traces.push(trace);
    }

    /// v2: Record a telemetry event. Returns the event index for outcome tracking.
    pub fn record_event(&self, event_type: TelemetryEventType) -> usize {
        let event = TelemetryEvent {
            timestamp: chrono::Utc::now(),
            event_type,
            outcome: None,
        };
        let mut events = self.events.lock().expect("events mutex poisoned");
        let idx = events.len();
        events.push(event);
        idx
    }

    /// v2: Mark a specific event by index with an outcome. Thread-safe.
    pub fn mark_outcome(&self, event_idx: usize, outcome: InterventionOutcome) {
        let mut events = self.events.lock().expect("events mutex poisoned");
        if let Some(event) = events.get_mut(event_idx) {
            event.outcome = Some(outcome);
        }
    }

    /// v2: Get all events.
    pub fn get_events(&self) -> Vec<TelemetryEvent> {
        let events = self.events.lock().expect("events mutex poisoned");
        events.clone()
    }

    /// v2: Compute an intervention summary from recorded events.
    pub fn intervention_summary(&self) -> InterventionSummary {
        let events = self.events.lock().expect("events mutex poisoned");
        let mut useful = 0u32;
        let mut redundant = 0u32;
        let mut harmful = 0u32;
        let mut unknown = 0u32;
        for event in events.iter() {
            match event.outcome {
                Some(InterventionOutcome::Useful) => useful += 1,
                Some(InterventionOutcome::Redundant) => redundant += 1,
                Some(InterventionOutcome::Harmful) => harmful += 1,
                Some(InterventionOutcome::Unknown) | None => unknown += 1,
            }
        }
        InterventionSummary {
            total_interventions: events.len() as u32,
            useful,
            redundant,
            harmful,
            unknown,
        }
    }

    /// v2: Export all events as JSONL (one event per line).
    pub fn export_events_jsonl(&self) -> String {
        let events = self.events.lock().expect("events mutex poisoned");
        events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get all traces for a given session.
    pub fn get_session_traces(&self, session_id: SessionId) -> Vec<DecisionTrace> {
        let traces = self.traces.lock().expect("trace mutex poisoned");
        traces
            .iter()
            .filter(|t| t.session_id == session_id)
            .cloned()
            .collect()
    }

    /// Export all traces for a session as a JSON string.
    pub fn export_json(&self, session_id: SessionId) -> String {
        let session_traces = self.get_session_traces(session_id);
        serde_json::to_string_pretty(&session_traces).unwrap_or_else(|_| "[]".to_string())
    }

    /// Return the total number of recorded traces.
    pub fn len(&self) -> usize {
        let traces = self.traces.lock().expect("trace mutex poisoned");
        traces.len()
    }

    /// Check if the collector is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all stored traces and events.
    pub fn clear(&self) {
        let mut traces = self.traces.lock().expect("trace mutex poisoned");
        traces.clear();
        drop(traces);
        let mut events = self.events.lock().expect("events mutex poisoned");
        events.clear();
    }
}

impl Default for DecisionTraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_event_and_summarize() {
        let collector = DecisionTraceCollector::new();
        let idx = collector.record_event(TelemetryEventType::HookTriggered {
            hook_name: "pre_tool_use".into(),
            tool_name: Some("Bash".into()),
        });
        collector.mark_outcome(idx, InterventionOutcome::Useful);
        collector.record_event(TelemetryEventType::VerifyCompleted {
            strategy: "build".into(),
            passed: true,
            duration_ms: 500,
        });

        let summary = collector.intervention_summary();
        assert_eq!(summary.total_interventions, 2);
        assert_eq!(summary.useful, 1);
        assert_eq!(summary.unknown, 1);
    }

    #[test]
    fn export_events_jsonl_format() {
        let collector = DecisionTraceCollector::new();
        collector.record_event(TelemetryEventType::SkillInvoked {
            skill_name: "oco-verify-fix".into(),
        });
        collector.record_event(TelemetryEventType::MemoryUpdated {
            operation: "add_finding".into(),
            active_count: 3,
        });

        let jsonl = collector.export_events_jsonl();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("skill_invoked"));
        assert!(lines[1].contains("memory_updated"));
    }
}
