use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::budget::Budget;

/// Unique identifier for an orchestration session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level orchestration session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: SessionStatus,
    pub budget: Budget,
    pub step_count: u32,
    pub max_steps: u32,
    /// The original user request that started this session.
    pub user_request: String,
    /// Workspace root path.
    pub workspace_root: Option<String>,
    /// Pinned context items that persist across steps.
    pub pinned_context: Vec<String>,
    /// Running summary of the session so far (compressed).
    pub summary: Option<String>,
    /// External session ID for correlation (e.g. Claude Code session).
    /// Opaque string — never used for routing or logic decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_session_id: Option<String>,
}

impl Session {
    pub fn new(user_request: String, workspace_root: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            created_at: now,
            updated_at: now,
            status: SessionStatus::Active,
            budget: Budget::default(),
            step_count: 0,
            max_steps: 25,
            user_request,
            workspace_root,
            pinned_context: Vec::new(),
            summary: None,
            external_session_id: None,
        }
    }

    /// Set the external session ID for correlation with the calling system.
    pub fn with_external_session_id(mut self, id: impl Into<String>) -> Self {
        self.external_session_id = Some(id.into());
        self
    }

    pub fn is_within_budget(&self) -> bool {
        self.budget.is_within_limits() && self.step_count < self.max_steps
    }

    pub fn increment_step(&mut self) {
        self.step_count += 1;
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    Cancelled,
    BudgetExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_without_external_id() {
        let session = Session::new("test".into(), None);
        assert!(session.external_session_id.is_none());

        // Serializes without the field
        let json = serde_json::to_string(&session).unwrap();
        assert!(!json.contains("external_session_id"));
    }

    #[test]
    fn session_with_external_id() {
        let session = Session::new("test".into(), None).with_external_session_id("claude-abc-123");
        assert_eq!(
            session.external_session_id.as_deref(),
            Some("claude-abc-123")
        );

        // Serializes with the field
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"external_session_id\":\"claude-abc-123\""));
    }

    #[test]
    fn session_external_id_survives_roundtrip() {
        let session = Session::new("test".into(), None).with_external_session_id("ext-42");
        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.external_session_id.as_deref(), Some("ext-42"));
    }

    #[test]
    fn session_deserialize_without_external_id_defaults_none() {
        // Serialize a session, strip external_session_id, re-deserialize.
        // This simulates loading a session saved before the field was added.
        let original = Session::new("test".into(), None);
        let mut json_val: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        // Remove the field if present (it's skip_serializing_if None, but be safe)
        json_val
            .as_object_mut()
            .unwrap()
            .remove("external_session_id");
        let restored: Session = serde_json::from_value(json_val).unwrap();
        assert!(restored.external_session_id.is_none());
    }
}
