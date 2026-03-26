//! Integration tests for the OCO orchestration pipeline.
//!
//! These tests exercise the full orchestration stack end-to-end using
//! temporary workspaces and the stub LLM provider.

use std::sync::Arc;

use oco_orchestrator_core::config::OrchestratorConfig;
use oco_orchestrator_core::llm::StubLlmProvider;
use oco_orchestrator_core::loop_runner::OrchestrationLoop;
use oco_orchestrator_core::runtime::OrchestratorRuntime;
use oco_shared_types::{
    Budget, ContextPriority, Observation, ObservationKind, ObservationSource, OrchestratorAction,
    StopReason,
};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp workspace with sample Rust, Python, and TypeScript files.
fn create_sample_workspace() -> TempDir {
    let dir = TempDir::new().expect("failed to create temp dir");

    // Rust file with a struct and function
    std::fs::write(
        dir.path().join("auth.rs"),
        r#"/// Authentication module for the application.
use std::collections::HashMap;

pub struct AuthManager {
    tokens: HashMap<String, String>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Validate a user token and return the associated user ID.
    pub fn validate_token(&self, token: &str) -> Option<&String> {
        self.tokens.get(token)
    }

    /// Issue a new authentication token for a given user.
    pub fn issue_token(&mut self, user_id: String) -> String {
        let token = format!("tok_{}", user_id);
        self.tokens.insert(token.clone(), user_id);
        token
    }
}

pub fn hash_password(password: &str) -> String {
    format!("hashed_{password}")
}
"#,
    )
    .unwrap();

    // Python file with a class
    std::fs::write(
        dir.path().join("database.py"),
        r#""""Database connection pool and query helpers."""

import sqlite3
from typing import Optional, List, Dict, Any

class DatabasePool:
    """Manages a pool of database connections."""

    def __init__(self, db_path: str, pool_size: int = 5):
        self.db_path = db_path
        self.pool_size = pool_size
        self._connections: List[sqlite3.Connection] = []

    def get_connection(self) -> sqlite3.Connection:
        """Acquire a connection from the pool."""
        if self._connections:
            return self._connections.pop()
        return sqlite3.connect(self.db_path)

    def execute_query(self, query: str, params: Optional[Dict[str, Any]] = None) -> List[Any]:
        """Execute a SQL query and return results."""
        conn = self.get_connection()
        try:
            cursor = conn.execute(query, params or {})
            return cursor.fetchall()
        finally:
            self._connections.append(conn)

def create_tables(pool: DatabasePool) -> None:
    """Initialize the database schema."""
    pool.execute_query("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT)")
"#,
    )
    .unwrap();

    // TypeScript file with an interface and function
    std::fs::write(
        dir.path().join("api.ts"),
        r#"/** REST API route handlers for the user service. */

export interface UserResponse {
  id: number;
  name: string;
  email: string;
  createdAt: string;
}

export interface CreateUserRequest {
  name: string;
  email: string;
  password: string;
}

export async function getUser(userId: number): Promise<UserResponse> {
  const response = await fetch(`/api/users/${userId}`);
  if (!response.ok) {
    throw new Error(`User ${userId} not found`);
  }
  return response.json();
}

export async function createUser(req: CreateUserRequest): Promise<UserResponse> {
  const response = await fetch('/api/users', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}
"#,
    )
    .unwrap();

    dir
}

/// Build a stub LLM provider wrapped in Arc.
fn stub_llm() -> Arc<StubLlmProvider> {
    Arc::new(StubLlmProvider {
        model: "stub-test".into(),
    })
}

/// Build a default orchestrator config.
fn default_config() -> OrchestratorConfig {
    OrchestratorConfig::default()
}

/// Build a tight-budget config for exhaustion tests.
fn tight_budget_config() -> OrchestratorConfig {
    OrchestratorConfig {
        default_budget: Budget {
            max_context_tokens: 500,
            max_output_tokens: 100,
            max_total_tokens: 100,
            tokens_used: 0,
            max_tool_calls: 1,
            tool_calls_used: 0,
            max_retrievals: 1,
            retrievals_used: 0,
            max_duration_secs: 300,
            max_verify_cycles: 0,
            verify_cycles_used: 0,
        },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Test 1: Index and search
// ---------------------------------------------------------------------------

#[test]
fn test_index_and_search() {
    let workspace = create_sample_workspace();
    let mut runtime = OrchestratorRuntime::new(workspace.path().to_path_buf());

    // Index the workspace
    let result = runtime.index_workspace().expect("indexing should succeed");

    // We wrote 3 files (auth.rs, database.py, api.ts)
    assert_eq!(result.file_count, 3, "should index exactly 3 files");
    assert!(
        result.symbol_count > 0,
        "should extract at least one symbol, got {}",
        result.symbol_count
    );
    assert!(runtime.indexed, "runtime should be marked as indexed");

    // Search for known content
    let results = runtime
        .search("AuthManager", 5)
        .expect("search should succeed");
    assert!(
        !results.is_empty(),
        "search for 'AuthManager' should return results"
    );
    assert!(
        results[0].path.contains("auth.rs"),
        "top result should be auth.rs, got: {}",
        results[0].path
    );

    // Search for Python content
    let py_results = runtime
        .search("DatabasePool", 5)
        .expect("search should succeed");
    assert!(
        !py_results.is_empty(),
        "search for 'DatabasePool' should return results"
    );

    // Search for TypeScript content
    let ts_results = runtime
        .search("UserResponse", 5)
        .expect("search should succeed");
    assert!(
        !ts_results.is_empty(),
        "search for 'UserResponse' should return results"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Orchestrate trivial task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_orchestrate_trivial_task() {
    let llm = stub_llm();
    let config = default_config();
    let mut orch = OrchestrationLoop::new(config, llm);

    let state = orch
        .run("explain what a mutex is".into(), None)
        .await
        .expect("trivial orchestration should succeed");

    // Trivial tasks should complete quickly: classify as Trivial/Low, respond immediately.
    // The policy engine should select Respond on the first or second step.
    assert!(
        state.session.step_count <= 4,
        "trivial task should complete in <= 4 steps, got {}",
        state.session.step_count
    );

    // Should have traces
    assert!(
        !state.traces.is_empty(),
        "should produce at least one decision trace"
    );

    // Last action should be Stop with TaskComplete
    let last_action = state.action_history.last().expect("should have actions");
    assert!(
        matches!(
            last_action,
            OrchestratorAction::Stop {
                reason: StopReason::TaskComplete
            }
        ),
        "last action should be Stop(TaskComplete), got: {last_action:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Orchestrate complex task with workspace (v2: plan engine)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_orchestrate_complex_task_with_workspace() {
    let workspace = create_sample_workspace();
    let llm = stub_llm();
    let config = default_config();
    let mut orch = OrchestrationLoop::new(config, llm);

    let ws_path = workspace.path().to_string_lossy().to_string();
    let state = orch
        .run(
            "refactor the authentication module to use JWT tokens".into(),
            Some(ws_path),
        )
        .await
        .expect("complex orchestration should succeed");

    // v2: Medium+ tasks route through the plan engine (GraphRunner).
    // The action history should contain a Plan action and a Stop action.
    let did_plan = state
        .action_history
        .iter()
        .any(|a| matches!(a, OrchestratorAction::Plan { .. }));
    assert!(
        did_plan,
        "Medium+ task should route through plan engine"
    );

    // Should terminate with Stop
    let did_stop = state
        .action_history
        .iter()
        .any(|a| matches!(a, OrchestratorAction::Stop { .. }));
    assert!(did_stop, "should terminate with Stop action");

    // Should produce observations from the plan execution
    assert!(
        !state.observations.is_empty(),
        "plan execution should produce observations"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Budget exhaustion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_budget_exhaustion() {
    let workspace = create_sample_workspace();
    let llm = stub_llm();
    let config = tight_budget_config();
    let mut orch = OrchestrationLoop::new(config, llm);

    let ws_path = workspace.path().to_string_lossy().to_string();
    let state = orch
        .run(
            "perform a complete security audit of all authentication code, \
             refactor every module to use RBAC, add comprehensive tests, \
             and deploy to staging"
                .into(),
            Some(ws_path),
        )
        .await
        .expect("budget-limited orchestration should complete without panic");

    // The loop should have stopped — either by budget exhaustion or max steps
    let last_action = state.action_history.last().expect("should have actions");
    let stopped_correctly = matches!(
        last_action,
        OrchestratorAction::Stop {
            reason: StopReason::BudgetExhausted
                | StopReason::MaxStepsReached
                | StopReason::TaskComplete
                | StopReason::Error { .. }
        }
    );
    assert!(
        stopped_correctly,
        "should stop due to budget/max_steps/completion/error, got: {last_action:?}"
    );

    // Budget fields should reflect usage
    let budget = &state.session.budget;
    // At least some budget tracking should have happened
    assert!(
        budget.retrievals_used > 0 || budget.tool_calls_used > 0 || state.session.step_count > 0,
        "budget tracking should reflect at least some usage"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Context assembly from observations
// ---------------------------------------------------------------------------

#[test]
fn test_context_assembly_from_observations() {
    let workspace = create_sample_workspace();
    let mut runtime = OrchestratorRuntime::new(workspace.path().to_path_buf());
    runtime.index_workspace().expect("indexing should succeed");

    // Create mock observations of different types
    let retrieval_obs = Observation::new(
        ObservationSource::Retrieval {
            source_type: "fts5".into(),
        },
        ObservationKind::Text {
            content: "Found AuthManager struct in auth.rs with validate_token method.".into(),
            metadata: Some(serde_json::json!({"query": "auth"})),
        },
        80,
    );

    let tool_obs = Observation::new(
        ObservationSource::ToolExecution {
            tool_name: "shell".into(),
        },
        ObservationKind::Text {
            content: "cargo test output: 5 passed, 0 failed".into(),
            metadata: None,
        },
        40,
    );

    let error_obs = Observation::new(
        ObservationSource::System,
        ObservationKind::Error {
            message: "timeout on previous step".into(),
            recoverable: true,
        },
        20,
    );

    let observations = vec![retrieval_obs, tool_obs, error_obs];
    let pinned = vec!["Always consider backwards compatibility.".to_string()];

    let context = runtime.build_context(
        "refactor authentication to use JWT",
        &observations,
        &pinned,
        4096,
        3, // current_step
    );

    // Assembled context should include items
    assert!(
        !context.items.is_empty(),
        "assembled context should contain items"
    );

    // Should respect budget
    assert!(
        context.total_tokens <= context.budget_tokens,
        "total_tokens ({}) should not exceed budget ({})",
        context.total_tokens,
        context.budget_tokens
    );

    // Pinned items should be present (system prompt, user request, and our explicit pin)
    let pinned_items: Vec<_> = context.items.iter().filter(|item| item.pinned).collect();
    assert!(
        !pinned_items.is_empty(),
        "pinned items should be included in context"
    );
    // Pinned items should have at least Pinned priority (System >= Pinned)
    for item in &pinned_items {
        assert!(
            item.priority >= ContextPriority::Pinned,
            "pinned items should have at least Pinned priority, got {:?} for '{}'",
            item.priority,
            item.label
        );
    }

    // Our explicit pinned content should appear
    let has_explicit_pin = context
        .items
        .iter()
        .any(|item| item.content.contains("backwards compatibility"));
    assert!(
        has_explicit_pin,
        "explicitly pinned content should appear in assembled context"
    );

    // Items should be ordered by priority (higher priority first)
    let priorities: Vec<ContextPriority> = context.items.iter().map(|i| i.priority).collect();
    for window in priorities.windows(2) {
        assert!(
            window[0] >= window[1],
            "context items should be in descending priority order, got {:?} before {:?}",
            window[0],
            window[1]
        );
    }

    // Retrieved content should appear in the assembled items
    let has_retrieval = context
        .items
        .iter()
        .any(|item| item.content.contains("AuthManager"));
    assert!(
        has_retrieval,
        "retrieval observation content should appear in assembled context"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Symbol search
// ---------------------------------------------------------------------------

#[test]
fn test_symbol_search() {
    let workspace = create_sample_workspace();
    let mut runtime = OrchestratorRuntime::new(workspace.path().to_path_buf());
    runtime.index_workspace().expect("indexing should succeed");

    // Search for Rust struct
    let matches = runtime.find_symbol("AuthManager");
    assert!(
        !matches.is_empty(),
        "should find AuthManager symbol, got 0 results"
    );
    let auth_sym = &matches[0];
    assert_eq!(auth_sym.name, "AuthManager");
    assert!(
        auth_sym.path.contains("auth.rs"),
        "AuthManager should be in auth.rs, got: {}",
        auth_sym.path
    );
    // kind should indicate it's a struct-like construct
    assert!(
        auth_sym.kind.contains("Struct") || auth_sym.kind.contains("Class"),
        "AuthManager should be a struct/class, got kind: {}",
        auth_sym.kind
    );

    // Search for Rust function
    let fn_matches = runtime.find_symbol("hash_password");
    assert!(!fn_matches.is_empty(), "should find hash_password symbol");
    let fn_sym = &fn_matches[0];
    assert_eq!(fn_sym.name, "hash_password");
    assert!(
        fn_sym.kind.contains("Function") || fn_sym.kind.contains("Method"),
        "hash_password should be a function, got kind: {}",
        fn_sym.kind
    );

    // Search for Python class
    let py_matches = runtime.find_symbol("DatabasePool");
    assert!(!py_matches.is_empty(), "should find DatabasePool symbol");
    let py_sym = &py_matches[0];
    assert_eq!(py_sym.name, "DatabasePool");
    assert!(
        py_sym.path.contains("database.py"),
        "DatabasePool should be in database.py, got: {}",
        py_sym.path
    );

    // Search for TypeScript interface
    let ts_matches = runtime.find_symbol("UserResponse");
    assert!(!ts_matches.is_empty(), "should find UserResponse symbol");
    let ts_sym = &ts_matches[0];
    assert_eq!(ts_sym.name, "UserResponse");
    assert!(
        ts_sym.path.contains("api.ts"),
        "UserResponse should be in api.ts, got: {}",
        ts_sym.path
    );
}
