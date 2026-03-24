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
        }
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
