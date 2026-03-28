//! Work protocols (#65) — typed agent collaboration patterns.
//!
//! Instead of exposing raw topologies (Mesh, HubSpoke, Pipeline),
//! expose concrete **work protocols** with typed roles and artifacts.
//! Claude Code follows protocols, not abstract topology decisions.
//!
//! Each protocol defines:
//! - An ordered set of roles
//! - What artifact each role produces
//! - How artifacts flow between roles

use serde::{Deserialize, Serialize};

/// A typed work protocol that agents follow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkProtocol {
    /// Protocol identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Ordered list of roles in this protocol.
    pub roles: Vec<RoleSpec>,
    /// How data flows between roles.
    pub flow: ProtocolFlow,
    /// When to use this protocol (for auto-selection).
    pub applicable_when: Vec<String>,
}

impl WorkProtocol {
    /// Get the role specs in execution order.
    pub fn execution_order(&self) -> &[RoleSpec] {
        &self.roles
    }

    /// Get a role by name.
    pub fn role(&self, name: &str) -> Option<&RoleSpec> {
        self.roles.iter().find(|r| r.name == name)
    }

    /// Validate that all required artifacts have been produced.
    pub fn validate_artifacts(&self, produced: &[(&str, &str)]) -> Vec<String> {
        let mut missing = Vec::new();
        for role in &self.roles {
            if role.artifact.required {
                let found = produced
                    .iter()
                    .any(|(role_name, _)| *role_name == role.name);
                if !found {
                    missing.push(format!(
                        "Role '{}' has not produced artifact '{}'",
                        role.name, role.artifact.name
                    ));
                }
            }
        }
        missing
    }
}

/// Specification of a single role in a work protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSpec {
    /// Role name (e.g., "investigator", "implementer", "verifier").
    pub name: String,
    /// What this role does.
    pub description: String,
    /// The artifact this role must produce.
    pub artifact: RoleArtifact,
    /// Preferred model tier for this role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_model: Option<String>,
    /// Capabilities required by this role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// A typed artifact that a role produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleArtifact {
    /// Artifact name (e.g., "investigation_report", "patch_set").
    pub name: String,
    /// What the artifact should contain.
    pub schema_hint: String,
    /// Whether this artifact is required for protocol completion.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

impl RoleArtifact {
    pub fn required(name: impl Into<String>, schema_hint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema_hint: schema_hint.into(),
            required: true,
        }
    }

    pub fn optional(name: impl Into<String>, schema_hint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema_hint: schema_hint.into(),
            required: false,
        }
    }
}

/// How data flows between protocol roles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFlow {
    /// Sequential: each role's artifact feeds the next.
    Pipeline,
    /// Hub: coordinator distributes work, collects results.
    Hub,
    /// Parallel: all roles work independently, results merged.
    Parallel,
}

// ---------------------------------------------------------------------------
// Predefined protocols
// ---------------------------------------------------------------------------

/// Investigation protocol: investigate → synthesize → implement → verify.
pub fn investigation_protocol() -> WorkProtocol {
    WorkProtocol {
        id: "investigation".into(),
        name: "Investigation Pipeline".into(),
        roles: vec![
            RoleSpec {
                name: "investigator".into(),
                description: "Explore the codebase, gather evidence, narrow scope".into(),
                artifact: RoleArtifact::required(
                    "investigation_report",
                    "{ findings: [], inspected_areas: [], hypotheses: [] }",
                ),
                preferred_model: Some("haiku".into()),
                capabilities: vec!["code_search".into(), "file_read".into()],
            },
            RoleSpec {
                name: "synthesizer".into(),
                description: "Synthesize findings into a coherent analysis".into(),
                artifact: RoleArtifact::required(
                    "synthesis",
                    "{ root_cause: string, confidence: number, evidence: [] }",
                ),
                preferred_model: Some("sonnet".into()),
                capabilities: vec!["code_search".into()],
            },
            RoleSpec {
                name: "implementer".into(),
                description: "Implement the fix based on the synthesis".into(),
                artifact: RoleArtifact::required(
                    "patch_set",
                    "{ files_modified: [], summary: string }",
                ),
                preferred_model: Some("sonnet".into()),
                capabilities: vec!["file_edit".into(), "code_search".into()],
            },
            RoleSpec {
                name: "verifier".into(),
                description: "Verify the implementation passes all checks".into(),
                artifact: RoleArtifact::required(
                    "verification_result",
                    "{ passed: bool, checks: { build, test, lint } }",
                ),
                preferred_model: Some("haiku".into()),
                capabilities: vec!["test_run".into(), "build".into()],
            },
        ],
        flow: ProtocolFlow::Pipeline,
        applicable_when: vec!["bug".into(), "regression".into(), "investigation".into()],
    }
}

/// Code review protocol: coordinator distributes review to parallel analysts.
pub fn review_protocol() -> WorkProtocol {
    WorkProtocol {
        id: "review".into(),
        name: "Hub Review".into(),
        roles: vec![
            RoleSpec {
                name: "coordinator".into(),
                description: "Orchestrate the review, assign areas, collect results".into(),
                artifact: RoleArtifact::required("review_plan", "{ areas: [], assignments: {} }"),
                preferred_model: Some("sonnet".into()),
                capabilities: vec!["code_search".into()],
            },
            RoleSpec {
                name: "symbol_analyst".into(),
                description: "Analyze symbol usage, dependencies, and contracts".into(),
                artifact: RoleArtifact::required(
                    "symbol_analysis",
                    "{ symbols: [], impact: [], breaking_changes: [] }",
                ),
                preferred_model: Some("haiku".into()),
                capabilities: vec!["code_search".into(), "file_read".into()],
            },
            RoleSpec {
                name: "test_analyst".into(),
                description: "Analyze test coverage and gaps".into(),
                artifact: RoleArtifact::required(
                    "test_coverage_report",
                    "{ covered: [], gaps: [], suggested_tests: [] }",
                ),
                preferred_model: Some("haiku".into()),
                capabilities: vec!["code_search".into(), "test_run".into()],
            },
        ],
        flow: ProtocolFlow::Hub,
        applicable_when: vec!["review".into(), "refactor".into(), "security".into()],
    }
}

/// Quick fix protocol: direct implementation + verification.
pub fn quick_fix_protocol() -> WorkProtocol {
    WorkProtocol {
        id: "quick_fix".into(),
        name: "Quick Fix".into(),
        roles: vec![
            RoleSpec {
                name: "fixer".into(),
                description: "Investigate briefly and implement the fix".into(),
                artifact: RoleArtifact::required(
                    "patch_set",
                    "{ files_modified: [], summary: string }",
                ),
                preferred_model: Some("sonnet".into()),
                capabilities: vec!["file_edit".into(), "code_search".into()],
            },
            RoleSpec {
                name: "verifier".into(),
                description: "Verify the fix".into(),
                artifact: RoleArtifact::required(
                    "verification_result",
                    "{ passed: bool, checks: {} }",
                ),
                preferred_model: Some("haiku".into()),
                capabilities: vec!["test_run".into(), "build".into()],
            },
        ],
        flow: ProtocolFlow::Pipeline,
        applicable_when: vec!["simple_bug".into(), "trivial".into(), "low".into()],
    }
}

/// Select the best protocol for a task category and complexity.
pub fn select_protocol(category: &str, complexity: &str) -> WorkProtocol {
    match (category, complexity) {
        ("bug" | "regression", "medium" | "high" | "critical") => investigation_protocol(),
        ("review" | "security" | "refactor", "medium" | "high" | "critical") => review_protocol(),
        _ => quick_fix_protocol(),
    }
}

/// List all available protocols.
pub fn all_protocols() -> Vec<WorkProtocol> {
    vec![
        investigation_protocol(),
        review_protocol(),
        quick_fix_protocol(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn investigation_protocol_has_4_roles() {
        let p = investigation_protocol();
        assert_eq!(p.roles.len(), 4);
        assert_eq!(p.flow, ProtocolFlow::Pipeline);
        assert_eq!(p.roles[0].name, "investigator");
        assert_eq!(p.roles[3].name, "verifier");
    }

    #[test]
    fn review_protocol_is_hub() {
        let p = review_protocol();
        assert_eq!(p.flow, ProtocolFlow::Hub);
        assert!(p.role("coordinator").is_some());
        assert!(p.role("symbol_analyst").is_some());
    }

    #[test]
    fn quick_fix_protocol_is_minimal() {
        let p = quick_fix_protocol();
        assert_eq!(p.roles.len(), 2);
        assert_eq!(p.flow, ProtocolFlow::Pipeline);
    }

    #[test]
    fn validate_artifacts_all_present() {
        let p = quick_fix_protocol();
        let produced = vec![("fixer", "patch data"), ("verifier", "pass")];
        let missing = p.validate_artifacts(&produced);
        assert!(missing.is_empty());
    }

    #[test]
    fn validate_artifacts_missing_role() {
        let p = quick_fix_protocol();
        let produced = vec![("fixer", "patch data")];
        let missing = p.validate_artifacts(&produced);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("verifier"));
    }

    #[test]
    fn select_protocol_bug_medium_returns_investigation() {
        let p = select_protocol("bug", "medium");
        assert_eq!(p.id, "investigation");
    }

    #[test]
    fn select_protocol_review_high_returns_review() {
        let p = select_protocol("review", "high");
        assert_eq!(p.id, "review");
    }

    #[test]
    fn select_protocol_trivial_returns_quick_fix() {
        let p = select_protocol("general", "trivial");
        assert_eq!(p.id, "quick_fix");
    }

    #[test]
    fn all_protocols_returns_three() {
        assert_eq!(all_protocols().len(), 3);
    }

    #[test]
    fn role_lookup_by_name() {
        let p = investigation_protocol();
        let synthesizer = p.role("synthesizer").unwrap();
        assert_eq!(synthesizer.preferred_model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn protocol_serialization_roundtrip() {
        let p = investigation_protocol();
        let json = serde_json::to_string(&p).unwrap();
        let parsed: WorkProtocol = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "investigation");
        assert_eq!(parsed.roles.len(), 4);
    }
}
