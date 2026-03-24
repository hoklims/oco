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
                    required_checks: vec!["build".into(), "test".into(), "lint".into(), "typecheck".into()],
                    max_steps: 0,
                    priority_paths: vec![".env".into(), "auth".into(), "crypto".into()],
                },
            ];
        }

        profile
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
}
