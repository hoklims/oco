use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Configuration for telemetry initialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Log level filter (e.g. "info", "debug", "oco_verifier=trace").
    pub log_level: String,
    /// Whether to format log output as JSON.
    pub json_output: bool,
    /// Optional file path to write traces to.
    pub trace_file: Option<String>,
    /// Write logs to a file instead of stdout/stderr (keeps terminal clean for UI).
    /// When set, no logs appear on stdout/stderr unless trace_file is also set.
    #[serde(default)]
    pub log_to_file: Option<String>,
    /// If true, suppress all log output (only errors go to stderr).
    #[serde(default)]
    pub quiet: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            json_output: false,
            trace_file: None,
            log_to_file: None,
            quiet: false,
        }
    }
}

/// Initialize the global tracing subscriber with the given configuration.
///
/// Priority:
/// 1. `quiet` — only ERROR level on stderr
/// 2. `log_to_file` — all logs go to file, terminal stays clean
/// 3. `trace_file` — logs go to both stdout and the file
/// 4. Default — logs go to stdout
pub fn init_tracing(config: TelemetryConfig) -> Result<()> {
    if config.quiet {
        // Quiet mode: only errors on stderr
        let env_filter = EnvFilter::new("error");
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_writer(std::io::stderr))
            .try_init()
            .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
        return Ok(());
    }

    let env_filter =
        EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(ref log_path) = config.log_to_file {
        // All logs to file, terminal stays clean for UI output
        if let Some(parent) = std::path::Path::new(log_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|e| anyhow::anyhow!("failed to open log file {log_path}: {e}"))?;
        let file_writer = Mutex::new(file);

        if config.json_output {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json().with_writer(file_writer))
                .try_init()
                .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().with_writer(file_writer))
                .try_init()
                .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
        }
    } else if let Some(ref trace_path) = config.trace_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(trace_path)
            .map_err(|e| anyhow::anyhow!("failed to open trace file {trace_path}: {e}"))?;
        let file_writer = Mutex::new(file);

        if config.json_output {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json().with_writer(std::io::stdout))
                .with(fmt::layer().json().with_writer(file_writer))
                .try_init()
                .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().with_writer(std::io::stdout))
                .with(fmt::layer().with_writer(file_writer))
                .try_init()
                .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
        }
    } else if config.json_output {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .try_init()
            .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer())
            .try_init()
            .map_err(|e| anyhow::anyhow!("failed to init tracing: {e}"))?;
    }

    Ok(())
}
