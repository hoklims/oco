//! Verification runners for tests, builds, lints, and type checks.
//!
//! Detects project type from manifest files and runs the appropriate
//! verification command through a unified `VerificationRunner` trait.

pub mod dispatcher;
pub mod error;
pub mod runner;
pub mod runners;

pub use dispatcher::VerificationDispatcher;
pub use error::VerifierError;
pub use runner::{VerificationOutput, VerificationRunner};
pub use runners::{BuildRunner, LintRunner, TestRunner, TypeCheckRunner};
