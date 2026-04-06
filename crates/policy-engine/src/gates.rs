use oco_shared_types::{
    PolicyPack, RepoProfile, ToolDescriptor, ToolGateDecision, VerificationFreshness,
};
use serde::{Deserialize, Serialize};

use crate::secret_scanner;

/// Write policy levels controlling how destructive actions are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WritePolicy {
    /// All write operations are allowed without confirmation.
    AllowAll,
    /// Write operations require explicit confirmation before execution.
    #[default]
    RequireConfirmation,
    /// Destructive operations are denied entirely; non-destructive writes may proceed.
    DenyDestructive,
}

/// Known destructive command patterns.
const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -r",
    "rmdir",
    "delete",
    "drop",
    "truncate",
    "format",
    "destroy",
    "purge",
    "reset --hard",
    "force push",
    "push --force",
    "push -f",
    "clean -fd",
    "checkout -- .",
    "restore .",
];

/// Known destructive tool names.
const DESTRUCTIVE_TOOLS: &[&str] = &[
    "file_delete",
    "directory_delete",
    "git_reset",
    "git_force_push",
    "database_drop",
    "database_truncate",
];

/// Policy gate for write and destructive actions.
///
/// Evaluates tool descriptors against the current write policy to decide
/// whether execution should proceed, require confirmation, or be denied.
pub struct PolicyGate {
    policy: WritePolicy,
}

impl PolicyGate {
    pub fn new(policy: WritePolicy) -> Self {
        Self { policy }
    }

    /// Evaluate whether a tool call should be allowed under the current policy.
    pub fn evaluate(&self, tool: &ToolDescriptor) -> ToolGateDecision {
        match self.policy {
            WritePolicy::AllowAll => ToolGateDecision::Allow,

            WritePolicy::RequireConfirmation => {
                if tool.requires_confirmation {
                    return ToolGateDecision::RequireConfirmation {
                        reason: format!("tool '{}' is marked as requiring confirmation", tool.name),
                    };
                }
                if tool.is_write {
                    return ToolGateDecision::RequireConfirmation {
                        reason: format!("tool '{}' performs write operations", tool.name),
                    };
                }
                ToolGateDecision::Allow
            }

            WritePolicy::DenyDestructive => {
                if Self::is_destructive_tool(tool) {
                    return ToolGateDecision::Deny {
                        reason: format!(
                            "tool '{}' is classified as destructive under DenyDestructive policy",
                            tool.name
                        ),
                    };
                }
                if tool.is_write {
                    return ToolGateDecision::RequireConfirmation {
                        reason: format!(
                            "tool '{}' performs write operations (non-destructive allowed with confirmation)",
                            tool.name
                        ),
                    };
                }
                ToolGateDecision::Allow
            }
        }
    }

    /// Evaluate a raw command string against destructive patterns and secret scanning.
    /// Useful for shell/exec tool calls where the actual command is in the arguments.
    pub fn evaluate_command(&self, command: &str) -> ToolGateDecision {
        // Always scan for secrets, regardless of policy level.
        let scan = secret_scanner::scan_secrets(command);
        if scan.has_secrets {
            let secret_names: Vec<&str> = scan
                .matches
                .iter()
                .map(|m| m.pattern_name.as_str())
                .collect();
            return ToolGateDecision::Deny {
                reason: format!(
                    "command contains embedded secrets: {}. Use environment variables instead",
                    secret_names.join(", ")
                ),
            };
        }

        match self.policy {
            WritePolicy::AllowAll => ToolGateDecision::Allow,

            WritePolicy::RequireConfirmation => {
                if Self::command_is_destructive(command) {
                    ToolGateDecision::RequireConfirmation {
                        reason: format!(
                            "command contains destructive pattern: '{}'",
                            Self::matching_destructive_pattern(command).unwrap_or("unknown")
                        ),
                    }
                } else {
                    ToolGateDecision::Allow
                }
            }

            WritePolicy::DenyDestructive => {
                if Self::command_is_destructive(command) {
                    ToolGateDecision::Deny {
                        reason: format!(
                            "command contains destructive pattern: '{}' (denied by policy)",
                            Self::matching_destructive_pattern(command).unwrap_or("unknown")
                        ),
                    }
                } else {
                    ToolGateDecision::Allow
                }
            }
        }
    }

    /// Get the current policy level.
    pub fn policy(&self) -> WritePolicy {
        self.policy
    }

    /// Change the policy level.
    pub fn set_policy(&mut self, policy: WritePolicy) {
        self.policy = policy;
    }

    /// Check if a tool descriptor indicates a destructive operation.
    fn is_destructive_tool(tool: &ToolDescriptor) -> bool {
        let name_lower = tool.name.to_lowercase();

        // Check against known destructive tool names
        if DESTRUCTIVE_TOOLS.iter().any(|dt| name_lower.contains(dt)) {
            return true;
        }

        // Check tags for destructive indicators
        if tool
            .tags
            .iter()
            .any(|tag| tag == "destructive" || tag == "dangerous" || tag == "irreversible")
        {
            return true;
        }

        // Check description for destructive keywords
        let desc_lower = tool.description.to_lowercase();
        desc_lower.contains("delete")
            || desc_lower.contains("destroy")
            || desc_lower.contains("remove permanently")
            || desc_lower.contains("irreversible")
            || desc_lower.contains("force")
    }

    /// Check if a command string contains destructive patterns.
    fn command_is_destructive(command: &str) -> bool {
        let cmd_lower = command.to_lowercase();
        DESTRUCTIVE_PATTERNS
            .iter()
            .any(|pattern| cmd_lower.contains(pattern))
    }

    /// Find which destructive pattern matches.
    fn matching_destructive_pattern(command: &str) -> Option<&'static str> {
        let cmd_lower = command.to_lowercase();
        DESTRUCTIVE_PATTERNS
            .iter()
            .find(|pattern| cmd_lower.contains(**pattern))
            .copied()
    }
}

/// Decision from a policy-pack gate check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum GateDecision {
    /// Completion is allowed.
    Allow,
    /// Completion is blocked, with an explanation.
    Block { reason: String },
}

/// Gate that evaluates whether task completion is allowed under the active
/// [`PolicyPack`].
///
/// Checks performed:
/// 1. Verification freshness vs pack policy (stale/none blocks unless Fast).
/// 2. Sensitive-path modifications under Strict pack.
pub struct PolicyPackGate;

impl PolicyPackGate {
    /// Evaluate whether completion should be allowed.
    ///
    /// - `profile`: the repo profile containing the active `PolicyPack` and
    ///   sensitive path definitions.
    /// - `freshness`: current verification freshness.
    /// - `changed_files`: files modified during this session (used for
    ///   sensitive-path checks under Strict).
    pub fn evaluate(
        profile: &RepoProfile,
        freshness: VerificationFreshness,
        changed_files: &[&str],
    ) -> GateDecision {
        // 1. Freshness check via profile helper.
        if profile.should_block_completion(freshness) {
            return GateDecision::Block {
                reason: format!(
                    "verification is {freshness:?} and policy pack {:?} does not allow stale completion",
                    profile.policy_pack
                ),
            };
        }

        // 2. Under Strict, block if any changed file is sensitive.
        if profile.policy_pack == PolicyPack::Strict {
            let sensitive_files: Vec<&str> = changed_files
                .iter()
                .filter(|f| profile.is_sensitive(f))
                .copied()
                .collect();
            if !sensitive_files.is_empty() {
                return GateDecision::Block {
                    reason: format!(
                        "strict policy pack blocks completion when sensitive files are modified: {}",
                        sensitive_files.join(", ")
                    ),
                };
            }
        }

        GateDecision::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_tool() -> ToolDescriptor {
        ToolDescriptor {
            name: "file_read".to_string(),
            description: "Read a file from disk".to_string(),
            input_schema: serde_json::json!({}),
            is_write: false,
            requires_confirmation: false,
            timeout_secs: 10,
            tags: vec!["read".to_string()],
        }
    }

    fn write_tool() -> ToolDescriptor {
        ToolDescriptor {
            name: "file_write".to_string(),
            description: "Write content to a file".to_string(),
            input_schema: serde_json::json!({}),
            is_write: true,
            requires_confirmation: false,
            timeout_secs: 10,
            tags: vec!["write".to_string()],
        }
    }

    fn destructive_tool() -> ToolDescriptor {
        ToolDescriptor {
            name: "file_delete".to_string(),
            description: "Delete a file permanently".to_string(),
            input_schema: serde_json::json!({}),
            is_write: true,
            requires_confirmation: true,
            timeout_secs: 10,
            tags: vec!["destructive".to_string()],
        }
    }

    #[test]
    fn allow_all_allows_everything() {
        let gate = PolicyGate::new(WritePolicy::AllowAll);
        assert!(matches!(
            gate.evaluate(&read_tool()),
            ToolGateDecision::Allow
        ));
        assert!(matches!(
            gate.evaluate(&write_tool()),
            ToolGateDecision::Allow
        ));
        assert!(matches!(
            gate.evaluate(&destructive_tool()),
            ToolGateDecision::Allow
        ));
    }

    #[test]
    fn require_confirmation_gates_writes() {
        let gate = PolicyGate::new(WritePolicy::RequireConfirmation);
        assert!(matches!(
            gate.evaluate(&read_tool()),
            ToolGateDecision::Allow
        ));
        assert!(matches!(
            gate.evaluate(&write_tool()),
            ToolGateDecision::RequireConfirmation { .. }
        ));
        assert!(matches!(
            gate.evaluate(&destructive_tool()),
            ToolGateDecision::RequireConfirmation { .. }
        ));
    }

    #[test]
    fn deny_destructive_blocks_destructive() {
        let gate = PolicyGate::new(WritePolicy::DenyDestructive);
        assert!(matches!(
            gate.evaluate(&read_tool()),
            ToolGateDecision::Allow
        ));
        assert!(matches!(
            gate.evaluate(&write_tool()),
            ToolGateDecision::RequireConfirmation { .. }
        ));
        assert!(matches!(
            gate.evaluate(&destructive_tool()),
            ToolGateDecision::Deny { .. }
        ));
    }

    #[test]
    fn command_evaluation() {
        let gate = PolicyGate::new(WritePolicy::DenyDestructive);
        assert!(matches!(
            gate.evaluate_command("rm -rf /tmp/stuff"),
            ToolGateDecision::Deny { .. }
        ));
        assert!(matches!(
            gate.evaluate_command("git push --force origin main"),
            ToolGateDecision::Deny { .. }
        ));
        assert!(matches!(
            gate.evaluate_command("ls -la"),
            ToolGateDecision::Allow
        ));
        assert!(matches!(
            gate.evaluate_command("cargo build"),
            ToolGateDecision::Allow
        ));
    }

    #[test]
    fn command_confirmation_mode() {
        let gate = PolicyGate::new(WritePolicy::RequireConfirmation);
        assert!(matches!(
            gate.evaluate_command("rm -rf /tmp"),
            ToolGateDecision::RequireConfirmation { .. }
        ));
        assert!(matches!(
            gate.evaluate_command("echo hello"),
            ToolGateDecision::Allow
        ));
    }

    #[test]
    fn secret_scanning_blocks_api_keys() {
        // Secret scanning applies regardless of policy level
        let gate = PolicyGate::new(WritePolicy::AllowAll);
        let result = gate.evaluate_command("curl -H 'Authorization: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwx' https://api.example.com");
        assert!(matches!(result, ToolGateDecision::Deny { .. }));
        if let ToolGateDecision::Deny { reason } = result {
            assert!(reason.contains("secrets"));
        }
    }

    #[test]
    fn secret_scanning_blocks_connection_strings() {
        let gate = PolicyGate::new(WritePolicy::AllowAll);
        let result =
            gate.evaluate_command("psql postgres://admin:password123@prod.db.example.com/mydb");
        assert!(matches!(result, ToolGateDecision::Deny { .. }));
    }

    #[test]
    fn clean_commands_pass_secret_scan() {
        let gate = PolicyGate::new(WritePolicy::AllowAll);
        assert!(matches!(
            gate.evaluate_command("cargo test --release"),
            ToolGateDecision::Allow
        ));
        assert!(matches!(
            gate.evaluate_command("git log --oneline -10"),
            ToolGateDecision::Allow
        ));
    }

    // --- PolicyPackGate tests ---

    fn balanced_profile() -> RepoProfile {
        RepoProfile {
            policy_pack: PolicyPack::Balanced,
            sensitive_paths: vec![".env".into(), "*.pem".into()],
            ..Default::default()
        }
    }

    #[test]
    fn policy_pack_gate_allows_fresh() {
        let profile = balanced_profile();
        assert_eq!(
            PolicyPackGate::evaluate(&profile, VerificationFreshness::Fresh, &[]),
            GateDecision::Allow,
        );
    }

    #[test]
    fn policy_pack_gate_blocks_stale_balanced() {
        let profile = balanced_profile();
        let decision =
            PolicyPackGate::evaluate(&profile, VerificationFreshness::Stale, &["src/lib.rs"]);
        assert!(matches!(decision, GateDecision::Block { .. }));
    }

    #[test]
    fn policy_pack_gate_blocks_none_balanced() {
        let profile = balanced_profile();
        let decision =
            PolicyPackGate::evaluate(&profile, VerificationFreshness::None, &["src/lib.rs"]);
        assert!(matches!(decision, GateDecision::Block { .. }));
    }

    #[test]
    fn policy_pack_gate_allows_stale_fast() {
        let profile = RepoProfile {
            policy_pack: PolicyPack::Fast,
            ..Default::default()
        };
        assert_eq!(
            PolicyPackGate::evaluate(&profile, VerificationFreshness::Stale, &[]),
            GateDecision::Allow,
        );
    }

    #[test]
    fn policy_pack_gate_blocks_none_even_fast() {
        // Even Fast pack blocks when NO verification has been done at all.
        let profile = RepoProfile {
            policy_pack: PolicyPack::Fast,
            ..Default::default()
        };
        assert_eq!(
            PolicyPackGate::evaluate(&profile, VerificationFreshness::None, &[]),
            GateDecision::Block {
                reason: "verification is None and policy pack Fast does not allow stale completion"
                    .into(),
            },
        );
    }

    #[test]
    fn policy_pack_gate_strict_blocks_sensitive_files() {
        let profile = RepoProfile {
            policy_pack: PolicyPack::Strict,
            sensitive_paths: vec![".env".into(), "*.pem".into()],
            ..Default::default()
        };
        let decision = PolicyPackGate::evaluate(
            &profile,
            VerificationFreshness::Fresh,
            &["src/main.rs", ".env"],
        );
        match decision {
            GateDecision::Block { reason } => {
                assert!(reason.contains(".env"));
                assert!(reason.contains("sensitive"));
            }
            GateDecision::Allow => panic!("expected Block for sensitive files under Strict"),
        }
    }

    #[test]
    fn policy_pack_gate_strict_allows_non_sensitive() {
        let profile = RepoProfile {
            policy_pack: PolicyPack::Strict,
            sensitive_paths: vec![".env".into()],
            ..Default::default()
        };
        assert_eq!(
            PolicyPackGate::evaluate(
                &profile,
                VerificationFreshness::Fresh,
                &["src/main.rs", "src/lib.rs"]
            ),
            GateDecision::Allow,
        );
    }

    #[test]
    fn policy_pack_gate_balanced_ignores_sensitive_files() {
        // Balanced pack should not block on sensitive file changes
        // (only Strict does).
        let profile = balanced_profile();
        assert_eq!(
            PolicyPackGate::evaluate(
                &profile,
                VerificationFreshness::Fresh,
                &[".env", "server.pem"]
            ),
            GateDecision::Allow,
        );
    }

    #[test]
    fn policy_pack_gate_partial_freshness_blocked_by_balanced() {
        // Balanced blocks on Partial — only Fast allows stale/partial.
        let profile = balanced_profile();
        assert!(matches!(
            PolicyPackGate::evaluate(&profile, VerificationFreshness::Partial, &[]),
            GateDecision::Block { .. },
        ));
    }

    #[test]
    fn policy_pack_gate_partial_freshness_allowed_by_fast() {
        let profile = RepoProfile {
            policy_pack: PolicyPack::Fast,
            ..Default::default()
        };
        assert_eq!(
            PolicyPackGate::evaluate(&profile, VerificationFreshness::Partial, &[]),
            GateDecision::Allow,
        );
    }
}
