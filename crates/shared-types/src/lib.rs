//! Core domain types for the Open Context Orchestrator.
//!
//! This crate defines the fundamental types shared across all OCO crates:
//! sessions, actions, observations, context items, budgets, decision traces,
//! working memory, verification state, repo profiles, and replay scenarios.

pub mod action;
pub mod agent;
pub mod budget;
pub mod capability;
pub mod context;
pub mod memory;
pub mod observation;
pub mod plan;
pub mod profile;
pub mod replay;
pub mod session;
pub mod team;
pub mod telemetry;
pub mod tool;
pub mod verification;

pub use action::*;
pub use agent::*;
pub use budget::*;
pub use capability::*;
pub use context::*;
pub use memory::*;
pub use observation::*;
pub use plan::*;
pub use profile::*;
pub use replay::*;
pub use session::*;
pub use team::*;
pub use telemetry::*;
pub use tool::*;
pub use verification::*;
