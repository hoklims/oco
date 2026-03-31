//! Typed errors for the Claude Code adapter layer.

use std::fmt;

/// All errors that can occur in the claude-adapter crate.
#[derive(Debug, thiserror::Error)]
pub enum ClaudeAdapterError {
    /// The `claude` CLI was not found on PATH.
    #[error("claude CLI not found on PATH")]
    ClaudeNotFound,

    /// Failed to parse a version string.
    #[error("failed to parse version from: {0}")]
    VersionParse(String),

    /// The detected Claude Code version is below the minimum required.
    #[error("unsupported Claude Code version {found} (minimum: {minimum})")]
    VersionTooOld {
        found: crate::ClaudeVersion,
        minimum: crate::ClaudeVersion,
    },

    /// A required capability is not available in the current environment.
    #[error("capability not available: {0}")]
    CapabilityUnavailable(String),

    /// Hook event deserialization failed.
    #[error("hook event deserialization failed: {0}")]
    EventDeserialize(#[from] serde_json::Error),

    /// Managed settings restrict the requested operation.
    #[error("managed settings block this operation: {0}")]
    ManagedRestriction(String),

    /// I/O error (filesystem, process spawning, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Display helper for version-too-old (uses ClaudeVersion's Display).
impl ClaudeAdapterError {
    /// Convenience constructor for version-too-old.
    pub fn version_too_old(found: crate::ClaudeVersion, minimum: crate::ClaudeVersion) -> Self {
        Self::VersionTooOld { found, minimum }
    }
}

// ClaudeVersion needs Display for the error messages.
impl fmt::Display for crate::detection::ClaudeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
