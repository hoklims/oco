//! Core orchestration state machine and action loop for OCO.
//!
//! This crate implements the main orchestration loop that:
//! 1. Receives a user request
//! 2. Classifies task complexity
//! 3. Selects an action via the policy engine
//! 4. Executes the action
//! 5. Normalizes observations
//! 6. Updates state
//! 7. Repeats until stop condition

pub mod agent_teams;
pub mod config;
pub mod error;
pub mod eval;
pub mod graph_runner;
pub mod llm;
pub mod llm_router;
pub mod loop_runner;
pub mod ml_client;
pub mod replay;
pub mod review_packet;
pub mod runtime;
pub mod scorecard;
pub mod state;

pub use config::{
    DEFAULT_HISTORY_PATH, OrchestratorConfig, evaluate_baseline_freshness, load_baseline_history,
    load_gate_config, load_gate_config_strict, load_review_config_strict, load_scorecard_weights,
    promote_baseline,
};
pub use error::OrchestratorError;
pub use graph_runner::GraphRunner;
pub use llm::RetryingLlmProvider;
pub use llm_router::LlmRouter;
pub use loop_runner::OrchestrationLoop;
pub use replay::{LoadedTrace, ReplayControls, ReplayRegistry, ReplaySession};
pub use review_packet::build_review_packet;
pub use runtime::OrchestratorRuntime;
pub use scorecard::ScorecardBuilder;
pub use state::OrchestrationState;
