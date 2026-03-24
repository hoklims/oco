//! Error types for the code intelligence crate.

use thiserror::Error;

/// Errors that can occur during code intelligence operations.
#[derive(Debug, Error)]
pub enum CodeIntelError {
    /// Failed to parse source code.
    #[error("parse error: {0}")]
    ParseError(String),

    /// The requested language is not supported.
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),

    /// An I/O error occurred (e.g. reading files for indexing).
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// An error occurred during symbol indexing.
    #[error("index error: {0}")]
    IndexError(String),
}
