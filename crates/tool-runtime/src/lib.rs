//! Tool runtime: registration, execution, and observation normalization.
//!
//! This crate provides:
//! - [`ToolRegistry`] — concurrent tool descriptor storage.
//! - [`ToolExecutor`] trait with [`ShellToolExecutor`] and [`FileToolExecutor`].
//! - [`ObservationNormalizer`] — converts [`ToolResult`] into [`Observation`].
//! - [`ToolRuntimeError`] — typed error enum for all failure modes.

pub mod error;
pub mod executor;
pub mod normalizer;
pub mod registry;

pub use error::ToolRuntimeError;
pub use executor::{FileToolExecutor, ShellToolExecutor, ToolExecutor};
pub use normalizer::ObservationNormalizer;
pub use registry::ToolRegistry;
