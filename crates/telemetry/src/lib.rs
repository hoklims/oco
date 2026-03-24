//! Structured telemetry, decision traces, and metrics collection.
//!
//! Provides tracing initialization, decision trace recording,
//! and session-level metrics aggregation.

pub mod init;
pub mod metrics;
pub mod traces;

pub use init::{init_tracing, TelemetryConfig};
pub use metrics::SessionMetrics;
pub use traces::DecisionTraceCollector;
