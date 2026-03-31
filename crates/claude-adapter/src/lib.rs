//! Claude Code adapter layer for OCO.
//!
//! This crate isolates all coupling between Claude Code's documented runtime
//! surface (hook events, version API, managed settings) and OCO's internal
//! event model. If Claude Code changes event shapes, only this crate breaks.
//!
//! ## Modules
//!
//! - [`detection`] — Version probing and feature gates
//! - [`error`] — Typed errors for the adapter layer
//! - [`events`] — Claude hook event types and mapping to OrchestrationEvent
//! - [`negotiation`] — Runtime capability detection and integration mode selection

pub mod detection;
pub mod error;
pub mod events;
pub mod negotiation;

pub use detection::ClaudeVersion;
pub use error::ClaudeAdapterError;
pub use events::{ClaudeHookEvent, HookDecision};
pub use negotiation::{
    ClaudeCapabilities, ClaudeFeature, DoctorCheck, DoctorStatus, IntegrationMode,
};
