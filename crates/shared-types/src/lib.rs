//! Core domain types for the Open Context Orchestrator.
//!
//! This crate defines the fundamental types shared across all OCO crates:
//! sessions, actions, observations, context items, budgets, decision traces,
//! working memory, verification state, repo profiles, and replay scenarios.

pub mod action;
pub mod budget;
pub mod context;
pub mod memory;
pub mod observation;
pub mod profile;
pub mod replay;
pub mod session;
pub mod telemetry;
pub mod tool;
pub mod verification;

pub use action::*;
pub use budget::*;
pub use context::*;
pub use memory::*;
pub use observation::*;
pub use profile::*;
pub use replay::*;
pub use session::*;
pub use telemetry::*;
pub use tool::*;
pub use verification::*;
