//! Durable mission memory — the authoritative handoff artifact for OCO.
//!
//! Unlike [`CompactSnapshot`](crate::CompactSnapshot) (designed for intra-session
//! compact survival), `MissionMemory` is the full inter-session handoff record.
//! It answers:
//!
//! - What does the orchestrator hold as true?
//! - What remains hypothetical?
//! - What is still open?
//! - What is the current/next objective?
//! - What is the confidence/verification level?
//!
//! It survives persistence/restoration across sessions and process restarts.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::SessionId;

/// Current schema version. Bump when the on-disk format changes.
pub const MISSION_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A verified fact with provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionFact {
    /// Human-readable content of the fact.
    pub content: String,
    /// Where this fact was established (file path, tool output, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// When this fact was confirmed.
    pub established_at: DateTime<Utc>,
}

/// An active hypothesis with confidence and evidence summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionHypothesis {
    /// Human-readable hypothesis text.
    pub content: String,
    /// Confidence as an integer percentage (0–100).
    pub confidence_pct: u8,
    /// Summary of supporting evidence (human-readable strings, not IDs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_evidence: Vec<String>,
}

/// Current plan state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MissionPlan {
    /// The immediate objective being pursued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<String>,
    /// Steps that have been completed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completed_steps: Vec<String>,
    /// Steps that remain to be done.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remaining_steps: Vec<String>,
    /// Current execution phase (explore, implement, verify, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// Verification status snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MissionVerificationStatus {
    /// Freshness at snapshot time.
    pub freshness: crate::VerificationFreshness,
    /// Files modified but not yet verified.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unverified_files: Vec<String>,
    /// When the last verification check ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<DateTime<Utc>>,
    /// Verification strategies that passed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks_passed: Vec<String>,
    /// Verification strategies that failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks_failed: Vec<String>,
}

// ---------------------------------------------------------------------------
// MissionMemory — the main artifact
// ---------------------------------------------------------------------------

/// Durable mission memory artifact.
///
/// This is the single authoritative record of what the orchestrator knows,
/// believes, questions, and plans at any point in time. It is designed to be:
///
/// - **Serializable** to JSON for disk persistence
/// - **Loadable** across sessions for handoff/resume
/// - **Renderable** as human-readable text for review
/// - **Mergeable** from a previous mission for continuity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionMemory {
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// Session ID that created this mission memory.
    pub session_id: SessionId,
    /// When this mission memory was captured.
    pub created_at: DateTime<Utc>,
    /// The original user request / mission statement.
    pub mission: String,

    // -- Epistemic state --
    /// Verified facts — things confirmed by evidence.
    #[serde(default)]
    pub facts: Vec<MissionFact>,
    /// Active hypotheses — things believed but not yet confirmed.
    #[serde(default)]
    pub hypotheses: Vec<MissionHypothesis>,
    /// Open questions — things that need more information.
    #[serde(default)]
    pub open_questions: Vec<String>,

    // -- Plan state --
    /// Current plan / next objective.
    #[serde(default)]
    pub plan: MissionPlan,

    // -- Verification state --
    /// Verification status at snapshot time.
    #[serde(default)]
    pub verification: MissionVerificationStatus,

    // -- Artifact metadata --
    /// Files modified during the mission.
    #[serde(default)]
    pub modified_files: Vec<String>,
    /// Key decisions made during the mission.
    #[serde(default)]
    pub key_decisions: Vec<String>,
    /// Risks identified but not yet mitigated.
    #[serde(default)]
    pub risks: Vec<String>,
    /// Overall trust verdict.
    pub trust_verdict: crate::TrustVerdict,
    /// Human-readable narrative summary of the mission state.
    #[serde(default)]
    pub narrative: String,
}

impl MissionMemory {
    /// Build a mission memory from the current orchestration working memory
    /// and verification state.
    pub fn from_working_state(
        session_id: SessionId,
        mission: &str,
        memory: &crate::WorkingMemory,
        verification: &crate::VerificationState,
        trust_verdict: crate::TrustVerdict,
    ) -> Self {
        let facts: Vec<MissionFact> = memory
            .verified_facts
            .iter()
            .map(|f| MissionFact {
                content: f.content.clone(),
                source: f.source.clone(),
                established_at: f.created_at,
            })
            .collect();

        let hypotheses: Vec<MissionHypothesis> = memory
            .hypotheses
            .iter()
            .filter(|h| h.status == crate::MemoryStatus::Active)
            .map(|h| MissionHypothesis {
                content: h.content.clone(),
                confidence_pct: (h.effective_confidence() * 100.0).round() as u8,
                supporting_evidence: h
                    .supporting_evidence
                    .iter()
                    .map(|id| format!("evidence:{id}"))
                    .collect(),
            })
            .collect();

        let open_questions: Vec<String> =
            memory.questions.iter().map(|q| q.content.clone()).collect();

        // Build plan from working memory
        let plan = MissionPlan {
            current_objective: memory
                .planner_state
                .as_ref()
                .and_then(|ps| ps.current_step.clone()),
            completed_steps: Vec::new(), // Not tracked in WorkingMemory today
            remaining_steps: memory.plan.clone(),
            phase: memory
                .planner_state
                .as_ref()
                .and_then(|ps| ps.phase.clone()),
        };

        // Build verification status
        let last_check = verification
            .runs
            .iter()
            .max_by_key(|r| r.timestamp)
            .map(|r| r.timestamp);

        let last_verify_ts = last_check;
        let unverified_files: Vec<String> = verification
            .modified_files
            .iter()
            .filter(|(_, mod_time)| last_verify_ts.is_none_or(|vt| **mod_time > vt))
            .map(|(path, _)| path.clone())
            .collect();

        let checks_passed: Vec<String> = verification
            .runs
            .iter()
            .filter(|r| r.passed)
            .map(|r| r.strategy.clone())
            .collect();

        let checks_failed: Vec<String> = verification
            .runs
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.strategy.clone())
            .collect();

        let ver_status = MissionVerificationStatus {
            freshness: verification.freshness(),
            unverified_files,
            last_check,
            checks_passed,
            checks_failed,
        };

        let modified_files: Vec<String> = verification.modified_files.keys().cloned().collect();

        Self {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id,
            created_at: Utc::now(),
            mission: mission.to_string(),
            facts,
            hypotheses,
            open_questions,
            plan,
            verification: ver_status,
            modified_files,
            key_decisions: Vec::new(),
            risks: Vec::new(),
            trust_verdict,
            narrative: String::new(),
        }
    }

    /// True if this mission memory contains any substantive content.
    pub fn has_content(&self) -> bool {
        !self.facts.is_empty()
            || !self.hypotheses.is_empty()
            || !self.open_questions.is_empty()
            || self.plan.current_objective.is_some()
            || !self.plan.remaining_steps.is_empty()
    }

    /// Render as a human-readable handoff text block.
    pub fn to_handoff_text(&self) -> String {
        let mut sections = Vec::new();

        // Header
        sections.push(format!(
            "OCO Mission Handoff\n\
             ====================\n\
             Mission: {}\n\
             Session: {}\n\
             Captured: {}\n\
             Trust: {}",
            self.mission,
            self.session_id.0,
            self.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            self.trust_verdict.label(),
        ));

        // Facts
        if !self.facts.is_empty() {
            let items: Vec<String> = self
                .facts
                .iter()
                .map(|f| {
                    if let Some(ref src) = f.source {
                        format!("  - {} (source: {})", f.content, src)
                    } else {
                        format!("  - {}", f.content)
                    }
                })
                .collect();
            sections.push(format!(
                "VERIFIED FACTS ({}):\n{}",
                self.facts.len(),
                items.join("\n")
            ));
        }

        // Hypotheses
        if !self.hypotheses.is_empty() {
            let items: Vec<String> = self
                .hypotheses
                .iter()
                .map(|h| format!("  - {} (confidence: {}%)", h.content, h.confidence_pct))
                .collect();
            sections.push(format!(
                "ACTIVE HYPOTHESES ({}):\n{}",
                self.hypotheses.len(),
                items.join("\n")
            ));
        }

        // Open questions
        if !self.open_questions.is_empty() {
            let items: Vec<String> = self
                .open_questions
                .iter()
                .map(|q| format!("  ? {q}"))
                .collect();
            sections.push(format!(
                "OPEN QUESTIONS ({}):\n{}",
                self.open_questions.len(),
                items.join("\n")
            ));
        }

        // Plan
        {
            let mut plan_parts = Vec::new();
            if let Some(ref obj) = self.plan.current_objective {
                plan_parts.push(format!("  Current objective: {obj}"));
            }
            if let Some(ref phase) = self.plan.phase {
                plan_parts.push(format!("  Phase: {phase}"));
            }
            if !self.plan.completed_steps.is_empty() {
                for (i, s) in self.plan.completed_steps.iter().enumerate() {
                    plan_parts.push(format!("  [done] {}. {s}", i + 1));
                }
            }
            if !self.plan.remaining_steps.is_empty() {
                for (i, s) in self.plan.remaining_steps.iter().enumerate() {
                    plan_parts.push(format!("  [todo] {}. {s}", i + 1));
                }
            }
            if !plan_parts.is_empty() {
                sections.push(format!("PLAN:\n{}", plan_parts.join("\n")));
            }
        }

        // Verification
        {
            let mut ver_parts = Vec::new();
            ver_parts.push(format!("  Freshness: {:?}", self.verification.freshness));
            if let Some(ts) = self.verification.last_check {
                ver_parts.push(format!("  Last check: {}", ts.format("%H:%M:%S UTC")));
            }
            if !self.verification.checks_passed.is_empty() {
                ver_parts.push(format!(
                    "  Passed: {}",
                    self.verification.checks_passed.join(", ")
                ));
            }
            if !self.verification.checks_failed.is_empty() {
                ver_parts.push(format!(
                    "  Failed: {}",
                    self.verification.checks_failed.join(", ")
                ));
            }
            if !self.verification.unverified_files.is_empty() {
                for f in &self.verification.unverified_files {
                    ver_parts.push(format!("  ! {f}"));
                }
            }
            sections.push(format!("VERIFICATION:\n{}", ver_parts.join("\n")));
        }

        // Modified files
        if !self.modified_files.is_empty() {
            let items: Vec<String> = self
                .modified_files
                .iter()
                .map(|f| format!("  - {f}"))
                .collect();
            sections.push(format!(
                "MODIFIED FILES ({}):\n{}",
                self.modified_files.len(),
                items.join("\n")
            ));
        }

        // Key decisions
        if !self.key_decisions.is_empty() {
            let items: Vec<String> = self
                .key_decisions
                .iter()
                .map(|d| format!("  - {d}"))
                .collect();
            sections.push(format!("KEY DECISIONS:\n{}", items.join("\n")));
        }

        // Risks
        if !self.risks.is_empty() {
            let items: Vec<String> = self.risks.iter().map(|r| format!("  ! {r}")).collect();
            sections.push(format!("RISKS:\n{}", items.join("\n")));
        }

        // Narrative
        if !self.narrative.is_empty() {
            sections.push(format!("NARRATIVE:\n  {}", self.narrative));
        }

        sections.join("\n\n")
    }

    /// Persist to a JSON file.
    pub fn save_to(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load from a JSON file. Returns an error if the file doesn't exist
    /// or cannot be parsed.
    pub fn load_from(path: &Path) -> Result<Self, MissionLoadError> {
        if !path.exists() {
            return Err(MissionLoadError::NotFound(path.display().to_string()));
        }
        let json =
            std::fs::read_to_string(path).map_err(|e| MissionLoadError::Io(e.to_string()))?;

        // Check schema version before full deserialization
        let raw: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| MissionLoadError::Parse(e.to_string()))?;

        if let Some(version) = raw.get("schema_version").and_then(|v| v.as_u64())
            && version as u32 > MISSION_SCHEMA_VERSION
        {
            return Err(MissionLoadError::IncompatibleSchema {
                found: version as u32,
                expected: MISSION_SCHEMA_VERSION,
            });
        }

        serde_json::from_value(raw).map_err(|e| MissionLoadError::Parse(e.to_string()))
    }

    /// Merge facts, hypotheses, and questions from a previous mission memory.
    ///
    /// Used when resuming: the previous mission's knowledge is carried forward.
    /// Duplicate facts (by content) are deduplicated.
    pub fn merge_from_previous(&mut self, previous: &MissionMemory) {
        // Merge facts — deduplicate by content
        let existing_facts: std::collections::HashSet<String> =
            self.facts.iter().map(|f| f.content.clone()).collect();
        for fact in &previous.facts {
            if !existing_facts.contains(&fact.content) {
                self.facts.push(fact.clone());
            }
        }

        // Merge hypotheses — deduplicate by content
        let existing_hyp: std::collections::HashSet<String> =
            self.hypotheses.iter().map(|h| h.content.clone()).collect();
        for hyp in &previous.hypotheses {
            if !existing_hyp.contains(&hyp.content) {
                self.hypotheses.push(hyp.clone());
            }
        }

        // Merge open questions — deduplicate
        let existing_q: std::collections::HashSet<String> =
            self.open_questions.iter().cloned().collect();
        for q in &previous.open_questions {
            if !existing_q.contains(q) {
                self.open_questions.push(q.clone());
            }
        }

        // Carry forward key decisions
        let existing_dec: std::collections::HashSet<String> =
            self.key_decisions.iter().cloned().collect();
        for d in &previous.key_decisions {
            if !existing_dec.contains(d) {
                self.key_decisions.push(d.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when loading a mission memory from disk.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MissionLoadError {
    #[error("mission file not found: {0}")]
    NotFound(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("incompatible schema version: found {found}, expected <= {expected}")]
    IncompatibleSchema { found: u32, expected: u32 },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MemoryEntry, MemoryStatus, PlannerState, TrustVerdict, VerificationFreshness,
        VerificationState, WorkingMemory,
    };

    fn sample_mission_memory() -> MissionMemory {
        MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "Fix authentication bug in login flow".to_string(),
            facts: vec![
                MissionFact {
                    content: "JWT middleware validates on every request".to_string(),
                    source: Some("src/auth/middleware.rs".to_string()),
                    established_at: Utc::now(),
                },
                MissionFact {
                    content: "Rate limiter uses atomic counter".to_string(),
                    source: None,
                    established_at: Utc::now(),
                },
            ],
            hypotheses: vec![MissionHypothesis {
                content: "Session cookie is not HttpOnly".to_string(),
                confidence_pct: 70,
                supporting_evidence: vec!["Set-Cookie header missing flag".to_string()],
            }],
            open_questions: vec!["Does rate limiter handle clock skew?".to_string()],
            plan: MissionPlan {
                current_objective: Some("Fix HttpOnly flag".to_string()),
                completed_steps: vec!["Investigate cookie handling".to_string()],
                remaining_steps: vec![
                    "Add HttpOnly flag".to_string(),
                    "Add integration test".to_string(),
                ],
                phase: Some("implement".to_string()),
            },
            verification: MissionVerificationStatus {
                freshness: VerificationFreshness::Stale,
                unverified_files: vec!["src/auth/cookies.rs".to_string()],
                last_check: Some(Utc::now()),
                checks_passed: vec!["build".to_string()],
                checks_failed: vec!["test".to_string()],
            },
            modified_files: vec!["src/auth/cookies.rs".to_string()],
            key_decisions: vec!["Chose direct fix over refactor".to_string()],
            risks: vec!["Cookie change may break existing sessions".to_string()],
            trust_verdict: TrustVerdict::Medium,
            narrative:
                "Auth bug investigation in progress. HttpOnly flag missing on session cookie."
                    .to_string(),
        }
    }

    // -- Serde roundtrip --

    #[test]
    fn serde_roundtrip() {
        let original = sample_mission_memory();
        let json = serde_json::to_string_pretty(&original).unwrap();
        let restored: MissionMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn serde_roundtrip_empty() {
        let empty = MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "empty mission".to_string(),
            facts: vec![],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: TrustVerdict::None,
            narrative: String::new(),
        };
        let json = serde_json::to_string(&empty).unwrap();
        let restored: MissionMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(empty.mission, restored.mission);
        assert!(!restored.has_content());
    }

    // -- from_working_state --

    #[test]
    fn from_working_state_captures_all_categories() {
        let mut memory = WorkingMemory::default();

        // Add a verified fact
        let fact = MemoryEntry::new("auth uses JWT".to_string(), 1.0);
        let fact_id = fact.id;
        memory.add_finding(fact);
        memory.promote_to_fact(fact_id);

        // Add a hypothesis
        memory.add_hypothesis(MemoryEntry::new("cookie might be stale".to_string(), 0.6));

        // Add a question
        memory.add_question(MemoryEntry::new(
            "which middleware runs first?".to_string(),
            0.5,
        ));

        // Set plan
        memory.update_plan(vec!["fix middleware".to_string(), "test".to_string()]);
        memory.update_planner_state(PlannerState {
            current_step: Some("fix middleware".to_string()),
            replan_count: 0,
            phase: Some("implement".to_string()),
            lease_id: None,
        });

        let verification = VerificationState::default();
        let session_id = SessionId::new();

        let mm = MissionMemory::from_working_state(
            session_id,
            "fix auth bug",
            &memory,
            &verification,
            TrustVerdict::Medium,
        );

        assert_eq!(mm.mission, "fix auth bug");
        assert_eq!(mm.facts.len(), 1);
        assert_eq!(mm.facts[0].content, "auth uses JWT");
        assert_eq!(mm.hypotheses.len(), 1);
        assert_eq!(mm.hypotheses[0].content, "cookie might be stale");
        assert!(mm.hypotheses[0].confidence_pct <= 100);
        assert_eq!(mm.open_questions.len(), 1);
        assert_eq!(mm.plan.current_objective.as_deref(), Some("fix middleware"));
        assert_eq!(mm.plan.remaining_steps.len(), 2);
        assert_eq!(mm.plan.phase.as_deref(), Some("implement"));
        assert_eq!(mm.trust_verdict, TrustVerdict::Medium);
        assert_eq!(mm.schema_version, MISSION_SCHEMA_VERSION);
        assert!(mm.has_content());
    }

    #[test]
    fn from_working_state_empty_memory() {
        let memory = WorkingMemory::default();
        let verification = VerificationState::default();
        let mm = MissionMemory::from_working_state(
            SessionId::new(),
            "nothing",
            &memory,
            &verification,
            TrustVerdict::None,
        );
        assert!(!mm.has_content());
    }

    #[test]
    fn from_working_state_filters_inactive_hypotheses() {
        let mut memory = WorkingMemory::default();
        let mut active = MemoryEntry::new("active hyp".to_string(), 0.7);
        active.status = MemoryStatus::Active;
        memory.add_hypothesis(active);

        let mut stale = MemoryEntry::new("stale hyp".to_string(), 0.3);
        stale.status = MemoryStatus::Stale;
        memory.hypotheses.push(stale);

        let verification = VerificationState::default();
        let mm = MissionMemory::from_working_state(
            SessionId::new(),
            "test",
            &memory,
            &verification,
            TrustVerdict::None,
        );
        assert_eq!(mm.hypotheses.len(), 1);
        assert_eq!(mm.hypotheses[0].content, "active hyp");
    }

    // -- to_handoff_text --

    #[test]
    fn handoff_text_contains_all_sections() {
        let mm = sample_mission_memory();
        let text = mm.to_handoff_text();

        assert!(text.contains("OCO Mission Handoff"));
        assert!(text.contains("Fix authentication bug"));
        assert!(text.contains("Trust: medium"));
        assert!(text.contains("VERIFIED FACTS (2)"));
        assert!(text.contains("JWT middleware validates"));
        assert!(text.contains("source: src/auth/middleware.rs"));
        assert!(text.contains("ACTIVE HYPOTHESES (1)"));
        assert!(text.contains("confidence: 70%"));
        assert!(text.contains("OPEN QUESTIONS (1)"));
        assert!(text.contains("clock skew"));
        assert!(text.contains("PLAN:"));
        assert!(text.contains("Fix HttpOnly flag"));
        assert!(text.contains("Phase: implement"));
        assert!(text.contains("VERIFICATION:"));
        assert!(text.contains("Stale"));
        assert!(text.contains("MODIFIED FILES"));
        assert!(text.contains("KEY DECISIONS"));
        assert!(text.contains("RISKS"));
        assert!(text.contains("NARRATIVE"));
    }

    #[test]
    fn handoff_text_empty_mission() {
        let mm = MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "empty".to_string(),
            facts: vec![],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: TrustVerdict::None,
            narrative: String::new(),
        };
        let text = mm.to_handoff_text();
        assert!(text.contains("OCO Mission Handoff"));
        assert!(text.contains("Trust: none"));
        // Should not contain empty sections
        assert!(!text.contains("VERIFIED FACTS"));
        assert!(!text.contains("ACTIVE HYPOTHESES"));
    }

    // -- Persistence --

    #[test]
    fn persistence_roundtrip() {
        let mm = sample_mission_memory();
        let dir = std::env::temp_dir().join("oco-test-mission-memory");
        let path = dir.join("mission.json");

        mm.save_to(&path).unwrap();

        let loaded = MissionMemory::load_from(&path).unwrap();
        assert_eq!(mm, loaded);

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_nonexistent_returns_not_found() {
        let path = std::path::Path::new("/tmp/oco-nonexistent-99999/mission.json");
        let result = MissionMemory::load_from(path);
        assert!(matches!(result, Err(MissionLoadError::NotFound(_))));
    }

    #[test]
    fn load_from_incompatible_schema() {
        let dir = std::env::temp_dir().join("oco-test-mission-schema");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mission.json");

        // Write a mission with a future schema version
        let json = serde_json::json!({
            "schema_version": 999,
            "session_id": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "created_at": "2026-01-01T00:00:00Z",
            "mission": "test",
            "trust_verdict": "none"
        });
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let result = MissionMemory::load_from(&path);
        assert!(matches!(
            result,
            Err(MissionLoadError::IncompatibleSchema { found: 999, .. })
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    // -- Merge --

    #[test]
    fn merge_from_previous_deduplicates() {
        let mut current = MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "current".to_string(),
            facts: vec![MissionFact {
                content: "fact A".to_string(),
                source: None,
                established_at: Utc::now(),
            }],
            hypotheses: vec![],
            open_questions: vec!["q1".to_string()],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: TrustVerdict::None,
            narrative: String::new(),
        };

        let previous = MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "previous".to_string(),
            facts: vec![
                MissionFact {
                    content: "fact A".to_string(), // duplicate
                    source: None,
                    established_at: Utc::now(),
                },
                MissionFact {
                    content: "fact B".to_string(), // new
                    source: None,
                    established_at: Utc::now(),
                },
            ],
            hypotheses: vec![MissionHypothesis {
                content: "hyp from previous".to_string(),
                confidence_pct: 50,
                supporting_evidence: vec![],
            }],
            open_questions: vec!["q1".to_string(), "q2".to_string()],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec!["decision X".to_string()],
            risks: vec![],
            trust_verdict: TrustVerdict::None,
            narrative: String::new(),
        };

        current.merge_from_previous(&previous);

        // fact A not duplicated, fact B added
        assert_eq!(current.facts.len(), 2);
        assert_eq!(current.facts[1].content, "fact B");

        // hypothesis merged
        assert_eq!(current.hypotheses.len(), 1);
        assert_eq!(current.hypotheses[0].content, "hyp from previous");

        // q1 not duplicated, q2 added
        assert_eq!(current.open_questions.len(), 2);
        assert_eq!(current.open_questions[1], "q2");

        // key decision merged
        assert_eq!(current.key_decisions.len(), 1);
        assert_eq!(current.key_decisions[0], "decision X");
    }

    // -- has_content --

    #[test]
    fn has_content_with_facts() {
        let mut mm = MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "test".to_string(),
            facts: vec![],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: TrustVerdict::None,
            narrative: String::new(),
        };
        assert!(!mm.has_content());

        mm.facts.push(MissionFact {
            content: "a fact".to_string(),
            source: None,
            established_at: Utc::now(),
        });
        assert!(mm.has_content());
    }

    #[test]
    fn has_content_with_plan_objective() {
        let mm = MissionMemory {
            schema_version: MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "test".to_string(),
            facts: vec![],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan {
                current_objective: Some("do something".to_string()),
                ..Default::default()
            },
            verification: MissionVerificationStatus::default(),
            modified_files: vec![],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: TrustVerdict::None,
            narrative: String::new(),
        };
        assert!(mm.has_content());
    }

    #[test]
    fn schema_version_field_present() {
        let mm = sample_mission_memory();
        let json: serde_json::Value = serde_json::to_value(&mm).unwrap();
        assert_eq!(
            json["schema_version"].as_u64().unwrap(),
            MISSION_SCHEMA_VERSION as u64
        );
    }
}
