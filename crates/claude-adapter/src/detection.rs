//! Claude Code version detection and feature gates.
//!
//! Probes the `claude` CLI on PATH and parses the version string.
//! Feature-gate methods return whether a specific Claude Code feature
//! is available at the detected version.

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::ClaudeAdapterError;

/// A parsed Claude Code version (semver-like).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClaudeVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ClaudeVersion {
    /// Create a new version.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Probe `claude --version` on PATH. Returns `None` if the CLI is not found.
    pub async fn detect() -> Option<Self> {
        let output = tokio::process::Command::new("claude")
            .arg("--version")
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            warn!("claude --version exited with status {}", output.status);
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        match Self::parse(stdout.trim()) {
            Ok(v) => {
                debug!(version = %v, "detected Claude Code version");
                Some(v)
            }
            Err(e) => {
                warn!(error = %e, output = %stdout.trim(), "failed to parse claude version");
                None
            }
        }
    }

    /// Parse from strings like `"claude-code 2.1.80"`, `"2.1.80"`, or `"v2.1.80"`.
    pub fn parse(s: &str) -> Result<Self, ClaudeAdapterError> {
        // Try to find a version-like pattern: digits.digits.digits
        let version_str = s
            .split_whitespace()
            .find(|part| {
                let trimmed = part.strip_prefix('v').unwrap_or(part);
                trimmed.split('.').count() >= 3
                    && trimmed.split('.').all(|seg| seg.parse::<u32>().is_ok())
            })
            .map(|part| part.strip_prefix('v').unwrap_or(part))
            .ok_or_else(|| ClaudeAdapterError::VersionParse(s.to_string()))?;

        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() < 3 {
            return Err(ClaudeAdapterError::VersionParse(s.to_string()));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| ClaudeAdapterError::VersionParse(s.to_string()))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| ClaudeAdapterError::VersionParse(s.to_string()))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| ClaudeAdapterError::VersionParse(s.to_string()))?;

        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    // -----------------------------------------------------------------------
    // Feature gates — minimum versions for Claude Code features
    // -----------------------------------------------------------------------

    /// HTTP hooks (type: "http" in settings). Requires >= 2.1.63.
    pub fn supports_http_hooks(&self) -> bool {
        *self >= Self::new(2, 1, 63)
    }

    /// Agent Teams (teammate spawning, mesh communication). Requires >= 2.1.70.
    pub fn supports_agent_teams(&self) -> bool {
        *self >= Self::new(2, 1, 70)
    }

    /// MCP elicitation dialogs. Requires >= 2.1.76.
    pub fn supports_elicitation(&self) -> bool {
        *self >= Self::new(2, 1, 76)
    }

    /// MCP Channels (push events into active session). Requires >= 2.1.80.
    pub fn supports_channels(&self) -> bool {
        *self >= Self::new(2, 1, 80)
    }

    /// Permission relay via channels. Requires >= 2.1.81.
    pub fn supports_permission_relay(&self) -> bool {
        *self >= Self::new(2, 1, 81)
    }

    /// Native plugin system (.claude-plugin/). Requires >= 1.0.33.
    pub fn supports_plugins(&self) -> bool {
        *self >= Self::new(1, 0, 33)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_version_string() {
        let v = ClaudeVersion::parse("claude-code 2.1.80").unwrap();
        assert_eq!(v, ClaudeVersion::new(2, 1, 80));
    }

    #[test]
    fn parse_bare_version() {
        let v = ClaudeVersion::parse("2.1.63").unwrap();
        assert_eq!(v, ClaudeVersion::new(2, 1, 63));
    }

    #[test]
    fn parse_v_prefix() {
        let v = ClaudeVersion::parse("v1.0.33").unwrap();
        assert_eq!(v, ClaudeVersion::new(1, 0, 33));
    }

    #[test]
    fn parse_with_extra_text() {
        let v = ClaudeVersion::parse("Claude Code CLI version 2.1.76 (stable)").unwrap();
        assert_eq!(v, ClaudeVersion::new(2, 1, 76));
    }

    #[test]
    fn parse_garbage_fails() {
        assert!(ClaudeVersion::parse("not a version").is_err());
        assert!(ClaudeVersion::parse("").is_err());
        assert!(ClaudeVersion::parse("abc.def.ghi").is_err());
    }

    #[test]
    fn version_ordering() {
        let v1 = ClaudeVersion::new(2, 1, 63);
        let v2 = ClaudeVersion::new(2, 1, 70);
        let v3 = ClaudeVersion::new(2, 1, 80);
        let v4 = ClaudeVersion::new(3, 0, 0);

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);
        assert!(v1 < v4);
    }

    #[test]
    fn version_equality() {
        let a = ClaudeVersion::new(2, 1, 80);
        let b = ClaudeVersion::new(2, 1, 80);
        assert_eq!(a, b);
    }

    #[test]
    fn feature_gate_http_hooks() {
        assert!(!ClaudeVersion::new(2, 1, 62).supports_http_hooks());
        assert!(ClaudeVersion::new(2, 1, 63).supports_http_hooks());
        assert!(ClaudeVersion::new(2, 1, 64).supports_http_hooks());
    }

    #[test]
    fn feature_gate_agent_teams() {
        assert!(!ClaudeVersion::new(2, 1, 69).supports_agent_teams());
        assert!(ClaudeVersion::new(2, 1, 70).supports_agent_teams());
    }

    #[test]
    fn feature_gate_elicitation() {
        assert!(!ClaudeVersion::new(2, 1, 75).supports_elicitation());
        assert!(ClaudeVersion::new(2, 1, 76).supports_elicitation());
    }

    #[test]
    fn feature_gate_channels() {
        assert!(!ClaudeVersion::new(2, 1, 79).supports_channels());
        assert!(ClaudeVersion::new(2, 1, 80).supports_channels());
    }

    #[test]
    fn feature_gate_permission_relay() {
        assert!(!ClaudeVersion::new(2, 1, 80).supports_permission_relay());
        assert!(ClaudeVersion::new(2, 1, 81).supports_permission_relay());
    }

    #[test]
    fn feature_gate_plugins() {
        assert!(!ClaudeVersion::new(1, 0, 32).supports_plugins());
        assert!(ClaudeVersion::new(1, 0, 33).supports_plugins());
        // Higher major version also works
        assert!(ClaudeVersion::new(2, 0, 0).supports_plugins());
    }

    #[test]
    fn display_format() {
        let v = ClaudeVersion::new(2, 1, 80);
        assert_eq!(format!("{v}"), "2.1.80");
    }

    #[test]
    fn serde_roundtrip() {
        let v = ClaudeVersion::new(2, 1, 80);
        let json = serde_json::to_string(&v).unwrap();
        let parsed: ClaudeVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, parsed);
    }
}
