use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use oco_shared_types::VerificationStrategy;
use tokio::process::Command;
use tracing::info;

use crate::error::VerifierError;
use crate::runner::{VerificationOutput, VerificationRunner};

/// Runs the project's lint command based on detected project type.
pub struct LintRunner {
    pub timeout_secs: u64,
}

impl LintRunner {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for LintRunner {
    fn default() -> Self {
        Self::new(120)
    }
}

#[async_trait]
impl VerificationRunner for LintRunner {
    async fn run(&self, _target: Option<&str>, working_dir: &str) -> Result<VerificationOutput> {
        let (program, args) = detect_lint_command(working_dir)?;
        info!(runner = "lint", %program, ?args, "running lint command");

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
            parse_lint_warnings(&stdout, &stderr)
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
        VerificationStrategy::Lint
    }
}

fn detect_lint_command(working_dir: &str) -> Result<(String, Vec<String>)> {
    let dir = Path::new(working_dir);

    if dir.join("Cargo.toml").exists() {
        return Ok((
            "cargo".to_string(),
            vec![
                "clippy".to_string(),
                "--".to_string(),
                "-D".to_string(),
                "warnings".to_string(),
            ],
        ));
    }

    if dir.join("package.json").exists() {
        return Ok((
            "npm".to_string(),
            vec!["run".to_string(), "lint".to_string()],
        ));
    }

    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        return Ok((
            "ruff".to_string(),
            vec!["check".to_string(), ".".to_string()],
        ));
    }

    if dir.join("go.mod").exists() {
        return Ok((
            "golangci-lint".to_string(),
            vec!["run".to_string(), "./...".to_string()],
        ));
    }

    Err(VerifierError::UnsupportedProjectType {
        path: working_dir.to_string(),
    }
    .into())
}

fn parse_lint_warnings(stdout: &str, stderr: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let lower = line.to_lowercase();
        if lower.contains("warning") || lower.contains("error") {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                failures.push(trimmed);
            }
        }
    }
    failures.truncate(50);
    failures
}
