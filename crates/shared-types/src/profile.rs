use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Per-repository profile for stack-specific adaptation.
///
/// Loaded from `oco.toml` `[profile]` section or auto-detected from manifests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoProfile {
    /// Detected or declared stack (e.g. "rust", "node", "python", "go", "mixed").
    pub stack: String,
    /// Build command override (e.g. "cargo build", "npm run build").
    pub build_command: Option<String>,
    /// Test command override.
    pub test_command: Option<String>,
    /// Lint command override.
    pub lint_command: Option<String>,
    /// Type-check command override.
    pub typecheck_command: Option<String>,
    /// Paths considered sensitive (should be flagged on modification).
    pub sensitive_paths: Vec<String>,
    /// High-value directories that should get context priority.
    pub high_value_paths: Vec<String>,
    /// File patterns to always exclude from indexing.
    pub exclude_patterns: Vec<String>,
    /// Risk level for the project (affects verification strictness).
    pub risk_level: RiskLevel,
    /// Custom verification strategies.
    pub custom_verifications: Vec<CustomVerification>,
    /// Key-value metadata for extensibility.
    pub metadata: HashMap<String, String>,
    /// Per-task-type verification policies.
    pub task_policies: Vec<TaskTypePolicy>,
    /// Q3: Policy pack governing this repo's trust contract.
    #[serde(default)]
    pub policy_pack: PolicyPack,
}

// ── Q3: Policy Packs ─────────────────────────────────────

/// A named policy pack that governs verification strictness.
///
/// Each pack defines which checks are mandatory vs optional,
/// the minimum verification tier, and whether completion on
/// stale verification state is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolicyPack {
    /// Fastest iteration: build-only gate, stale completion allowed.
    Fast,
    /// Default: build + test required, stale completion blocked.
    #[default]
    Balanced,
    /// Production-grade: full verification suite, stale blocked,
    /// sensitive-path changes require thorough tier.
    Strict,
}

impl PolicyPack {
    /// Minimum verification tier enforced by this pack.
    pub fn minimum_tier(&self) -> crate::VerificationTier {
        match self {
            Self::Fast => crate::VerificationTier::Light,
            Self::Balanced => crate::VerificationTier::Standard,
            Self::Strict => crate::VerificationTier::Thorough,
        }
    }

    /// Strategies that are mandatory (must pass) under this pack.
    pub fn mandatory_strategies(&self) -> Vec<crate::VerificationStrategy> {
        use crate::VerificationStrategy;
        match self {
            Self::Fast => vec![VerificationStrategy::Build],
            Self::Balanced => vec![VerificationStrategy::Build, VerificationStrategy::RunTests],
            Self::Strict => vec![
                VerificationStrategy::Build,
                VerificationStrategy::RunTests,
                VerificationStrategy::Lint,
                VerificationStrategy::TypeCheck,
            ],
        }
    }

    /// Whether completion on stale verification is allowed.
    pub fn allows_stale_completion(&self) -> bool {
        matches!(self, Self::Fast)
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::Strict => "strict",
        }
    }
}

/// Verification policy for a specific type of task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTypePolicy {
    /// Task pattern to match (e.g. "debug", "refactor", "implement", "review").
    pub task_pattern: String,
    /// Override risk level for this task type.
    pub risk_level: Option<RiskLevel>,
    /// Required verification steps before completion.
    pub required_checks: Vec<String>,
    /// Maximum steps allowed for this task type (0 = no limit).
    #[serde(default)]
    pub max_steps: u32,
    /// Context priority paths specific to this task type.
    #[serde(default)]
    pub priority_paths: Vec<String>,
}

/// Risk level affects how aggressively verification is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Minimal verification — local scripts, experiments.
    Low,
    /// Standard verification — most projects.
    #[default]
    Standard,
    /// Strict verification — production code, security-sensitive.
    High,
    /// Maximum verification — critical infrastructure.
    Critical,
}

/// A custom verification step defined in the repo profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomVerification {
    /// Name for this verification step.
    pub name: String,
    /// Shell command to run.
    pub command: String,
    /// When to trigger: "always", "on_write", "on_test_fail".
    pub trigger: String,
    /// Timeout in seconds.
    pub timeout_secs: u64,
}

impl RepoProfile {
    /// Detect a profile from the workspace root by inspecting manifest files.
    pub fn detect(workspace_root: &std::path::Path) -> Self {
        let mut profile = Self::default();

        // Detect Rust
        if workspace_root.join("Cargo.toml").exists() {
            profile.stack = if profile.stack.is_empty() {
                "rust".into()
            } else {
                "mixed".into()
            };
            profile
                .build_command
                .get_or_insert_with(|| "cargo build".into());
            profile
                .test_command
                .get_or_insert_with(|| "cargo test".into());
            profile
                .lint_command
                .get_or_insert_with(|| "cargo clippy -- -D warnings".into());
            profile
                .typecheck_command
                .get_or_insert_with(|| "cargo check".into());
        }

        // Detect Node.js
        if workspace_root.join("package.json").exists() {
            profile.stack = if profile.stack.is_empty() {
                "node".into()
            } else {
                "mixed".into()
            };
            profile
                .build_command
                .get_or_insert_with(|| "npm run build".into());
            profile
                .test_command
                .get_or_insert_with(|| "npm test".into());
            profile
                .lint_command
                .get_or_insert_with(|| "npm run lint".into());
            profile
                .typecheck_command
                .get_or_insert_with(|| "npx tsc --noEmit".into());
        }

        // Detect Python
        if workspace_root.join("pyproject.toml").exists()
            || workspace_root.join("setup.py").exists()
        {
            profile.stack = if profile.stack.is_empty() {
                "python".into()
            } else {
                "mixed".into()
            };
            profile
                .build_command
                .get_or_insert_with(|| "python -m build".into());
            profile.test_command.get_or_insert_with(|| "pytest".into());
            profile
                .lint_command
                .get_or_insert_with(|| "ruff check .".into());
            profile
                .typecheck_command
                .get_or_insert_with(|| "mypy --strict .".into());
        }

        // Detect Go
        if workspace_root.join("go.mod").exists() {
            profile.stack = if profile.stack.is_empty() {
                "go".into()
            } else {
                "mixed".into()
            };
            profile
                .build_command
                .get_or_insert_with(|| "go build ./...".into());
            profile
                .test_command
                .get_or_insert_with(|| "go test ./...".into());
            profile
                .lint_command
                .get_or_insert_with(|| "golangci-lint run".into());
            profile
                .typecheck_command
                .get_or_insert_with(|| "go vet ./...".into());
        }

        // Default sensitive paths
        if profile.sensitive_paths.is_empty() {
            profile.sensitive_paths = vec![
                ".env".into(),
                ".env.local".into(),
                ".env.production".into(),
                "credentials.json".into(),
                "secrets.yaml".into(),
                "*.pem".into(),
                "*.key".into(),
            ];
        }

        if profile.stack.is_empty() {
            profile.stack = "unknown".into();
        }

        // Default task policies based on common patterns.
        if profile.task_policies.is_empty() {
            profile.task_policies = vec![
                TaskTypePolicy {
                    task_pattern: "debug".into(),
                    risk_level: Some(RiskLevel::Standard),
                    required_checks: vec!["test".into()],
                    max_steps: 0,
                    priority_paths: vec!["src/".into(), "tests/".into()],
                },
                TaskTypePolicy {
                    task_pattern: "refactor".into(),
                    risk_level: Some(RiskLevel::High),
                    required_checks: vec!["build".into(), "test".into(), "lint".into()],
                    max_steps: 0,
                    priority_paths: vec![],
                },
                TaskTypePolicy {
                    task_pattern: "implement".into(),
                    risk_level: Some(RiskLevel::Standard),
                    required_checks: vec!["build".into(), "test".into()],
                    max_steps: 0,
                    priority_paths: vec![],
                },
                TaskTypePolicy {
                    task_pattern: "review".into(),
                    risk_level: Some(RiskLevel::Low),
                    required_checks: vec![],
                    max_steps: 20,
                    priority_paths: vec![],
                },
                TaskTypePolicy {
                    task_pattern: "security".into(),
                    risk_level: Some(RiskLevel::Critical),
                    required_checks: vec![
                        "build".into(),
                        "test".into(),
                        "lint".into(),
                        "typecheck".into(),
                    ],
                    max_steps: 0,
                    priority_paths: vec![".env".into(), "auth".into(), "crypto".into()],
                },
            ];
        }

        profile
    }

    /// Compute the effective verification tier for a set of changed files,
    /// taking the policy pack minimum into account.
    pub fn effective_tier(
        &self,
        changed_files: &[impl AsRef<std::path::Path>],
    ) -> crate::VerificationTier {
        let detected = crate::TierSelector::select(changed_files);
        let pack_min = self.policy_pack.minimum_tier();
        std::cmp::max(detected, pack_min)
    }

    /// Check whether a completion attempt should be blocked based on
    /// the current verification freshness and the active policy pack.
    pub fn should_block_completion(&self, freshness: crate::VerificationFreshness) -> bool {
        use crate::VerificationFreshness;
        match freshness {
            VerificationFreshness::Fresh => false,
            VerificationFreshness::Partial => !self.policy_pack.allows_stale_completion(),
            VerificationFreshness::Stale => !self.policy_pack.allows_stale_completion(),
            VerificationFreshness::None => true, // always block if nothing was verified
        }
    }

    /// Merge overrides from another profile (e.g. from oco.toml).
    /// Non-empty/non-default values in `other` take precedence.
    pub fn merge(&mut self, other: &RepoProfile) {
        if !other.stack.is_empty() && other.stack != "unknown" {
            self.stack = other.stack.clone();
        }
        if other.build_command.is_some() {
            self.build_command.clone_from(&other.build_command);
        }
        if other.test_command.is_some() {
            self.test_command.clone_from(&other.test_command);
        }
        if other.lint_command.is_some() {
            self.lint_command.clone_from(&other.lint_command);
        }
        if other.typecheck_command.is_some() {
            self.typecheck_command.clone_from(&other.typecheck_command);
        }
        if !other.sensitive_paths.is_empty() {
            self.sensitive_paths.extend(other.sensitive_paths.clone());
        }
        if !other.high_value_paths.is_empty() {
            self.high_value_paths = other.high_value_paths.clone();
        }
        if !other.exclude_patterns.is_empty() {
            self.exclude_patterns.extend(other.exclude_patterns.clone());
        }
        if other.risk_level != RiskLevel::Standard {
            self.risk_level = other.risk_level;
        }
        if !other.custom_verifications.is_empty() {
            self.custom_verifications
                .extend(other.custom_verifications.clone());
        }
        for (k, v) in &other.metadata {
            self.metadata.insert(k.clone(), v.clone());
        }
        if other.policy_pack != PolicyPack::Balanced {
            self.policy_pack = other.policy_pack;
        }
    }

    /// Find the task policy matching a user request (by keyword).
    /// Returns the first matching policy, or `None` if no pattern matches.
    pub fn matching_policy(&self, user_request: &str) -> Option<&TaskTypePolicy> {
        let lower = user_request.to_lowercase();
        self.task_policies
            .iter()
            .find(|p| lower.contains(&p.task_pattern))
    }

    /// Check if a path matches any sensitive path pattern.
    pub fn is_sensitive(&self, path: &str) -> bool {
        self.sensitive_paths.iter().any(|pattern| {
            if pattern.contains('*') {
                // Simple glob: *.pem matches foo.pem
                let suffix = pattern.trim_start_matches('*');
                path.ends_with(suffix)
            } else {
                path == pattern || path.ends_with(pattern)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_unknown_for_empty_dir() {
        let dir = std::env::temp_dir().join("oco_test_empty_detect");
        let _ = std::fs::create_dir_all(&dir);
        let profile = RepoProfile::detect(&dir);
        assert_eq!(profile.stack, "unknown");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_overrides_non_default() {
        let mut base = RepoProfile {
            stack: "rust".into(),
            build_command: Some("cargo build".into()),
            ..Default::default()
        };

        let override_profile = RepoProfile {
            test_command: Some("cargo nextest run".into()),
            risk_level: RiskLevel::High,
            ..Default::default()
        };

        base.merge(&override_profile);
        assert_eq!(base.stack, "rust"); // not overridden (other is "unknown")
        assert_eq!(base.test_command.as_deref(), Some("cargo nextest run"));
        assert_eq!(base.risk_level, RiskLevel::High);
    }

    #[test]
    fn matching_policy_finds_by_keyword() {
        let profile = RepoProfile {
            task_policies: vec![
                TaskTypePolicy {
                    task_pattern: "debug".into(),
                    risk_level: Some(RiskLevel::Standard),
                    required_checks: vec!["test".into()],
                    max_steps: 0,
                    priority_paths: vec![],
                },
                TaskTypePolicy {
                    task_pattern: "refactor".into(),
                    risk_level: Some(RiskLevel::High),
                    required_checks: vec!["build".into(), "test".into()],
                    max_steps: 0,
                    priority_paths: vec![],
                },
            ],
            ..Default::default()
        };
        let policy = profile.matching_policy("Please refactor the auth module");
        assert!(policy.is_some());
        assert_eq!(policy.unwrap().risk_level, Some(RiskLevel::High));
        assert!(profile.matching_policy("explain this code").is_none());
    }

    #[test]
    fn is_sensitive_matches_patterns() {
        let profile = RepoProfile {
            sensitive_paths: vec![".env".into(), "*.pem".into()],
            ..Default::default()
        };
        assert!(profile.is_sensitive(".env"));
        assert!(profile.is_sensitive("certs/server.pem"));
        assert!(!profile.is_sensitive("src/main.rs"));
    }

    // ── Policy pack tests ──

    #[test]
    fn policy_pack_default_is_balanced() {
        assert_eq!(PolicyPack::default(), PolicyPack::Balanced);
    }

    #[test]
    fn policy_pack_minimum_tier() {
        use crate::VerificationTier;
        assert_eq!(PolicyPack::Fast.minimum_tier(), VerificationTier::Light);
        assert_eq!(
            PolicyPack::Balanced.minimum_tier(),
            VerificationTier::Standard
        );
        assert_eq!(
            PolicyPack::Strict.minimum_tier(),
            VerificationTier::Thorough
        );
    }

    #[test]
    fn policy_pack_mandatory_strategies_count() {
        assert_eq!(PolicyPack::Fast.mandatory_strategies().len(), 1);
        assert_eq!(PolicyPack::Balanced.mandatory_strategies().len(), 2);
        assert_eq!(PolicyPack::Strict.mandatory_strategies().len(), 4);
    }

    #[test]
    fn policy_pack_stale_completion() {
        assert!(PolicyPack::Fast.allows_stale_completion());
        assert!(!PolicyPack::Balanced.allows_stale_completion());
        assert!(!PolicyPack::Strict.allows_stale_completion());
    }

    #[test]
    fn effective_tier_respects_pack_minimum() {
        use crate::VerificationTier;
        let profile = RepoProfile {
            policy_pack: PolicyPack::Strict,
            ..Default::default()
        };
        // Even docs-only changes get Thorough under strict pack.
        let files = vec!["README.md"];
        assert_eq!(profile.effective_tier(&files), VerificationTier::Thorough);
    }

    #[test]
    fn effective_tier_detected_can_exceed_pack() {
        use crate::VerificationTier;
        let profile = RepoProfile {
            policy_pack: PolicyPack::Fast,
            ..Default::default()
        };
        // Security files still get Thorough even under fast pack.
        let files = vec!["src/auth/middleware.rs"];
        assert_eq!(profile.effective_tier(&files), VerificationTier::Thorough);
    }

    #[test]
    fn should_block_completion_fresh_never_blocks() {
        use crate::VerificationFreshness;
        let profile = RepoProfile {
            policy_pack: PolicyPack::Strict,
            ..Default::default()
        };
        assert!(!profile.should_block_completion(VerificationFreshness::Fresh));
    }

    #[test]
    fn should_block_completion_stale_blocked_by_balanced() {
        use crate::VerificationFreshness;
        let profile = RepoProfile {
            policy_pack: PolicyPack::Balanced,
            ..Default::default()
        };
        assert!(profile.should_block_completion(VerificationFreshness::Stale));
    }

    #[test]
    fn should_block_completion_stale_allowed_by_fast() {
        use crate::VerificationFreshness;
        let profile = RepoProfile {
            policy_pack: PolicyPack::Fast,
            ..Default::default()
        };
        assert!(!profile.should_block_completion(VerificationFreshness::Stale));
    }

    #[test]
    fn should_block_completion_none_always_blocks() {
        use crate::VerificationFreshness;
        for pack in [PolicyPack::Fast, PolicyPack::Balanced, PolicyPack::Strict] {
            let profile = RepoProfile {
                policy_pack: pack,
                ..Default::default()
            };
            assert!(profile.should_block_completion(VerificationFreshness::None));
        }
    }

    #[test]
    fn merge_overrides_policy_pack() {
        let mut base = RepoProfile::default();
        assert_eq!(base.policy_pack, PolicyPack::Balanced);

        let strict = RepoProfile {
            policy_pack: PolicyPack::Strict,
            ..Default::default()
        };
        base.merge(&strict);
        assert_eq!(base.policy_pack, PolicyPack::Strict);
    }

    #[test]
    fn policy_pack_serde_round_trip() {
        for pack in [PolicyPack::Fast, PolicyPack::Balanced, PolicyPack::Strict] {
            let json = serde_json::to_string(&pack).unwrap();
            let parsed: PolicyPack = serde_json::from_str(&json).unwrap();
            assert_eq!(pack, parsed);
        }
    }

    #[test]
    fn policy_pack_labels() {
        assert_eq!(PolicyPack::Fast.label(), "fast");
        assert_eq!(PolicyPack::Balanced.label(), "balanced");
        assert_eq!(PolicyPack::Strict.label(), "strict");
    }
}
