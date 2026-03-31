//! Claude Code hook event types and mapping to OrchestrationEvent.
//!
//! Covers all 24 documented Claude Code hook events (v2.1.81+).
//! Each variant can be deserialized from the JSON POST body of an HTTP hook.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use oco_shared_types::telemetry::{BudgetSnapshot, OrchestrationEvent};
use oco_shared_types::{OrchestratorAction, StopReason};

/// A zeroed budget snapshot (used when mapping events without budget context).
fn zero_budget() -> BudgetSnapshot {
    BudgetSnapshot {
        tokens_used: 0,
        tokens_remaining: 0,
        tool_calls_used: 0,
        tool_calls_remaining: 0,
        retrievals_used: 0,
        verify_cycles_used: 0,
        elapsed_secs: 0,
    }
}

// ---------------------------------------------------------------------------
// ClaudeHookEvent — all 24 Claude Code hook events
// ---------------------------------------------------------------------------

/// All 24 Claude Code hook events (documented as of v2.1.81+).
///
/// Deserialized from the JSON POST body via serde tag on the `event` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "PascalCase")]
pub enum ClaudeHookEvent {
    /// A new Claude Code session started.
    SessionStart {
        #[serde(default)]
        reason: String,
    },

    /// The user submitted a prompt.
    UserPromptSubmit {
        #[serde(default)]
        prompt: String,
    },

    /// About to execute a tool (blocking — can be denied).
    PreToolUse {
        tool_name: String,
        #[serde(default)]
        input: serde_json::Value,
    },

    /// A tool was successfully executed.
    PostToolUse {
        tool_name: String,
        #[serde(default)]
        output: serde_json::Value,
        #[serde(default)]
        success: bool,
        #[serde(default)]
        duration_ms: u64,
    },

    /// A tool execution failed.
    PostToolUseFailure {
        tool_name: String,
        #[serde(default)]
        error: String,
    },

    /// Claude Code is about to stop.
    Stop {
        #[serde(default)]
        reason: String,
    },

    /// Claude Code stop sequence failed.
    StopFailure {
        #[serde(default)]
        error_type: String,
    },

    /// A file in the workspace changed.
    FileChanged {
        #[serde(default)]
        path: String,
        #[serde(default)]
        change_type: String,
    },

    /// Context was compacted (summarized to free tokens).
    PostCompact {
        #[serde(default)]
        compact_summary: String,
    },

    /// A task (subagent or teammate) completed.
    TaskCompleted {
        #[serde(default)]
        task_id: String,
        #[serde(default)]
        success: bool,
        #[serde(default)]
        output: String,
    },

    /// A teammate is idle and waiting for work.
    TeammateIdle {
        #[serde(default)]
        teammate_name: String,
    },

    /// A subagent was spawned.
    SubagentStart {
        #[serde(default)]
        agent_name: String,
    },

    /// A subagent finished execution.
    SubagentStop {
        #[serde(default)]
        agent_name: String,
        #[serde(default)]
        success: bool,
    },

    /// The session is ending (final cleanup).
    SessionEnd {},

    /// A task was created (TaskCreate tool).
    TaskCreated {
        #[serde(default)]
        task_id: String,
        #[serde(default)]
        description: String,
    },

    /// CLAUDE.md / instructions were loaded.
    InstructionsLoaded {
        #[serde(default)]
        source: String,
    },

    /// A Claude Code configuration value changed.
    ConfigChange {
        #[serde(default)]
        key: String,
        #[serde(default)]
        value: serde_json::Value,
    },

    /// The working directory changed.
    CwdChanged {
        #[serde(default)]
        new_cwd: String,
    },

    /// A git worktree was created (for agent isolation).
    WorktreeCreate {
        #[serde(default)]
        path: String,
    },

    /// A git worktree was removed.
    WorktreeRemove {
        #[serde(default)]
        path: String,
    },

    /// About to compact context (pre-compaction hook).
    PreCompact {},

    /// A tool requested elevated permissions.
    PermissionRequest {
        #[serde(default)]
        tool_name: String,
        #[serde(default)]
        description: String,
    },

    /// An MCP server requested user input via elicitation.
    Elicitation {
        #[serde(default)]
        server: String,
        #[serde(default)]
        request: serde_json::Value,
    },

    /// The user responded to an elicitation dialog.
    ElicitationResult {
        #[serde(default)]
        server: String,
        #[serde(default)]
        response: serde_json::Value,
    },
}

impl ClaudeHookEvent {
    /// Map this Claude Code event to an OCO `OrchestrationEvent`.
    ///
    /// Returns `None` for events that trigger side effects only (re-index,
    /// memory update, etc.) and do not map to an orchestration lifecycle event.
    pub fn to_orchestration_event(&self) -> Option<OrchestrationEvent> {
        match self {
            ClaudeHookEvent::PostToolUse {
                tool_name,
                success,
                duration_ms,
                ..
            } => Some(OrchestrationEvent::StepCompleted {
                step: 0,
                action: OrchestratorAction::ToolCall {
                    tool_name: tool_name.clone(),
                    arguments: serde_json::Value::Null,
                },
                reason: format!("tool:{tool_name}"),
                duration_ms: *duration_ms,
                budget_snapshot: zero_budget(),
                knowledge_confidence: 0.0,
                success: *success,
            }),

            ClaudeHookEvent::PostToolUseFailure {
                tool_name, error, ..
            } => Some(OrchestrationEvent::StepCompleted {
                step: 0,
                action: OrchestratorAction::ToolCall {
                    tool_name: tool_name.clone(),
                    arguments: serde_json::Value::Null,
                },
                reason: format!("tool_failure:{tool_name}: {error}"),
                duration_ms: 0,
                budget_snapshot: zero_budget(),
                knowledge_confidence: 0.0,
                success: false,
            }),

            ClaudeHookEvent::Stop { .. } => Some(OrchestrationEvent::Stopped {
                reason: StopReason::TaskComplete,
                total_steps: 0,
                total_tokens: 0,
            }),

            ClaudeHookEvent::TaskCompleted {
                task_id, success, ..
            } => Some(OrchestrationEvent::PlanStepCompleted {
                step_id: Uuid::nil(),
                step_name: task_id.clone(),
                success: *success,
                duration_ms: 0,
                tokens_used: 0,
            }),

            ClaudeHookEvent::SubagentStart { agent_name } => {
                Some(OrchestrationEvent::PlanStepStarted {
                    step_id: Uuid::nil(),
                    step_name: agent_name.clone(),
                    role: "subagent".to_string(),
                    execution_mode: "subagent".to_string(),
                })
            }

            ClaudeHookEvent::SubagentStop {
                agent_name,
                success,
            } => Some(OrchestrationEvent::PlanStepCompleted {
                step_id: Uuid::nil(),
                step_name: agent_name.clone(),
                success: *success,
                duration_ms: 0,
                tokens_used: 0,
            }),

            ClaudeHookEvent::SessionEnd {} => Some(OrchestrationEvent::Stopped {
                reason: StopReason::TaskComplete,
                total_steps: 0,
                total_tokens: 0,
            }),

            // All other events are side-effect only — no orchestration event.
            ClaudeHookEvent::SessionStart { .. }
            | ClaudeHookEvent::UserPromptSubmit { .. }
            | ClaudeHookEvent::PreToolUse { .. }
            | ClaudeHookEvent::StopFailure { .. }
            | ClaudeHookEvent::FileChanged { .. }
            | ClaudeHookEvent::PostCompact { .. }
            | ClaudeHookEvent::TeammateIdle { .. }
            | ClaudeHookEvent::TaskCreated { .. }
            | ClaudeHookEvent::InstructionsLoaded { .. }
            | ClaudeHookEvent::ConfigChange { .. }
            | ClaudeHookEvent::CwdChanged { .. }
            | ClaudeHookEvent::WorktreeCreate { .. }
            | ClaudeHookEvent::WorktreeRemove { .. }
            | ClaudeHookEvent::PreCompact {}
            | ClaudeHookEvent::PermissionRequest { .. }
            | ClaudeHookEvent::Elicitation { .. }
            | ClaudeHookEvent::ElicitationResult { .. } => None,
        }
    }

    /// Returns the event name as a static string.
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::SessionStart { .. } => "SessionStart",
            Self::UserPromptSubmit { .. } => "UserPromptSubmit",
            Self::PreToolUse { .. } => "PreToolUse",
            Self::PostToolUse { .. } => "PostToolUse",
            Self::PostToolUseFailure { .. } => "PostToolUseFailure",
            Self::Stop { .. } => "Stop",
            Self::StopFailure { .. } => "StopFailure",
            Self::FileChanged { .. } => "FileChanged",
            Self::PostCompact { .. } => "PostCompact",
            Self::TaskCompleted { .. } => "TaskCompleted",
            Self::TeammateIdle { .. } => "TeammateIdle",
            Self::SubagentStart { .. } => "SubagentStart",
            Self::SubagentStop { .. } => "SubagentStop",
            Self::SessionEnd {} => "SessionEnd",
            Self::TaskCreated { .. } => "TaskCreated",
            Self::InstructionsLoaded { .. } => "InstructionsLoaded",
            Self::ConfigChange { .. } => "ConfigChange",
            Self::CwdChanged { .. } => "CwdChanged",
            Self::WorktreeCreate { .. } => "WorktreeCreate",
            Self::WorktreeRemove { .. } => "WorktreeRemove",
            Self::PreCompact {} => "PreCompact",
            Self::PermissionRequest { .. } => "PermissionRequest",
            Self::Elicitation { .. } => "Elicitation",
            Self::ElicitationResult { .. } => "ElicitationResult",
        }
    }
}

// ---------------------------------------------------------------------------
// HookDecision — response from OCO back to Claude Code
// ---------------------------------------------------------------------------

/// Response sent back to Claude Code from a blocking hook.
///
/// An empty/default decision means "continue normally".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookDecision {
    /// Whether to continue the action. Defaults to `true`.
    #[serde(rename = "continue", default = "default_true")]
    pub should_continue: bool,

    /// If continuing is denied, the reason why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    /// Hook-specific output to pass back (e.g., injected context).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

impl HookDecision {
    /// Allow the action to proceed.
    pub fn allow() -> Self {
        Self {
            should_continue: true,
            stop_reason: None,
            hook_specific_output: None,
        }
    }

    /// Deny the action with a reason.
    pub fn deny(reason: &str) -> Self {
        Self {
            should_continue: false,
            stop_reason: Some(reason.to_string()),
            hook_specific_output: None,
        }
    }

    /// Attach hook-specific output (e.g., injected context after compact).
    pub fn with_output(mut self, key: &str, value: serde_json::Value) -> Self {
        let map = self
            .hook_specific_output
            .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let serde_json::Value::Object(m) = map {
            m.insert(key.to_string(), value);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Deserialization tests for all 24 event types
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_session_start() {
        let json = r#"{"event": "SessionStart", "reason": "user_initiated"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::SessionStart { reason } if reason == "user_initiated")
        );
    }

    #[test]
    fn deserialize_user_prompt_submit() {
        let json = r#"{"event": "UserPromptSubmit", "prompt": "fix the bug"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::UserPromptSubmit { prompt } if prompt == "fix the bug")
        );
    }

    #[test]
    fn deserialize_pre_tool_use() {
        let json = r#"{"event": "PreToolUse", "tool_name": "Bash", "input": {"command": "ls"}}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::PreToolUse { tool_name, .. } if tool_name == "Bash")
        );
    }

    #[test]
    fn deserialize_post_tool_use() {
        let json = r#"{"event": "PostToolUse", "tool_name": "Read", "output": "file contents", "success": true, "duration_ms": 42}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::PostToolUse { tool_name, success, duration_ms, .. } if tool_name == "Read" && success && duration_ms == 42)
        );
    }

    #[test]
    fn deserialize_post_tool_use_failure() {
        let json =
            r#"{"event": "PostToolUseFailure", "tool_name": "Bash", "error": "command not found"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::PostToolUseFailure { tool_name, error } if tool_name == "Bash" && error == "command not found")
        );
    }

    #[test]
    fn deserialize_stop() {
        let json = r#"{"event": "Stop", "reason": "task_complete"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ClaudeHookEvent::Stop { reason } if reason == "task_complete"));
    }

    #[test]
    fn deserialize_stop_failure() {
        let json = r#"{"event": "StopFailure", "error_type": "timeout"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::StopFailure { error_type } if error_type == "timeout")
        );
    }

    #[test]
    fn deserialize_file_changed() {
        let json = r#"{"event": "FileChanged", "path": "src/main.rs", "change_type": "modified"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::FileChanged { path, change_type } if path == "src/main.rs" && change_type == "modified")
        );
    }

    #[test]
    fn deserialize_post_compact() {
        let json = r#"{"event": "PostCompact", "compact_summary": "Summarized 50k tokens to 8k"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::PostCompact { compact_summary } if compact_summary.contains("50k"))
        );
    }

    #[test]
    fn deserialize_task_completed() {
        let json = r#"{"event": "TaskCompleted", "task_id": "abc-123", "success": true, "output": "done"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::TaskCompleted { task_id, success, .. } if task_id == "abc-123" && success)
        );
    }

    #[test]
    fn deserialize_teammate_idle() {
        let json = r#"{"event": "TeammateIdle", "teammate_name": "reviewer"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::TeammateIdle { teammate_name } if teammate_name == "reviewer")
        );
    }

    #[test]
    fn deserialize_subagent_start() {
        let json = r#"{"event": "SubagentStart", "agent_name": "debugger"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::SubagentStart { agent_name } if agent_name == "debugger")
        );
    }

    #[test]
    fn deserialize_subagent_stop() {
        let json = r#"{"event": "SubagentStop", "agent_name": "debugger", "success": true}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::SubagentStop { agent_name, success } if agent_name == "debugger" && success)
        );
    }

    #[test]
    fn deserialize_session_end() {
        let json = r#"{"event": "SessionEnd"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ClaudeHookEvent::SessionEnd {}));
    }

    #[test]
    fn deserialize_task_created() {
        let json = r#"{"event": "TaskCreated", "task_id": "t-1", "description": "Fix login"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::TaskCreated { task_id, description } if task_id == "t-1" && description == "Fix login")
        );
    }

    #[test]
    fn deserialize_instructions_loaded() {
        let json = r#"{"event": "InstructionsLoaded", "source": "CLAUDE.md"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::InstructionsLoaded { source } if source == "CLAUDE.md")
        );
    }

    #[test]
    fn deserialize_config_change() {
        let json = r#"{"event": "ConfigChange", "key": "model", "value": "opus"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ClaudeHookEvent::ConfigChange { key, .. } if key == "model"));
    }

    #[test]
    fn deserialize_cwd_changed() {
        let json = r#"{"event": "CwdChanged", "new_cwd": "/home/user/project"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::CwdChanged { new_cwd } if new_cwd == "/home/user/project")
        );
    }

    #[test]
    fn deserialize_worktree_create() {
        let json = r#"{"event": "WorktreeCreate", "path": "/tmp/worktree-abc"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::WorktreeCreate { path } if path.contains("worktree"))
        );
    }

    #[test]
    fn deserialize_worktree_remove() {
        let json = r#"{"event": "WorktreeRemove", "path": "/tmp/worktree-abc"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::WorktreeRemove { path } if path.contains("worktree"))
        );
    }

    #[test]
    fn deserialize_pre_compact() {
        let json = r#"{"event": "PreCompact"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ClaudeHookEvent::PreCompact {}));
    }

    #[test]
    fn deserialize_permission_request() {
        let json = r#"{"event": "PermissionRequest", "tool_name": "Bash", "description": "Execute rm -rf"}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::PermissionRequest { tool_name, .. } if tool_name == "Bash")
        );
    }

    #[test]
    fn deserialize_elicitation() {
        let json =
            r#"{"event": "Elicitation", "server": "oco-mcp", "request": {"question": "approve?"}}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::Elicitation { server, .. } if server == "oco-mcp")
        );
    }

    #[test]
    fn deserialize_elicitation_result() {
        let json = r#"{"event": "ElicitationResult", "server": "oco-mcp", "response": {"approved": true}}"#;
        let event: ClaudeHookEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(event, ClaudeHookEvent::ElicitationResult { server, .. } if server == "oco-mcp")
        );
    }

    // -----------------------------------------------------------------------
    // Unknown event → error
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_event_tag_fails() {
        let json = r#"{"event": "BogusEvent", "data": 42}"#;
        let result = serde_json::from_str::<ClaudeHookEvent>(json);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // HookDecision tests
    // -----------------------------------------------------------------------

    #[test]
    fn hook_decision_allow() {
        let d = HookDecision::allow();
        assert!(d.should_continue);
        assert!(d.stop_reason.is_none());
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["continue"], true);
    }

    #[test]
    fn hook_decision_deny() {
        let d = HookDecision::deny("policy violation");
        assert!(!d.should_continue);
        assert_eq!(d.stop_reason.as_deref(), Some("policy violation"));
    }

    #[test]
    fn hook_decision_with_output() {
        let d = HookDecision::allow().with_output("context", serde_json::json!({"key": "value"}));
        assert!(d.hook_specific_output.is_some());
        let out = d.hook_specific_output.unwrap();
        assert_eq!(out["context"]["key"], "value");
    }

    #[test]
    fn hook_decision_roundtrip() {
        let d = HookDecision::deny("test");
        let json = serde_json::to_string(&d).unwrap();
        let parsed: HookDecision = serde_json::from_str(&json).unwrap();
        assert!(!parsed.should_continue);
        assert_eq!(parsed.stop_reason.as_deref(), Some("test"));
    }

    // -----------------------------------------------------------------------
    // Mapping tests
    // -----------------------------------------------------------------------

    #[test]
    fn post_tool_use_maps_to_step_completed() {
        let event = ClaudeHookEvent::PostToolUse {
            tool_name: "Read".to_string(),
            output: serde_json::Value::Null,
            success: true,
            duration_ms: 100,
        };
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(
            oco,
            OrchestrationEvent::StepCompleted { success: true, .. }
        ));
    }

    #[test]
    fn post_tool_use_failure_maps_to_failed_step() {
        let event = ClaudeHookEvent::PostToolUseFailure {
            tool_name: "Bash".to_string(),
            error: "not found".to_string(),
        };
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(
            oco,
            OrchestrationEvent::StepCompleted { success: false, .. }
        ));
    }

    #[test]
    fn stop_maps_to_stopped() {
        let event = ClaudeHookEvent::Stop {
            reason: "task_complete".to_string(),
        };
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(oco, OrchestrationEvent::Stopped { .. }));
    }

    #[test]
    fn task_completed_maps_to_plan_step_completed() {
        let event = ClaudeHookEvent::TaskCompleted {
            task_id: "abc".to_string(),
            success: true,
            output: "done".to_string(),
        };
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(
            oco,
            OrchestrationEvent::PlanStepCompleted { success: true, .. }
        ));
    }

    #[test]
    fn subagent_start_maps_to_plan_step_started() {
        let event = ClaudeHookEvent::SubagentStart {
            agent_name: "debugger".to_string(),
        };
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(oco, OrchestrationEvent::PlanStepStarted { .. }));
    }

    #[test]
    fn subagent_stop_maps_to_plan_step_completed() {
        let event = ClaudeHookEvent::SubagentStop {
            agent_name: "debugger".to_string(),
            success: false,
        };
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(
            oco,
            OrchestrationEvent::PlanStepCompleted { success: false, .. }
        ));
    }

    #[test]
    fn session_end_maps_to_stopped() {
        let event = ClaudeHookEvent::SessionEnd {};
        let oco = event.to_orchestration_event().unwrap();
        assert!(matches!(oco, OrchestrationEvent::Stopped { .. }));
    }

    #[test]
    fn side_effect_events_return_none() {
        let side_effect_events = vec![
            ClaudeHookEvent::SessionStart {
                reason: String::new(),
            },
            ClaudeHookEvent::UserPromptSubmit {
                prompt: String::new(),
            },
            ClaudeHookEvent::PreToolUse {
                tool_name: String::new(),
                input: serde_json::Value::Null,
            },
            ClaudeHookEvent::StopFailure {
                error_type: String::new(),
            },
            ClaudeHookEvent::FileChanged {
                path: String::new(),
                change_type: String::new(),
            },
            ClaudeHookEvent::PostCompact {
                compact_summary: String::new(),
            },
            ClaudeHookEvent::TeammateIdle {
                teammate_name: String::new(),
            },
            ClaudeHookEvent::TaskCreated {
                task_id: String::new(),
                description: String::new(),
            },
            ClaudeHookEvent::InstructionsLoaded {
                source: String::new(),
            },
            ClaudeHookEvent::ConfigChange {
                key: String::new(),
                value: serde_json::Value::Null,
            },
            ClaudeHookEvent::CwdChanged {
                new_cwd: String::new(),
            },
            ClaudeHookEvent::WorktreeCreate {
                path: String::new(),
            },
            ClaudeHookEvent::WorktreeRemove {
                path: String::new(),
            },
            ClaudeHookEvent::PreCompact {},
            ClaudeHookEvent::PermissionRequest {
                tool_name: String::new(),
                description: String::new(),
            },
            ClaudeHookEvent::Elicitation {
                server: String::new(),
                request: serde_json::Value::Null,
            },
            ClaudeHookEvent::ElicitationResult {
                server: String::new(),
                response: serde_json::Value::Null,
            },
        ];

        for event in &side_effect_events {
            assert!(
                event.to_orchestration_event().is_none(),
                "{} should map to None",
                event.event_name()
            );
        }
    }

    #[test]
    fn event_name_covers_all_variants() {
        // Ensure event_name returns a non-empty string for all variants
        let events: Vec<ClaudeHookEvent> = vec![
            ClaudeHookEvent::SessionStart {
                reason: String::new(),
            },
            ClaudeHookEvent::UserPromptSubmit {
                prompt: String::new(),
            },
            ClaudeHookEvent::PreToolUse {
                tool_name: String::new(),
                input: serde_json::Value::Null,
            },
            ClaudeHookEvent::PostToolUse {
                tool_name: String::new(),
                output: serde_json::Value::Null,
                success: true,
                duration_ms: 0,
            },
            ClaudeHookEvent::PostToolUseFailure {
                tool_name: String::new(),
                error: String::new(),
            },
            ClaudeHookEvent::Stop {
                reason: String::new(),
            },
            ClaudeHookEvent::StopFailure {
                error_type: String::new(),
            },
            ClaudeHookEvent::FileChanged {
                path: String::new(),
                change_type: String::new(),
            },
            ClaudeHookEvent::PostCompact {
                compact_summary: String::new(),
            },
            ClaudeHookEvent::TaskCompleted {
                task_id: String::new(),
                success: true,
                output: String::new(),
            },
            ClaudeHookEvent::TeammateIdle {
                teammate_name: String::new(),
            },
            ClaudeHookEvent::SubagentStart {
                agent_name: String::new(),
            },
            ClaudeHookEvent::SubagentStop {
                agent_name: String::new(),
                success: true,
            },
            ClaudeHookEvent::SessionEnd {},
            ClaudeHookEvent::TaskCreated {
                task_id: String::new(),
                description: String::new(),
            },
            ClaudeHookEvent::InstructionsLoaded {
                source: String::new(),
            },
            ClaudeHookEvent::ConfigChange {
                key: String::new(),
                value: serde_json::Value::Null,
            },
            ClaudeHookEvent::CwdChanged {
                new_cwd: String::new(),
            },
            ClaudeHookEvent::WorktreeCreate {
                path: String::new(),
            },
            ClaudeHookEvent::WorktreeRemove {
                path: String::new(),
            },
            ClaudeHookEvent::PreCompact {},
            ClaudeHookEvent::PermissionRequest {
                tool_name: String::new(),
                description: String::new(),
            },
            ClaudeHookEvent::Elicitation {
                server: String::new(),
                request: serde_json::Value::Null,
            },
            ClaudeHookEvent::ElicitationResult {
                server: String::new(),
                response: serde_json::Value::Null,
            },
        ];

        assert_eq!(events.len(), 24, "must cover all 24 event variants");
        for event in &events {
            assert!(!event.event_name().is_empty());
        }
    }
}
