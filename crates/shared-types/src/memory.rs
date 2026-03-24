use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Structured working memory for an orchestration session.
///
/// Unlike raw observations (append-only log), working memory is curated:
/// findings can be promoted to facts, invalidated, or superseded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkingMemory {
    /// Active findings — things discovered but not yet verified.
    pub findings: Vec<MemoryEntry>,
    /// Verified facts — findings that have been confirmed.
    pub verified_facts: Vec<MemoryEntry>,
    /// Current hypotheses being explored.
    pub hypotheses: Vec<MemoryEntry>,
    /// Unresolved questions that need more information.
    pub questions: Vec<MemoryEntry>,
    /// Current plan or next steps.
    pub plan: Vec<String>,
    /// Invalidated entries (kept for audit trail).
    pub invalidated: Vec<MemoryEntry>,
}

/// A single entry in working memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    /// Human-readable content.
    pub content: String,
    /// When this entry was created.
    pub created_at: DateTime<Utc>,
    /// Optional source reference (file path, observation ID, etc.).
    pub source: Option<String>,
    /// Tags for categorization and retrieval.
    pub tags: Vec<String>,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f64,
}

impl MemoryEntry {
    pub fn new(content: String, confidence: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            content,
            created_at: Utc::now(),
            source: None,
            tags: Vec::new(),
            confidence,
        }
    }

    pub fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

impl WorkingMemory {
    /// Add a new finding.
    pub fn add_finding(&mut self, entry: MemoryEntry) {
        self.findings.push(entry);
    }

    /// Promote a finding to a verified fact by ID.
    /// Returns `true` if the finding was found and promoted.
    pub fn promote_to_fact(&mut self, id: Uuid) -> bool {
        if let Some(pos) = self.findings.iter().position(|f| f.id == id) {
            let mut entry = self.findings.remove(pos);
            entry.confidence = 1.0;
            self.verified_facts.push(entry);
            true
        } else {
            false
        }
    }

    /// Invalidate an entry by ID from any category.
    /// Moves it to the invalidated list.
    pub fn invalidate(&mut self, id: Uuid, reason: &str) -> bool {
        let lists: Vec<&mut Vec<MemoryEntry>> = vec![
            &mut self.findings,
            &mut self.hypotheses,
            &mut self.verified_facts,
        ];

        for list in lists {
            if let Some(pos) = list.iter().position(|e| e.id == id) {
                let mut entry = list.remove(pos);
                entry.tags.push(format!("invalidated: {reason}"));
                self.invalidated.push(entry);
                return true;
            }
        }
        false
    }

    /// Add a hypothesis.
    pub fn add_hypothesis(&mut self, entry: MemoryEntry) {
        self.hypotheses.push(entry);
    }

    /// Add an unresolved question.
    pub fn add_question(&mut self, entry: MemoryEntry) {
        self.questions.push(entry);
    }

    /// Update the current plan.
    pub fn update_plan(&mut self, steps: Vec<String>) {
        self.plan = steps;
    }

    /// Resolve a question by ID — removes it from questions.
    pub fn resolve_question(&mut self, id: Uuid) -> bool {
        if let Some(pos) = self.questions.iter().position(|q| q.id == id) {
            self.questions.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get all active entries (findings + hypotheses + facts + questions).
    pub fn active_entries(&self) -> Vec<&MemoryEntry> {
        self.findings
            .iter()
            .chain(self.hypotheses.iter())
            .chain(self.verified_facts.iter())
            .chain(self.questions.iter())
            .collect()
    }

    /// Total count of active (non-invalidated) entries.
    pub fn active_count(&self) -> usize {
        self.findings.len()
            + self.hypotheses.len()
            + self.verified_facts.len()
            + self.questions.len()
    }

    /// Render a compact summary of working memory for inclusion in context.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.verified_facts.is_empty() {
            parts.push(format!(
                "Verified facts ({}):\n{}",
                self.verified_facts.len(),
                self.verified_facts
                    .iter()
                    .map(|f| format!("  - {}", f.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.findings.is_empty() {
            parts.push(format!(
                "Findings ({}):\n{}",
                self.findings.len(),
                self.findings
                    .iter()
                    .map(|f| format!("  - {} (confidence: {:.0}%)", f.content, f.confidence * 100.0))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.hypotheses.is_empty() {
            parts.push(format!(
                "Hypotheses ({}):\n{}",
                self.hypotheses.len(),
                self.hypotheses
                    .iter()
                    .map(|h| format!("  - {}", h.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.questions.is_empty() {
            parts.push(format!(
                "Open questions ({}):\n{}",
                self.questions.len(),
                self.questions
                    .iter()
                    .map(|q| format!("  ? {}", q.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.plan.is_empty() {
            parts.push(format!(
                "Current plan:\n{}",
                self.plan
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("  {}. {s}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if parts.is_empty() {
            "Working memory: empty".into()
        } else {
            parts.join("\n\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_promote_finding() {
        let mut mem = WorkingMemory::default();
        let entry = MemoryEntry::new("bug in auth module".into(), 0.7);
        let id = entry.id;
        mem.add_finding(entry);
        assert_eq!(mem.findings.len(), 1);

        assert!(mem.promote_to_fact(id));
        assert_eq!(mem.findings.len(), 0);
        assert_eq!(mem.verified_facts.len(), 1);
        assert_eq!(mem.verified_facts[0].confidence, 1.0);
    }

    #[test]
    fn invalidate_moves_to_invalidated() {
        let mut mem = WorkingMemory::default();
        let entry = MemoryEntry::new("hypothesis A".into(), 0.5);
        let id = entry.id;
        mem.add_hypothesis(entry);

        assert!(mem.invalidate(id, "disproven by test"));
        assert_eq!(mem.hypotheses.len(), 0);
        assert_eq!(mem.invalidated.len(), 1);
    }

    #[test]
    fn summary_renders_nonempty() {
        let mut mem = WorkingMemory::default();
        mem.add_finding(MemoryEntry::new("found issue X".into(), 0.6));
        mem.update_plan(vec!["fix X".into(), "test".into()]);

        let summary = mem.summary();
        assert!(summary.contains("found issue X"));
        assert!(summary.contains("fix X"));
    }

    #[test]
    fn active_count_tracks_all_categories() {
        let mut mem = WorkingMemory::default();
        mem.add_finding(MemoryEntry::new("f1".into(), 0.5));
        mem.add_hypothesis(MemoryEntry::new("h1".into(), 0.5));
        mem.add_question(MemoryEntry::new("q1".into(), 0.5));
        assert_eq!(mem.active_count(), 3);
    }
}
