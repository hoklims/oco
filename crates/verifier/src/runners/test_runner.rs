use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use oco_shared_types::VerificationStrategy;
use tokio::process::Command;
use tracing::info;

use crate::error::VerifierError;
use crate::runner::{VerificationOutput, VerificationRunner};

/// Runs the project's test suite based on detected project type.
pub struct TestRunner {
    pub timeout_secs: u64,
}

impl TestRunner {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for TestRunner {
    fn default() -> Self {
        Self::new(300)
    }
}

#[async_trait]
impl VerificationRunner for TestRunner {
    async fn run(&self, target: Option<&str>, working_dir: &str) -> Result<VerificationOutput> {
        let (program, args) = detect_test_command(working_dir, target)?;
        info!(runner = "test", %program, ?args, "running test command");

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
            parse_failure_lines(&stdout, &stderr)
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
        VerificationStrategy::RunTests
    }
}

fn detect_test_command(working_dir: &str, target: Option<&str>) -> Result<(String, Vec<String>)> {
    let dir = Path::new(working_dir);

    if dir.join("Cargo.toml").exists() {
        let mut args = vec!["test".to_string()];
        if let Some(t) = target {
            args.push(t.to_string());
        }
        return Ok(("cargo".to_string(), args));
    }

    if dir.join("package.json").exists() {
        let mut args = vec!["test".to_string()];
        if let Some(t) = target {
            args.push("--".to_string());
            args.push(t.to_string());
        }
        return Ok(("npm".to_string(), args));
    }

    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        let mut args = Vec::new();
        if let Some(t) = target {
            args.push(t.to_string());
        }
        return Ok(("pytest".to_string(), args));
    }

    if dir.join("go.mod").exists() {
        let mut args = vec!["test".to_string(), "./...".to_string()];
        if let Some(t) = target {
            args.push("-run".to_string());
            args.push(t.to_string());
        }
        return Ok(("go".to_string(), args));
    }

    Err(VerifierError::UnsupportedProjectType {
        path: working_dir.to_string(),
    }
    .into())
}

fn parse_failure_lines(stdout: &str, stderr: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let lower = line.to_lowercase();
        if lower.contains("fail") || lower.contains("error") || lower.contains("panic") {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                failures.push(trimmed);
            }
        }
    }
    failures.truncate(50);
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let (prog, args) = detect_test_command(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(prog, "cargo");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn detect_rust_project_with_target() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let (prog, args) =
            detect_test_command(dir.path().to_str().unwrap(), Some("my_test")).unwrap();
        assert_eq!(prog, "cargo");
        assert_eq!(args, vec!["test", "my_test"]);
    }

    #[test]
    fn detect_node_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let (prog, args) = detect_test_command(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn detect_python_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        let (prog, _) = detect_test_command(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(prog, "pytest");
    }

    #[test]
    fn detect_go_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test").unwrap();
        let (prog, args) = detect_test_command(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(prog, "go");
        assert_eq!(args, vec!["test", "./..."]);
    }

    #[test]
    fn detect_unsupported_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = detect_test_command(dir.path().to_str().unwrap(), None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_failures_extracts_errors() {
        let stdout = "test result: ok\ntest foo ... FAILED\nerror[E0308]: mismatched types\n";
        let stderr = "panic at 'assertion failed'\n";
        let failures = parse_failure_lines(stdout, stderr);
        assert_eq!(failures.len(), 3);
        assert!(failures[0].contains("FAILED"));
        assert!(failures[1].contains("error"));
        assert!(failures[2].contains("panic"));
    }

    #[test]
    fn parse_failures_truncates_at_50() {
        let stdout: String = (0..100).map(|i| format!("error: failure #{i}\n")).collect();
        let failures = parse_failure_lines(&stdout, "");
        assert_eq!(failures.len(), 50);
    }

    #[test]
    fn parse_failures_empty_on_success() {
        let failures = parse_failure_lines("all tests passed\n", "");
        assert!(failures.is_empty());
    }
}
