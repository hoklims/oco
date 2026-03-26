//! MCP elicitation types for interactive orchestration decisions.
//!
//! Claude Code v2.1.76+ supports MCP elicitation — servers can request
//! structured user input via interactive dialogs mid-task. OCO uses this
//! for replan confirmation, architecture choices, and verify gate failures.
//!
//! ## Protocol
//!
//! 1. OCO detects a decision point (replan, ambiguity, verify failure).
//! 2. MCP server returns an `ElicitationRequest` in the tool response.
//! 3. Claude Code opens an interactive dialog with the fields.
//! 4. User fills the form → Claude Code sends `ElicitationResult` hook.
//! 5. OCO resumes execution with the user's choice.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// When the orchestrator should request user input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ElicitationPoint {
    /// A plan step failed and replanning is needed.
    Replan {
        /// Name of the failed step.
        failed_step: String,
        /// Replan attempt number.
        attempt: u32,
        /// Why the step failed.
        failure_reason: String,
    },
    /// A verify gate failed after a step.
    VerifyGate {
        /// Step that was verified.
        step_name: String,
        /// Which checks failed.
        failed_checks: Vec<String>,
    },
    /// The planner detected multiple valid approaches.
    Ambiguity {
        /// What the decision is about.
        question: String,
        /// Available options.
        options: Vec<ElicitationOption>,
    },
}

/// An option presented to the user in an ambiguity elicitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElicitationOption {
    /// Machine-readable identifier for the option.
    pub value: String,
    /// Short human-readable label for the option.
    pub label: String,
    /// Longer description.
    pub description: String,
    /// Whether this is the recommended option.
    #[serde(default)]
    pub recommended: bool,
}

/// A request for user input sent via MCP elicitation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationRequest {
    /// Unique ID for this elicitation (to match with the response).
    pub id: Uuid,
    /// What triggered this elicitation.
    pub point: ElicitationPoint,
    /// Human-readable title for the dialog.
    pub title: String,
    /// Human-readable description.
    pub description: String,
    /// Form fields for the dialog.
    pub fields: Vec<ElicitationField>,
    /// Default action if the user dismisses without responding.
    pub default_action: ElicitationAction,
}

impl ElicitationRequest {
    /// Create a replan elicitation request.
    pub fn replan(
        failed_step: impl Into<String>,
        attempt: u32,
        failure_reason: impl Into<String>,
    ) -> Self {
        let failed_step = failed_step.into();
        let failure_reason = failure_reason.into();
        Self {
            id: Uuid::new_v4(),
            point: ElicitationPoint::Replan {
                failed_step: failed_step.clone(),
                attempt,
                failure_reason: failure_reason.clone(),
            },
            title: format!("Replan required — step \"{failed_step}\" failed"),
            description: failure_reason,
            fields: vec![
                ElicitationField::Select {
                    name: "action".into(),
                    label: "Choose an action".into(),
                    options: vec![
                        "retry".into(),
                        "skip".into(),
                        "abort".into(),
                        "custom".into(),
                    ],
                    default: Some("retry".into()),
                },
                ElicitationField::Text {
                    name: "custom_instruction".into(),
                    label: "Custom instruction (if 'custom' selected)".into(),
                    placeholder: Some("Describe recovery approach".into()),
                },
            ],
            default_action: ElicitationAction::Retry,
        }
    }

    /// Create a verify gate elicitation request.
    pub fn verify_gate(step_name: impl Into<String>, failed_checks: Vec<String>) -> Self {
        let step_name = step_name.into();
        Self {
            id: Uuid::new_v4(),
            point: ElicitationPoint::VerifyGate {
                step_name: step_name.clone(),
                failed_checks: failed_checks.clone(),
            },
            title: format!("Verify gate failed — {step_name}"),
            description: format!("Failed checks: {}", failed_checks.join(", ")),
            fields: vec![ElicitationField::Select {
                name: "action".into(),
                label: "Choose an action".into(),
                options: vec![
                    "fix_and_reverify".into(),
                    "accept_and_continue".into(),
                    "rollback".into(),
                ],
                default: Some("fix_and_reverify".into()),
            }],
            default_action: ElicitationAction::FixAndReverify,
        }
    }

    /// Create an ambiguity elicitation request.
    ///
    /// # Panics (debug only)
    ///
    /// Debug-asserts that `options` is non-empty.
    pub fn ambiguity(question: impl Into<String>, options: Vec<ElicitationOption>) -> Self {
        debug_assert!(!options.is_empty(), "ambiguity options must not be empty");

        let question = question.into();
        let option_values: Vec<String> = options.iter().map(|o| o.value.clone()).collect();
        let recommended = options.iter().find(|o| o.recommended);
        let default = recommended.or(options.first()).map(|o| o.value.clone());
        // UseRecommended works for both cases: when there's a recommended option
        // it selects that; when there's none, the default falls back to the first option.
        let default_action = ElicitationAction::UseRecommended;
        Self {
            id: Uuid::new_v4(),
            point: ElicitationPoint::Ambiguity {
                question: question.clone(),
                options,
            },
            title: question,
            description: String::new(),
            fields: vec![ElicitationField::Select {
                name: "choice".into(),
                label: "Choose an approach".into(),
                options: option_values,
                default,
            }],
            default_action,
        }
    }
}

/// A field in the elicitation dialog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElicitationField {
    /// A dropdown/radio selection.
    Select {
        name: String,
        label: String,
        options: Vec<String>,
        #[serde(default)]
        default: Option<String>,
    },
    /// A free-text input field.
    Text {
        name: String,
        label: String,
        #[serde(default)]
        placeholder: Option<String>,
    },
}

/// What action to take based on elicitation response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ElicitationAction {
    /// Retry the failed step with a modified approach.
    Retry,
    /// Skip the step and continue.
    Skip,
    /// Abort the entire plan.
    Abort,
    /// Fix issues and re-verify.
    FixAndReverify,
    /// Accept failures and continue.
    AcceptAndContinue,
    /// Rollback the step's changes.
    Rollback,
    /// Use the recommended option (for ambiguity).
    UseRecommended,
    /// Custom instruction provided by the user.
    Custom { instruction: String },
}

/// The user's response to an elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ElicitationResponse {
    /// The user submitted the form with values.
    Submitted {
        /// Must match the `ElicitationRequest.id`.
        request_id: Uuid,
        /// Field values from the form.
        values: std::collections::HashMap<String, String>,
        /// Resolved action based on the response.
        action: ElicitationAction,
    },
    /// The user dismissed the dialog without responding.
    Dismissed {
        /// Must match the `ElicitationRequest.id`.
        request_id: Uuid,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replan_request_has_correct_fields() {
        let req = ElicitationRequest::replan("implement-auth", 1, "compilation error");
        // Select + Text (custom instruction)
        assert_eq!(req.fields.len(), 2);
        assert!(req.title.contains("implement-auth"));
        assert_eq!(req.default_action, ElicitationAction::Retry);
        match &req.point {
            ElicitationPoint::Replan {
                failed_step,
                attempt,
                ..
            } => {
                assert_eq!(failed_step, "implement-auth");
                assert_eq!(*attempt, 1);
            }
            _ => panic!("wrong elicitation point"),
        }
    }

    #[test]
    fn verify_gate_response_submitted() {
        let resp = ElicitationResponse::Submitted {
            request_id: Uuid::new_v4(),
            values: [("action".into(), "fix_and_reverify".into())]
                .into_iter()
                .collect(),
            action: ElicitationAction::FixAndReverify,
        };
        match &resp {
            ElicitationResponse::Submitted { action, .. } => {
                assert_eq!(*action, ElicitationAction::FixAndReverify);
            }
            _ => panic!("expected Submitted"),
        }
    }

    #[test]
    fn ambiguity_request_sets_recommended_default() {
        let options = vec![
            ElicitationOption {
                value: "redis".into(),
                label: "Redis".into(),
                description: "External cache".into(),
                recommended: false,
            },
            ElicitationOption {
                value: "lru".into(),
                label: "LRU".into(),
                description: "In-memory cache".into(),
                recommended: true,
            },
        ];
        let req = ElicitationRequest::ambiguity("Caching strategy", options);

        match &req.fields[0] {
            ElicitationField::Select { default, .. } => {
                assert_eq!(default.as_deref(), Some("lru"));
            }
            _ => panic!("expected Select field"),
        }
    }

    #[test]
    fn json_round_trip_request() {
        let req = ElicitationRequest::replan("step-1", 2, "test failure");
        let json = serde_json::to_string(&req).expect("serialize");
        let restored: ElicitationRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.id, req.id);
        assert_eq!(restored.default_action, ElicitationAction::Retry);
    }

    #[test]
    fn json_round_trip_response() {
        let resp = ElicitationResponse::Submitted {
            request_id: Uuid::new_v4(),
            values: [("action".into(), "abort".into())].into_iter().collect(),
            action: ElicitationAction::Abort,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let restored: ElicitationResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, resp);
    }

    // --- New tests for review issues ---

    #[test]
    fn json_snapshot_action_custom() {
        let action = ElicitationAction::Custom {
            instruction: "try a different approach".into(),
        };
        let json = serde_json::to_string(&action).expect("serialize");
        // Uniform adjacently-tagged form
        assert!(json.contains("\"type\":\"custom\""));
        assert!(json.contains("\"data\""));
        assert!(json.contains("\"instruction\":\"try a different approach\""));

        let restored: ElicitationAction = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, action);
    }

    #[test]
    fn json_snapshot_action_unit_variant() {
        let action = ElicitationAction::Retry;
        let json = serde_json::to_string(&action).expect("serialize");
        // Unit variant also gets uniform tagged form
        assert!(json.contains("\"type\":\"retry\""));

        let restored: ElicitationAction = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, action);
    }

    #[test]
    fn json_round_trip_dismissed() {
        let id = Uuid::new_v4();
        let resp = ElicitationResponse::Dismissed { request_id: id };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"status\":\"dismissed\""));

        let restored: ElicitationResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, resp);
    }

    #[test]
    fn ambiguity_no_recommended_defaults_to_first() {
        let options = vec![
            ElicitationOption {
                value: "option_a".into(),
                label: "Option A".into(),
                description: "First option".into(),
                recommended: false,
            },
            ElicitationOption {
                value: "option_b".into(),
                label: "Option B".into(),
                description: "Second option".into(),
                recommended: false,
            },
        ];
        let req = ElicitationRequest::ambiguity("Pick one", options);

        // Default should fall back to first option's value
        match &req.fields[0] {
            ElicitationField::Select { default, .. } => {
                assert_eq!(default.as_deref(), Some("option_a"));
            }
            _ => panic!("expected Select field"),
        }
    }

    #[test]
    fn option_has_value_field() {
        let opt = ElicitationOption {
            value: "redis_cache".into(),
            label: "Redis".into(),
            description: "External cache".into(),
            recommended: true,
        };
        let json = serde_json::to_string(&opt).expect("serialize");
        assert!(json.contains("\"value\":\"redis_cache\""));
        assert!(json.contains("\"label\":\"Redis\""));
    }

    #[test]
    fn replan_includes_custom_instruction_text_field() {
        let req = ElicitationRequest::replan("step-x", 1, "failed");
        let has_text_field = req.fields.iter().any(
            |f| matches!(f, ElicitationField::Text { name, .. } if name == "custom_instruction"),
        );
        assert!(
            has_text_field,
            "replan should include a Text field for custom instructions"
        );
    }
}
