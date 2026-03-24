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
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            json_output: false,
            trace_file: None,
        }
    }
}

/// Initialize the global tracing subscriber with the given configuration.
///
/// If `trace_file` is set, a file appender layer is added alongside stdout.
pub fn init_tracing(config: TelemetryConfig) -> Result<()> {
    let env_filter =
        EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(ref trace_path) = config.trace_file {
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
