use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use oco_shared_types::VerificationStrategy;
use tokio::process::Command;
use tracing::info;

use crate::error::VerifierError;
use crate::runner::{VerificationOutput, VerificationRunner};

/// Runs the project's build command based on detected project type.
pub struct BuildRunner {
    pub timeout_secs: u64,
}

impl BuildRunner {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for BuildRunner {
    fn default() -> Self {
        Self::new(300)
    }
}

#[async_trait]
impl VerificationRunner for BuildRunner {
    async fn run(&self, _target: Option<&str>, working_dir: &str) -> Result<VerificationOutput> {
        let (program, args) = detect_build_command(working_dir)?;
        info!(runner = "build", %program, ?args, "running build command");

        let start = Instant::now();
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            Command::new(&program)
                .args(&args)
                .current_dir(working_dir)
                .output(),
        )
        .await
        .map_err(|_| VerifierError::Timeout {
            timeout_secs: self.timeout_secs,
        })?
        .map_err(VerifierError::IoError)?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);
        let passed = output.status.success();

        let failures = if !passed {
            parse_build_errors(&stdout, &stderr)
        } else {
            Vec::new()
        };

        Ok(VerificationOutput {
            passed,
            stdout,
            stderr,
            exit_code,
            duration_ms,
            failures,
        })
    }

    fn strategy(&self) -> VerificationStrategy {
        VerificationStrategy::Build
    }
}

fn detect_build_command(working_dir: &str) -> Result<(String, Vec<String>)> {
    let dir = Path::new(working_dir);

    if dir.join("Cargo.toml").exists() {
        return Ok(("cargo".to_string(), vec!["build".to_string()]));
    }

    if dir.join("package.json").exists() {
        return Ok((
            "npm".to_string(),
            vec!["run".to_string(), "build".to_string()],
        ));
    }

    if dir.join("pyproject.toml").exists() {
        return Ok((
            "python".to_string(),
            vec!["-m".to_string(), "build".to_string()],
        ));
    }

    if dir.join("go.mod").exists() {
        return Ok((
            "go".to_string(),
            vec!["build".to_string(), "./...".to_string()],
        ));
    }

    Err(VerifierError::UnsupportedProjectType {
        path: working_dir.to_string(),
    }
    .into())
}

fn parse_build_errors(stdout: &str, stderr: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let lower = line.to_lowercase();
        if lower.contains("error") {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                failures.push(trimmed);
            }
        }
    }
    failures.truncate(50);
    failures
}
