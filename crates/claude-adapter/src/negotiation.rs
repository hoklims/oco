//! Runtime capability detection and integration mode selection.
//!
//! Probes the environment (Claude Code version, managed settings, env vars)
//! to determine what features are available and which integration mode to use.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use crate::ClaudeAdapterError;
use crate::detection::ClaudeVersion;

// ---------------------------------------------------------------------------
// IntegrationMode — the 4 operating modes OCO supports
// ---------------------------------------------------------------------------

/// The integration mode OCO operates in, determined by runtime environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    /// Full hooks + MCP + skills + agents. Default for solo dev.
    Full,
    /// Plugin from marketplace. Hooks + MCP + skills. OCO runtime optional.
    Plugin,
    /// Managed settings restrict hooks/MCP. Minimal surface.
    EnterpriseSafe,
    /// No Claude Code client. Agent SDK or external runtime.
    SdkFallback,
}

impl std::fmt::Display for IntegrationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(f, "full"),
            Self::Plugin => write!(f, "plugin"),
            Self::EnterpriseSafe => write!(f, "enterprise_safe"),
            Self::SdkFallback => write!(f, "sdk_fallback"),
        }
    }
}

// ---------------------------------------------------------------------------
// ClaudeFeature — enumeration of probed features
// ---------------------------------------------------------------------------

/// A discrete Claude Code feature that can be probed at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaudeFeature {
    HttpHooks,
    AgentTeams,
    Channels,
    PermissionRelay,
    Plugins,
    Elicitation,
}

impl std::fmt::Display for ClaudeFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HttpHooks => write!(f, "HTTP Hooks"),
            Self::AgentTeams => write!(f, "Agent Teams"),
            Self::Channels => write!(f, "MCP Channels"),
            Self::PermissionRelay => write!(f, "Permission Relay"),
            Self::Plugins => write!(f, "Native Plugins"),
            Self::Elicitation => write!(f, "MCP Elicitation"),
        }
    }
}

// ---------------------------------------------------------------------------
// DoctorCheck — structured health check for `oco doctor`
// ---------------------------------------------------------------------------

/// A single health check result for `oco doctor` output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
}

/// Status of a doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

// ---------------------------------------------------------------------------
// ClaudeCapabilities — runtime probe result
// ---------------------------------------------------------------------------

/// The full result of probing the Claude Code environment.
///
/// Created by [`ClaudeCapabilities::negotiate()`] or constructed manually for tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCapabilities {
    /// Detected Claude Code version (None if CLI not found).
    pub version: Option<ClaudeVersion>,
    /// HTTP hooks available (>= 2.1.63).
    pub http_hooks: bool,
    /// Agent Teams available (>= 2.1.70).
    pub agent_teams: bool,
    /// MCP Channels available (>= 2.1.80).
    pub channels: bool,
    /// Native plugin system available (>= 1.0.33).
    pub plugins: bool,
    /// MCP elicitation available (>= 2.1.76).
    pub elicitation: bool,
    /// Managed settings restrict hooks to managed-only.
    pub managed_hooks_only: bool,
    /// Managed settings restrict MCP servers to managed-only.
    pub managed_mcp_only: bool,
    /// Agent SDK (Python/Node) detected on PATH.
    pub sdk_available: bool,
    /// When this probe was performed.
    pub probed_at: DateTime<Utc>,
}

/// Cache file validity duration (24 hours).
const CACHE_TTL_HOURS: i64 = 24;

/// Cache file name.
const CACHE_FILE: &str = "claude-capabilities.json";

impl ClaudeCapabilities {
    /// Full async probe: `claude --version` + env vars + managed settings.
    pub async fn negotiate() -> Self {
        let version = ClaudeVersion::detect().await;

        let (http_hooks, agent_teams, channels, plugins, elicitation) = match &version {
            Some(v) => (
                v.supports_http_hooks(),
                v.supports_agent_teams(),
                v.supports_channels(),
                v.supports_plugins(),
                v.supports_elicitation(),
            ),
            None => (false, false, false, false, false),
        };

        let (managed_hooks_only, managed_mcp_only) = detect_managed_settings();

        let sdk_available = detect_sdk_available().await;

        debug!(
            ?version,
            http_hooks,
            agent_teams,
            channels,
            plugins,
            managed_hooks_only,
            managed_mcp_only,
            sdk_available,
            "Claude Code capabilities negotiated"
        );

        Self {
            version,
            http_hooks,
            agent_teams,
            channels,
            plugins,
            elicitation,
            managed_hooks_only,
            managed_mcp_only,
            sdk_available,
            probed_at: Utc::now(),
        }
    }

    /// Construct capabilities from a known version (for tests or when version is pre-known).
    pub fn from_version(version: ClaudeVersion) -> Self {
        Self {
            http_hooks: version.supports_http_hooks(),
            agent_teams: version.supports_agent_teams(),
            channels: version.supports_channels(),
            plugins: version.supports_plugins(),
            elicitation: version.supports_elicitation(),
            version: Some(version),
            managed_hooks_only: false,
            managed_mcp_only: false,
            sdk_available: false,
            probed_at: Utc::now(),
        }
    }

    /// Construct capabilities representing no Claude Code available.
    pub fn none() -> Self {
        Self {
            version: None,
            http_hooks: false,
            agent_teams: false,
            channels: false,
            plugins: false,
            elicitation: false,
            managed_hooks_only: false,
            managed_mcp_only: false,
            sdk_available: false,
            probed_at: Utc::now(),
        }
    }

    /// What integration mode should OCO use based on these capabilities?
    pub fn recommended_mode(&self) -> IntegrationMode {
        if self.managed_hooks_only || self.managed_mcp_only {
            return IntegrationMode::EnterpriseSafe;
        }

        if self.version.is_none() {
            return IntegrationMode::SdkFallback;
        }

        if self.http_hooks {
            IntegrationMode::Full
        } else {
            IntegrationMode::Plugin
        }
    }

    /// Check if a specific feature is available.
    pub fn has(&self, feature: ClaudeFeature) -> bool {
        match feature {
            ClaudeFeature::HttpHooks => self.http_hooks,
            ClaudeFeature::AgentTeams => self.agent_teams,
            ClaudeFeature::Channels => self.channels,
            ClaudeFeature::PermissionRelay => self
                .version
                .as_ref()
                .is_some_and(|v| v.supports_permission_relay()),
            ClaudeFeature::Plugins => self.plugins,
            ClaudeFeature::Elicitation => self.elicitation,
        }
    }

    /// Generate a structured health report for `oco doctor`.
    pub fn doctor_report(&self) -> Vec<DoctorCheck> {
        let mut checks = Vec::new();

        // Claude CLI detection
        match &self.version {
            Some(v) => checks.push(DoctorCheck {
                name: "Claude Code CLI".to_string(),
                status: DoctorStatus::Pass,
                detail: format!("v{v} detected"),
            }),
            None => checks.push(DoctorCheck {
                name: "Claude Code CLI".to_string(),
                status: DoctorStatus::Fail,
                detail: "not found on PATH".to_string(),
            }),
        }

        // Feature checks
        let features = [
            (
                ClaudeFeature::HttpHooks,
                "required for real-time event flow",
            ),
            (
                ClaudeFeature::AgentTeams,
                "enables parallel agent execution",
            ),
            (ClaudeFeature::Plugins, "enables native plugin installation"),
            (
                ClaudeFeature::Elicitation,
                "enables interactive MCP dialogs",
            ),
            (ClaudeFeature::Channels, "enables push events to session"),
        ];

        for (feature, description) in features {
            let available = self.has(feature);
            checks.push(DoctorCheck {
                name: format!("{feature}"),
                status: if available {
                    DoctorStatus::Pass
                } else {
                    DoctorStatus::Warn
                },
                detail: if available {
                    "available".to_string()
                } else {
                    format!("not available — {description}")
                },
            });
        }

        // Managed settings
        if self.managed_hooks_only {
            checks.push(DoctorCheck {
                name: "Managed Settings".to_string(),
                status: DoctorStatus::Warn,
                detail: "hooks restricted to managed-only".to_string(),
            });
        }
        if self.managed_mcp_only {
            checks.push(DoctorCheck {
                name: "Managed MCP".to_string(),
                status: DoctorStatus::Warn,
                detail: "MCP servers restricted to managed-only".to_string(),
            });
        }

        // Integration mode
        let mode = self.recommended_mode();
        checks.push(DoctorCheck {
            name: "Integration Mode".to_string(),
            status: match mode {
                IntegrationMode::Full => DoctorStatus::Pass,
                IntegrationMode::Plugin => DoctorStatus::Pass,
                IntegrationMode::EnterpriseSafe => DoctorStatus::Warn,
                IntegrationMode::SdkFallback => DoctorStatus::Warn,
            },
            detail: format!("{mode}"),
        });

        checks
    }

    /// Persist capabilities to `.oco/claude-capabilities.json`.
    pub fn save(&self, workspace: &Path) -> Result<(), ClaudeAdapterError> {
        let dir = workspace.join(".oco");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(CACHE_FILE);
        let json =
            serde_json::to_string_pretty(self).map_err(ClaudeAdapterError::EventDeserialize)?;
        std::fs::write(&path, json)?;
        debug!(path = %path.display(), "saved Claude capabilities cache");
        Ok(())
    }

    /// Load cached capabilities. Returns `None` if cache doesn't exist or is stale (>24h).
    pub fn load_cached(workspace: &Path) -> Option<Self> {
        let path = workspace.join(".oco").join(CACHE_FILE);
        let content = std::fs::read_to_string(&path).ok()?;
        let caps: Self = serde_json::from_str(&content).ok()?;

        let age = Utc::now() - caps.probed_at;
        if age.num_hours() > CACHE_TTL_HOURS {
            debug!(
                age_hours = age.num_hours(),
                "cached capabilities are stale, re-probe needed"
            );
            return None;
        }

        debug!(
            age_hours = age.num_hours(),
            "loaded cached Claude capabilities"
        );
        Some(caps)
    }
}

// ---------------------------------------------------------------------------
// Managed settings detection (platform-specific)
// ---------------------------------------------------------------------------

/// Detect managed settings that restrict hooks or MCP servers.
fn detect_managed_settings() -> (bool, bool) {
    let mut hooks_only = false;
    let mut mcp_only = false;

    for path in managed_settings_paths() {
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content)
        {
            if parsed
                .get("allowManagedHooksOnly")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                hooks_only = true;
                warn!(path = %path.display(), "managed settings: hooks restricted");
            }
            if parsed
                .get("allowManagedMcpServersOnly")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                mcp_only = true;
                warn!(path = %path.display(), "managed settings: MCP restricted");
            }
        }
    }

    (hooks_only, mcp_only)
}

/// Platform-specific paths for managed settings files.
fn managed_settings_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Library/Application Support/ClaudeCode/managed-settings.json",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/etc/claude-code/managed-settings.json"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(PathBuf::from(
            r"C:\Program Files\ClaudeCode\managed-settings.json",
        ));
    }

    paths
}

/// Detect if Agent SDK is available (check for `claude-agent-sdk` Python package or Node module).
async fn detect_sdk_available() -> bool {
    // Check Python SDK
    let python_check = tokio::process::Command::new("python3")
        .args(["-c", "import claude_agent_sdk"])
        .output()
        .await;

    if let Ok(output) = python_check
        && output.status.success()
    {
        return true;
    }

    // Check Node SDK
    let node_check = tokio::process::Command::new("node")
        .args(["-e", "require('@anthropic-ai/claude-agent-sdk')"])
        .output()
        .await;

    if let Ok(output) = node_check
        && output.status.success()
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_version_full_capabilities() {
        let caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 81));
        assert!(caps.http_hooks);
        assert!(caps.agent_teams);
        assert!(caps.elicitation);
        assert!(caps.channels);
        assert!(caps.plugins);
        assert_eq!(caps.recommended_mode(), IntegrationMode::Full);
    }

    #[test]
    fn from_version_old_version() {
        let caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 60));
        assert!(!caps.http_hooks);
        assert!(!caps.agent_teams);
        assert!(!caps.elicitation);
        assert!(!caps.channels);
        // Plugin mode when no HTTP hooks
        assert_eq!(caps.recommended_mode(), IntegrationMode::Plugin);
    }

    #[test]
    fn none_capabilities() {
        let caps = ClaudeCapabilities::none();
        assert!(caps.version.is_none());
        assert!(!caps.http_hooks);
        assert_eq!(caps.recommended_mode(), IntegrationMode::SdkFallback);
    }

    #[test]
    fn enterprise_safe_when_managed() {
        let mut caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 81));
        caps.managed_hooks_only = true;
        assert_eq!(caps.recommended_mode(), IntegrationMode::EnterpriseSafe);
    }

    #[test]
    fn enterprise_safe_when_mcp_managed() {
        let mut caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 81));
        caps.managed_mcp_only = true;
        assert_eq!(caps.recommended_mode(), IntegrationMode::EnterpriseSafe);
    }

    #[test]
    fn has_feature_checks() {
        let caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 81));
        assert!(caps.has(ClaudeFeature::HttpHooks));
        assert!(caps.has(ClaudeFeature::AgentTeams));
        assert!(caps.has(ClaudeFeature::Channels));
        assert!(caps.has(ClaudeFeature::PermissionRelay));
        assert!(caps.has(ClaudeFeature::Plugins));
        assert!(caps.has(ClaudeFeature::Elicitation));
    }

    #[test]
    fn has_feature_missing() {
        let caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 60));
        assert!(!caps.has(ClaudeFeature::HttpHooks));
        assert!(!caps.has(ClaudeFeature::AgentTeams));
        assert!(!caps.has(ClaudeFeature::Channels));
        assert!(!caps.has(ClaudeFeature::PermissionRelay));
    }

    #[test]
    fn doctor_report_full_version() {
        let caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 81));
        let report = caps.doctor_report();
        assert!(!report.is_empty());

        // First check should be CLI detection (Pass)
        assert_eq!(report[0].name, "Claude Code CLI");
        assert_eq!(report[0].status, DoctorStatus::Pass);

        // Integration mode should be last and Pass for Full
        let last = report.last().unwrap();
        assert_eq!(last.name, "Integration Mode");
        assert_eq!(last.status, DoctorStatus::Pass);
    }

    #[test]
    fn doctor_report_no_claude() {
        let caps = ClaudeCapabilities::none();
        let report = caps.doctor_report();
        assert_eq!(report[0].status, DoctorStatus::Fail);
    }

    #[test]
    fn doctor_report_managed_settings() {
        let mut caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 81));
        caps.managed_hooks_only = true;
        let report = caps.doctor_report();
        let managed = report
            .iter()
            .find(|c| c.name == "Managed Settings")
            .unwrap();
        assert_eq!(managed.status, DoctorStatus::Warn);
    }

    #[test]
    fn save_and_load_cache_roundtrip() {
        let dir = TempDir::new().unwrap();
        let caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 80));
        caps.save(dir.path()).unwrap();

        let loaded = ClaudeCapabilities::load_cached(dir.path()).unwrap();
        assert_eq!(loaded.version, caps.version);
        assert_eq!(loaded.http_hooks, caps.http_hooks);
        assert_eq!(loaded.agent_teams, caps.agent_teams);
    }

    #[test]
    fn cache_staleness_returns_none() {
        let dir = TempDir::new().unwrap();
        let mut caps = ClaudeCapabilities::from_version(ClaudeVersion::new(2, 1, 80));
        // Fake an old probe time
        caps.probed_at = Utc::now() - chrono::Duration::hours(25);
        caps.save(dir.path()).unwrap();

        assert!(ClaudeCapabilities::load_cached(dir.path()).is_none());
    }

    #[test]
    fn cache_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        assert!(ClaudeCapabilities::load_cached(dir.path()).is_none());
    }

    #[test]
    fn integration_mode_display() {
        assert_eq!(format!("{}", IntegrationMode::Full), "full");
        assert_eq!(format!("{}", IntegrationMode::Plugin), "plugin");
        assert_eq!(
            format!("{}", IntegrationMode::EnterpriseSafe),
            "enterprise_safe"
        );
        assert_eq!(format!("{}", IntegrationMode::SdkFallback), "sdk_fallback");
    }

    #[test]
    fn integration_mode_serde_roundtrip() {
        let mode = IntegrationMode::Full;
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: IntegrationMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, parsed);
    }

    #[test]
    fn claude_feature_display() {
        assert_eq!(format!("{}", ClaudeFeature::HttpHooks), "HTTP Hooks");
        assert_eq!(format!("{}", ClaudeFeature::AgentTeams), "Agent Teams");
    }

    #[test]
    fn doctor_check_serde() {
        let check = DoctorCheck {
            name: "test".to_string(),
            status: DoctorStatus::Pass,
            detail: "all good".to_string(),
        };
        let json = serde_json::to_string(&check).unwrap();
        let parsed: DoctorCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.status, DoctorStatus::Pass);
    }
}
