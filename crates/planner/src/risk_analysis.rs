//! Failure-First risk analysis (#67).
//!
//! Before generating a plan for Medium+ tasks, analyze how the task
//! is likely to fail if treated naively. The output feeds into:
//! - verify gates in the execution plan
//! - risk context in the planner prompt
//! - failure previews returned to the caller

use oco_shared_types::{TaskCategory, TaskComplexity};
use serde::{Deserialize, Serialize};

use crate::context::PlanningContext;

/// Preview of likely failure modes for a task.
/// Generated BEFORE the plan, not after.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailurePreview {
    /// Primary risks: how this task is likely to fail.
    pub risks: Vec<Risk>,
    /// Zones of uncertainty: things we don't know yet.
    pub uncertainties: Vec<String>,
    /// Artifacts missing before we can safely proceed.
    pub missing_artifacts: Vec<String>,
    /// Suggested verify gates derived from the risks.
    pub suggested_verify_gates: Vec<String>,
    /// Overall risk level (0.0 = safe, 1.0 = dangerous).
    pub risk_score: f64,
}

/// A specific risk identified for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Risk {
    /// Short identifier.
    pub id: String,
    /// What could go wrong.
    pub description: String,
    /// How likely this is (0.0 to 1.0).
    pub likelihood: f64,
    /// How bad it would be if it happens (0.0 to 1.0).
    pub severity: f64,
    /// Mitigation: what verify gate or step would catch this.
    pub mitigation: String,
}

impl Risk {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        likelihood: f64,
        severity: f64,
        mitigation: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            likelihood: likelihood.clamp(0.0, 1.0),
            severity: severity.clamp(0.0, 1.0),
            mitigation: mitigation.into(),
        }
    }

    /// Combined risk score: likelihood × severity.
    pub fn score(&self) -> f64 {
        self.likelihood * self.severity
    }
}

/// Analyze a task for failure modes. Deterministic — no LLM call.
/// Uses category, complexity, and repo profile heuristics.
pub fn analyze_risks(request: &str, context: &PlanningContext) -> FailurePreview {
    let mut preview = FailurePreview::default();
    let request_lower = request.to_lowercase();
    // Tokenize on word boundaries for keyword matching (avoids "authorization" matching "auth")
    let tokens: Vec<&str> = request_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();

    // --- Category-specific risks ---
    match context.task_category {
        TaskCategory::Bug => {
            preview.risks.push(Risk::new(
                "wrong_root_cause",
                "Fix addresses symptom, not root cause",
                0.6,
                0.8,
                "Verify gate: targeted test must fail before fix, pass after",
            ));
            preview
                .uncertainties
                .push("Root cause may not be in the obvious location".into());
            preview
                .suggested_verify_gates
                .push("regression_test".into());
        }
        TaskCategory::Refactor => {
            preview.risks.push(Risk::new(
                "missed_callers",
                "Renamed/moved symbol still referenced elsewhere",
                0.7,
                0.7,
                "Verify gate: full build + typecheck after changes",
            ));
            preview.risks.push(Risk::new(
                "behavior_change",
                "Refactor accidentally changes observable behavior",
                0.4,
                0.9,
                "Verify gate: existing test suite must pass unchanged",
            ));
            preview.suggested_verify_gates.push("build".into());
            preview.suggested_verify_gates.push("test".into());
            preview.suggested_verify_gates.push("typecheck".into());
        }
        TaskCategory::NewFeature => {
            preview.risks.push(Risk::new(
                "incomplete_integration",
                "New feature not wired into existing code paths",
                0.5,
                0.6,
                "Verify gate: integration test covering the new path",
            ));
            preview.suggested_verify_gates.push("build".into());
            preview.suggested_verify_gates.push("test".into());
        }
        TaskCategory::Security => {
            preview.risks.push(Risk::new(
                "incomplete_fix",
                "Security fix addresses one vector but misses similar patterns",
                0.5,
                0.95,
                "Verify gate: scan for similar patterns across codebase",
            ));
            preview
                .uncertainties
                .push("Attack surface may extend beyond the obvious location".into());
            preview.suggested_verify_gates.push("security_scan".into());
        }
        _ => {}
    }

    // --- Complexity-specific risks ---
    match context.task_complexity {
        TaskComplexity::High | TaskComplexity::Critical => {
            preview.risks.push(Risk::new(
                "scope_creep",
                "Task is larger than estimated, exhausts budget mid-execution",
                0.5,
                0.6,
                "Budget monitoring + early termination if >50% budget used in explore phase",
            ));
            preview
                .uncertainties
                .push("Full scope may only become clear during investigation".into());
        }
        TaskComplexity::Medium => {
            preview.risks.push(Risk::new(
                "premature_fix",
                "Jumping to implementation before understanding the full context",
                0.4,
                0.5,
                "Ensure explore step completes before implementation",
            ));
        }
        _ => {}
    }

    // --- Keyword-based risks (exact token match to avoid false positives) ---
    let has_token = |keywords: &[&str]| tokens.iter().any(|t| keywords.contains(t));

    if has_token(&[
        "auth",
        "authentication",
        "session",
        "token",
        "login",
        "logout",
    ]) {
        preview.risks.push(Risk::new(
            "auth_regression",
            "Changes to auth/session code may break authentication flows",
            0.6,
            0.9,
            "Verify gate: auth integration tests must pass",
        ));
        preview
            .missing_artifacts
            .push("Auth flow test coverage".into());
    }

    if has_token(&["database", "migration", "schema", "migrate", "db"]) {
        preview.risks.push(Risk::new(
            "data_loss",
            "Schema/migration changes may cause data loss or corruption",
            0.3,
            1.0,
            "Verify gate: migration reversibility check",
        ));
    }

    if has_token(&["delete", "remove", "drop", "purge", "destroy"]) {
        preview.risks.push(Risk::new(
            "unintended_deletion",
            "Deletion may remove more than intended or break dependents",
            0.4,
            0.7,
            "Verify gate: impact scan before deletion",
        ));
        preview
            .missing_artifacts
            .push("Dependency/caller analysis of deletion targets".into());
    }

    if has_token(&[
        "concurrent",
        "async",
        "parallel",
        "mutex",
        "race",
        "deadlock",
    ]) {
        preview.risks.push(Risk::new(
            "race_condition",
            "Concurrent code changes may introduce race conditions",
            0.5,
            0.8,
            "Verify gate: concurrent test scenarios",
        ));
    }

    // --- Repo-specific risks ---
    if matches!(
        context.repo_profile.risk_level,
        oco_shared_types::RiskLevel::High | oco_shared_types::RiskLevel::Critical
    ) {
        preview
            .uncertainties
            .push("High-risk repo: extra caution required".into());
    }

    // --- Compute overall risk score ---
    if !preview.risks.is_empty() {
        // Max individual risk score, boosted by count
        let max_score = preview
            .risks
            .iter()
            .map(|r| r.score())
            .fold(0.0f64, f64::max);
        let count_boost = ((preview.risks.len() as f64 - 1.0) * 0.05).min(0.2);
        preview.risk_score = (max_score + count_boost).clamp(0.0, 1.0);
    }

    // Dedup verify gates
    preview.suggested_verify_gates.sort();
    preview.suggested_verify_gates.dedup();

    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bug_task_identifies_root_cause_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::Bug);
        let preview = analyze_risks("fix the null pointer in auth handler", &ctx);

        assert!(!preview.risks.is_empty());
        assert!(preview.risks.iter().any(|r| r.id == "wrong_root_cause"));
        assert!(preview.risks.iter().any(|r| r.id == "auth_regression"));
        assert!(preview.risk_score > 0.0);
    }

    #[test]
    fn refactor_task_identifies_caller_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Refactor);
        let preview = analyze_risks("rename UserSession to AuthSession", &ctx);

        assert!(preview.risks.iter().any(|r| r.id == "missed_callers"));
        assert!(preview.risks.iter().any(|r| r.id == "behavior_change"));
        assert!(
            preview
                .suggested_verify_gates
                .contains(&"build".to_string())
        );
        assert!(
            preview
                .suggested_verify_gates
                .contains(&"typecheck".to_string())
        );
    }

    #[test]
    fn high_complexity_adds_scope_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::High, TaskCategory::NewFeature);
        let preview = analyze_risks("add OAuth2 support", &ctx);

        assert!(preview.risks.iter().any(|r| r.id == "scope_creep"));
        assert!(preview.risk_score > 0.0);
    }

    #[test]
    fn security_task_is_high_severity() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Security);
        let preview = analyze_risks("fix SQL injection in user search", &ctx);

        assert!(preview.risks.iter().any(|r| r.id == "incomplete_fix"));
        let security_risk = preview
            .risks
            .iter()
            .find(|r| r.id == "incomplete_fix")
            .unwrap();
        assert!(security_risk.severity > 0.9);
    }

    #[test]
    fn deletion_keywords_trigger_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::Refactor);
        let preview = analyze_risks("delete the unused UserCache module", &ctx);

        assert!(preview.risks.iter().any(|r| r.id == "unintended_deletion"));
        assert!(!preview.missing_artifacts.is_empty());
    }

    #[test]
    fn database_keywords_trigger_data_loss_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let preview = analyze_risks("add database migration for new user fields", &ctx);

        assert!(preview.risks.iter().any(|r| r.id == "data_loss"));
    }

    #[test]
    fn concurrent_keywords_trigger_race_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let preview = analyze_risks("implement async job queue with parallel workers", &ctx);

        assert!(preview.risks.iter().any(|r| r.id == "race_condition"));
    }

    #[test]
    fn trivial_general_task_has_low_risk() {
        let ctx = PlanningContext::minimal(TaskComplexity::Trivial, TaskCategory::Explanation);
        let preview = analyze_risks("explain what a linked list is", &ctx);

        assert!(preview.risks.is_empty());
        assert_eq!(preview.risk_score, 0.0);
    }

    #[test]
    fn risk_score_computation() {
        let ctx = PlanningContext::minimal(TaskComplexity::High, TaskCategory::Refactor);
        let preview = analyze_risks("refactor auth session with database migration", &ctx);

        // Multiple risks → score should be meaningful
        assert!(preview.risks.len() >= 3);
        assert!(preview.risk_score > 0.3);
        assert!(preview.risk_score <= 1.0);
    }

    #[test]
    fn verify_gates_deduped() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Refactor);
        let preview = analyze_risks("refactor", &ctx);

        let gates = &preview.suggested_verify_gates;
        let unique: std::collections::HashSet<_> = gates.iter().collect();
        assert_eq!(gates.len(), unique.len());
    }
}
