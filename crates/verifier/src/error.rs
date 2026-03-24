use thiserror::Error;

/// Errors that can occur during verification runs.
#[derive(Debug, Error)]
pub enum VerifierError {
    #[error("command failed with exit code {exit_code}: {message}")]
    CommandFailed { exit_code: i32, message: String },

    #[error("verification timed out after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    #[error("unsupported project type: no recognized manifest found in {path}")]
    UnsupportedProjectType { path: String },

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}
