use oco_shared_types::{ToolDescriptor, ToolGateDecision};
use serde::{Deserialize, Serialize};

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
                        reason: format!(
                            "tool '{}' is marked as requiring confirmation",
                            tool.name
                        ),
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

    /// Evaluate a raw command string against destructive patterns.
    /// Useful for shell/exec tool calls where the actual command is in the arguments.
    pub fn evaluate_command(&self, command: &str) -> ToolGateDecision {
        match self.policy {
            WritePolicy::AllowAll => ToolGateDecision::Allow,

            WritePolicy::RequireConfirmation => {
                if Self::command_is_destructive(command) {
                    ToolGateDecision::RequireConfirmation {
                        reason: format!(
                            "command contains destructive pattern: '{}'",
                            Self::matching_destructive_pattern(command)
                                .unwrap_or("unknown")
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
                            Self::matching_destructive_pattern(command)
                                .unwrap_or("unknown")
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
        if DESTRUCTIVE_TOOLS
            .iter()
            .any(|dt| name_lower.contains(dt))
        {
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
        assert!(matches!(gate.evaluate(&read_tool()), ToolGateDecision::Allow));
        assert!(matches!(gate.evaluate(&write_tool()), ToolGateDecision::Allow));
        assert!(matches!(
            gate.evaluate(&destructive_tool()),
            ToolGateDecision::Allow
        ));
    }

    #[test]
    fn require_confirmation_gates_writes() {
        let gate = PolicyGate::new(WritePolicy::RequireConfirmation);
        assert!(matches!(gate.evaluate(&read_tool()), ToolGateDecision::Allow));
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
        assert!(matches!(gate.evaluate(&read_tool()), ToolGateDecision::Allow));
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
}
