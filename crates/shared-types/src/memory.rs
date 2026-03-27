use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
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
    /// Files and symbols that have been inspected during this session.
    #[serde(default)]
    pub inspected_areas: Vec<InspectedArea>,
    /// Current planner/execution state for context survival across compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_state: Option<PlannerState>,
}

/// A code area that has been inspected during the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectedArea {
    /// File path or module path.
    pub path: String,
    /// Symbols found (function names, types, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    /// What was learned from inspecting this area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// When this area was inspected.
    pub inspected_at: DateTime<Utc>,
}

impl InspectedArea {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            symbols: Vec::new(),
            summary: None,
            inspected_at: Utc::now(),
        }
    }

    pub fn with_symbols(mut self, symbols: Vec<String>) -> Self {
        self.symbols = symbols;
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

/// Planner execution state — survives context compaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlannerState {
    /// Current step name being executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
    /// Number of replans triggered so far.
    #[serde(default)]
    pub replan_count: u32,
    /// Execution phase (explore, implement, verify).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Task ID if part of a lease.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<Uuid>,
}

/// Severity level of a memory entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySeverity {
    /// Informational — context only.
    #[default]
    Info,
    /// Warning — potential issue.
    Warning,
    /// Error — confirmed failure.
    Error,
    /// Critical — blocking issue.
    Critical,
}

/// Status of a memory entry in its lifecycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    /// Newly created, not yet acted upon.
    #[default]
    Active,
    /// Confirmed by evidence.
    Confirmed,
    /// Contradicted by newer evidence.
    Contradicted,
    /// Superseded by a more specific entry.
    Superseded,
    /// Stale — no longer relevant to the current task context.
    Stale,
}

/// A single entry in working memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    /// Human-readable content.
    pub content: String,
    /// When this entry was created.
    pub created_at: DateTime<Utc>,
    /// When this entry was last updated.
    pub updated_at: DateTime<Utc>,
    /// Optional source reference (file path, observation ID, etc.).
    pub source: Option<String>,
    /// Tags for categorization and retrieval.
    pub tags: Vec<String>,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f64,
    /// Severity level.
    #[serde(default)]
    pub severity: MemorySeverity,
    /// Lifecycle status.
    #[serde(default)]
    pub status: MemoryStatus,
    /// IDs of entries that support this one.
    #[serde(default)]
    pub supporting_evidence: Vec<Uuid>,
    /// IDs of entries that contradict this one.
    #[serde(default)]
    pub contradicting_evidence: Vec<Uuid>,
}

impl MemoryEntry {
    pub fn new(content: String, confidence: f64) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            content,
            created_at: now,
            updated_at: now,
            source: None,
            tags: Vec::new(),
            confidence,
            severity: MemorySeverity::Info,
            status: MemoryStatus::Active,
            supporting_evidence: Vec::new(),
            contradicting_evidence: Vec::new(),
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

    pub fn with_severity(mut self, severity: MemorySeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Record that another entry supports this one.
    pub fn add_support(&mut self, evidence_id: Uuid) {
        if !self.supporting_evidence.contains(&evidence_id) {
            self.supporting_evidence.push(evidence_id);
            self.updated_at = Utc::now();
        }
    }

    /// Record that another entry contradicts this one.
    pub fn add_contradiction(&mut self, evidence_id: Uuid) {
        if !self.contradicting_evidence.contains(&evidence_id) {
            self.contradicting_evidence.push(evidence_id);
            self.updated_at = Utc::now();
        }
    }

    /// Net confidence: base confidence adjusted by support/contradiction ratio.
    pub fn effective_confidence(&self) -> f64 {
        let support = self.supporting_evidence.len() as f64;
        let contradict = self.contradicting_evidence.len() as f64;
        let total = support + contradict;
        if total == 0.0 {
            return self.confidence;
        }
        // Boost or penalize by up to ±0.2 based on evidence balance.
        let balance = (support - contradict) / total;
        (self.confidence + balance * 0.2).clamp(0.0, 1.0)
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
            entry.status = MemoryStatus::Confirmed;
            entry.updated_at = Utc::now();
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
                entry.status = MemoryStatus::Contradicted;
                entry.updated_at = Utc::now();
                self.invalidated.push(entry);
                return true;
            }
        }
        false
    }

    /// Mark an entry as superseded by a newer, more specific entry.
    pub fn supersede(&mut self, old_id: Uuid, new_id: Uuid) -> bool {
        for entry in self
            .findings
            .iter_mut()
            .chain(self.hypotheses.iter_mut())
            .chain(self.verified_facts.iter_mut())
        {
            if entry.id == old_id {
                entry.status = MemoryStatus::Superseded;
                entry.tags.push(format!("superseded_by:{new_id}"));
                entry.updated_at = Utc::now();
                return true;
            }
        }
        false
    }

    /// Link two entries: `evidence_id` supports `target_id`.
    pub fn add_evidence_link(&mut self, target_id: Uuid, evidence_id: Uuid, supports: bool) {
        for entry in self
            .findings
            .iter_mut()
            .chain(self.hypotheses.iter_mut())
            .chain(self.verified_facts.iter_mut())
            .chain(self.questions.iter_mut())
        {
            if entry.id == target_id {
                if supports {
                    entry.add_support(evidence_id);
                } else {
                    entry.add_contradiction(evidence_id);
                }
                return;
            }
        }
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

    /// Get all active entries (findings + hypotheses + facts + questions)
    /// that are not superseded or contradicted.
    pub fn active_entries(&self) -> Vec<&MemoryEntry> {
        self.findings
            .iter()
            .chain(self.hypotheses.iter())
            .chain(self.verified_facts.iter())
            .chain(self.questions.iter())
            .filter(|e| matches!(e.status, MemoryStatus::Active | MemoryStatus::Confirmed))
            .collect()
    }

    /// Total count of active (non-invalidated) entries.
    pub fn active_count(&self) -> usize {
        self.active_entries().len()
    }

    /// Check if any active finding has the given severity or higher.
    pub fn has_severity_at_least(&self, min_severity: MemorySeverity) -> bool {
        self.active_entries()
            .iter()
            .any(|e| e.severity as u8 >= min_severity as u8)
    }

    /// Return active entries with errors or critical severity.
    pub fn unresolved_errors(&self) -> Vec<&MemoryEntry> {
        self.active_entries()
            .into_iter()
            .filter(|e| matches!(e.severity, MemorySeverity::Error | MemorySeverity::Critical))
            .collect()
    }

    /// Record that a code area was inspected.
    pub fn record_inspection(&mut self, area: InspectedArea) {
        // Deduplicate by path — update if already inspected
        if let Some(existing) = self.inspected_areas.iter_mut().find(|a| a.path == area.path) {
            existing.symbols.extend(area.symbols);
            existing.symbols.sort();
            existing.symbols.dedup();
            if area.summary.is_some() {
                existing.summary = area.summary;
            }
            existing.inspected_at = Utc::now();
        } else {
            self.inspected_areas.push(area);
        }
    }

    /// Update the planner execution state.
    pub fn update_planner_state(&mut self, state: PlannerState) {
        self.planner_state = Some(state);
    }

    /// Persist working memory to a JSON file.
    pub fn save_to(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load working memory from a JSON file. Returns default if file doesn't exist.
    pub fn load_from(path: &Path) -> Result<Self, std::io::Error> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Export a compact JSON snapshot suitable for MCP injection.
    /// Keeps only active entries, omits audit trail.
    pub fn compact_snapshot(&self) -> serde_json::Value {
        let active_hypotheses: Vec<_> = self.hypotheses.iter()
            .filter(|h| h.status == MemoryStatus::Active)
            .map(|h| serde_json::json!({
                "text": h.content,
                "confidence": format!("{:.0}%", h.effective_confidence() * 100.0),
                "status": "active",
            }))
            .collect();

        let facts: Vec<_> = self.verified_facts.iter()
            .map(|f| &f.content)
            .collect();

        let areas: Vec<_> = self.inspected_areas.iter()
            .map(|a| &a.path)
            .collect();

        let questions: Vec<_> = self.questions.iter()
            .map(|q| &q.content)
            .collect();

        let mut snapshot = serde_json::json!({
            "hypotheses": active_hypotheses,
            "verified_facts": facts,
            "inspected_areas": areas,
            "open_questions": questions,
        });

        if let Some(ref ps) = self.planner_state {
            snapshot["planner_state"] = serde_json::json!({
                "current_step": ps.current_step,
                "replan_count": ps.replan_count,
                "phase": ps.phase,
            });
        }

        if !self.plan.is_empty() {
            snapshot["plan"] = serde_json::json!(self.plan);
        }

        snapshot
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
            let active_findings: Vec<_> = self
                .findings
                .iter()
                .filter(|f| matches!(f.status, MemoryStatus::Active | MemoryStatus::Confirmed))
                .collect();
            if !active_findings.is_empty() {
                parts.push(format!(
                    "Findings ({}):\n{}",
                    active_findings.len(),
                    active_findings
                        .iter()
                        .map(|f| {
                            let sev = match f.severity {
                                MemorySeverity::Critical => " [CRITICAL]",
                                MemorySeverity::Error => " [ERROR]",
                                MemorySeverity::Warning => " [WARN]",
                                MemorySeverity::Info => "",
                            };
                            format!(
                                "  - {}{} (confidence: {:.0}%)",
                                f.content,
                                sev,
                                f.effective_confidence() * 100.0
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }

        if !self.hypotheses.is_empty() {
            let active_hyp: Vec<_> = self
                .hypotheses
                .iter()
                .filter(|h| matches!(h.status, MemoryStatus::Active))
                .collect();
            if !active_hyp.is_empty() {
                parts.push(format!(
                    "Hypotheses ({}):\n{}",
                    active_hyp.len(),
                    active_hyp
                        .iter()
                        .map(|h| format!(
                            "  - {} (confidence: {:.0}%)",
                            h.content,
                            h.effective_confidence() * 100.0
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
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

        if !self.inspected_areas.is_empty() {
            parts.push(format!(
                "Inspected areas ({}):\n{}",
                self.inspected_areas.len(),
                self.inspected_areas
                    .iter()
                    .map(|a| {
                        let symbols = if a.symbols.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", a.symbols.join(", "))
                        };
                        format!("  - {}{}", a.path, symbols)
                    })
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

        if let Some(ref ps) = self.planner_state {
            let mut state_parts = Vec::new();
            if let Some(ref step) = ps.current_step {
                state_parts.push(format!("step: {step}"));
            }
            if let Some(ref phase) = ps.phase {
                state_parts.push(format!("phase: {phase}"));
            }
            if ps.replan_count > 0 {
                state_parts.push(format!("replans: {}", ps.replan_count));
            }
            if !state_parts.is_empty() {
                parts.push(format!("Planner: {}", state_parts.join(", ")));
            }
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
        assert_eq!(mem.verified_facts[0].status, MemoryStatus::Confirmed);
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
        assert_eq!(mem.invalidated[0].status, MemoryStatus::Contradicted);
    }

    #[test]
    fn supersede_marks_old_entry() {
        let mut mem = WorkingMemory::default();
        let old = MemoryEntry::new("old finding".into(), 0.6);
        let old_id = old.id;
        let new = MemoryEntry::new("refined finding".into(), 0.9);
        let new_id = new.id;
        mem.add_finding(old);
        mem.add_finding(new);

        assert!(mem.supersede(old_id, new_id));
        assert_eq!(mem.findings[0].status, MemoryStatus::Superseded);
        // Superseded entries are excluded from active_entries.
        assert_eq!(mem.active_count(), 1);
    }

    #[test]
    fn evidence_links_affect_confidence() {
        let mut entry = MemoryEntry::new("hypothesis".into(), 0.5);
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let c1 = Uuid::new_v4();

        entry.add_support(s1);
        entry.add_support(s2);
        entry.add_contradiction(c1);
        // 2 supports, 1 contradiction → balance = 1/3 → boost = +0.067
        let eff = entry.effective_confidence();
        assert!(eff > 0.5);
        assert!(eff < 0.6);
    }

    #[test]
    fn severity_filter() {
        let mut mem = WorkingMemory::default();
        mem.add_finding(MemoryEntry::new("info".into(), 0.5).with_severity(MemorySeverity::Info));
        mem.add_finding(MemoryEntry::new("error".into(), 0.8).with_severity(MemorySeverity::Error));

        assert!(mem.has_severity_at_least(MemorySeverity::Error));
        assert_eq!(mem.unresolved_errors().len(), 1);
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
    fn summary_shows_severity() {
        let mut mem = WorkingMemory::default();
        mem.add_finding(
            MemoryEntry::new("critical bug".into(), 0.9).with_severity(MemorySeverity::Critical),
        );
        let summary = mem.summary();
        assert!(summary.contains("[CRITICAL]"));
    }

    #[test]
    fn active_count_tracks_all_categories() {
        let mut mem = WorkingMemory::default();
        mem.add_finding(MemoryEntry::new("f1".into(), 0.5));
        mem.add_hypothesis(MemoryEntry::new("h1".into(), 0.5));
        mem.add_question(MemoryEntry::new("q1".into(), 0.5));
        assert_eq!(mem.active_count(), 3);
    }

    #[test]
    fn record_inspection_deduplicates() {
        let mut mem = WorkingMemory::default();
        mem.record_inspection(
            InspectedArea::new("src/auth.rs")
                .with_symbols(vec!["login".into()])
                .with_summary("handles JWT auth"),
        );
        mem.record_inspection(
            InspectedArea::new("src/auth.rs")
                .with_symbols(vec!["logout".into()])
                .with_summary("updated understanding"),
        );
        assert_eq!(mem.inspected_areas.len(), 1);
        assert_eq!(mem.inspected_areas[0].symbols, vec!["login", "logout"]);
        assert_eq!(
            mem.inspected_areas[0].summary.as_deref(),
            Some("updated understanding")
        );
    }

    #[test]
    fn record_inspection_multiple_paths() {
        let mut mem = WorkingMemory::default();
        mem.record_inspection(InspectedArea::new("src/auth.rs"));
        mem.record_inspection(InspectedArea::new("src/db.rs"));
        assert_eq!(mem.inspected_areas.len(), 2);
    }

    #[test]
    fn planner_state_updates() {
        let mut mem = WorkingMemory::default();
        assert!(mem.planner_state.is_none());
        mem.update_planner_state(PlannerState {
            current_step: Some("investigate".into()),
            replan_count: 1,
            phase: Some("explore".into()),
            lease_id: None,
        });
        assert_eq!(
            mem.planner_state.as_ref().unwrap().current_step.as_deref(),
            Some("investigate")
        );
        assert_eq!(mem.planner_state.as_ref().unwrap().replan_count, 1);
    }

    #[test]
    fn compact_snapshot_structure() {
        let mut mem = WorkingMemory::default();
        mem.add_hypothesis(MemoryEntry::new("session cookie issue".into(), 0.6));
        mem.add_finding(MemoryEntry::new("typecheck passes".into(), 0.9));
        let fact = MemoryEntry::new("auth test fails on refresh".into(), 1.0);
        let fact_id = fact.id;
        mem.add_finding(fact);
        mem.promote_to_fact(fact_id);
        mem.record_inspection(InspectedArea::new("api/auth/middleware.ts"));
        mem.add_question(MemoryEntry::new("which middleware runs first?".into(), 0.5));
        mem.update_planner_state(PlannerState {
            current_step: Some("verify middleware chain".into()),
            replan_count: 1,
            phase: Some("investigate".into()),
            lease_id: None,
        });

        let snapshot = mem.compact_snapshot();
        assert!(snapshot["hypotheses"].is_array());
        assert_eq!(snapshot["hypotheses"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["verified_facts"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["inspected_areas"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["open_questions"].as_array().unwrap().len(), 1);
        assert!(snapshot["planner_state"]["current_step"].is_string());
    }

    #[test]
    fn persistence_roundtrip() {
        let mut mem = WorkingMemory::default();
        mem.add_finding(MemoryEntry::new("test finding".into(), 0.7));
        mem.record_inspection(InspectedArea::new("src/main.rs"));
        mem.update_planner_state(PlannerState {
            current_step: Some("step1".into()),
            replan_count: 0,
            phase: None,
            lease_id: None,
        });

        let dir = std::env::temp_dir().join("oco-test-memory");
        let path = dir.join("memory.json");
        mem.save_to(&path).unwrap();

        let loaded = WorkingMemory::load_from(&path).unwrap();
        assert_eq!(loaded.findings.len(), 1);
        assert_eq!(loaded.findings[0].content, "test finding");
        assert_eq!(loaded.inspected_areas.len(), 1);
        assert!(loaded.planner_state.is_some());

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_nonexistent_returns_default() {
        let path = std::path::Path::new("/tmp/oco-nonexistent-12345/memory.json");
        let mem = WorkingMemory::load_from(path).unwrap();
        assert_eq!(mem.active_count(), 0);
    }

    #[test]
    fn summary_includes_inspected_areas() {
        let mut mem = WorkingMemory::default();
        mem.record_inspection(
            InspectedArea::new("src/auth.rs").with_symbols(vec!["login".into(), "verify".into()]),
        );
        let summary = mem.summary();
        assert!(summary.contains("Inspected areas"));
        assert!(summary.contains("src/auth.rs"));
        assert!(summary.contains("login, verify"));
    }

    #[test]
    fn summary_includes_planner_state() {
        let mut mem = WorkingMemory::default();
        mem.update_planner_state(PlannerState {
            current_step: Some("investigate auth".into()),
            replan_count: 2,
            phase: Some("explore".into()),
            lease_id: None,
        });
        let summary = mem.summary();
        assert!(summary.contains("Planner:"));
        assert!(summary.contains("investigate auth"));
        assert!(summary.contains("replans: 2"));
    }
}
