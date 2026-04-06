//! Structured telemetry, decision traces, and metrics collection.
//!
//! Provides tracing initialization, decision trace recording,
//! and session-level metrics aggregation.

pub mod init;
pub mod metrics;
pub mod proof_chain;
pub mod traces;

pub use init::{TelemetryConfig, init_tracing};
pub use metrics::SessionMetrics;
pub use proof_chain::{ProofChain, ProofChainError, ProofEnvelope};
pub use traces::{DecisionTraceCollector, RunSummaryBuilder, summary_to_text};
