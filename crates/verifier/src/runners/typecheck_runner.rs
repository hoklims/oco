use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use oco_shared_types::VerificationStrategy;
use tokio::process::Command;
use tracing::info;

use crate::error::VerifierError;
use crate::runner::{VerificationOutput, VerificationRunner};

/// Runs the project's type-checking command based on detected project type.
pub struct TypeCheckRunner {
    pub timeout_secs: u64,
}

impl TypeCheckRunner {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for TypeCheckRunner {
    fn default() -> Self {
        Self::new(120)
    }
}

#[async_trait]
impl VerificationRunner for TypeCheckRunner {
    async fn run(&self, _target: Option<&str>, working_dir: &str) -> Result<VerificationOutput> {
        let (program, args) = detect_typecheck_command(working_dir)?;
        info!(runner = "typecheck", %program, ?args, "running type-check command");

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
            parse_typecheck_errors(&stdout, &stderr)
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
        VerificationStrategy::TypeCheck
    }
}

fn detect_typecheck_command(working_dir: &str) -> Result<(String, Vec<String>)> {
    let dir = Path::new(working_dir);

    if dir.join("Cargo.toml").exists() {
        return Ok(("cargo".to_string(), vec!["check".to_string()]));
    }

    if dir.join("tsconfig.json").exists() || dir.join("package.json").exists() {
        return Ok((
            "npx".to_string(),
            vec!["tsc".to_string(), "--noEmit".to_string()],
        ));
    }

    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        return Ok(("mypy".to_string(), vec![".".to_string()]));
    }

    if dir.join("go.mod").exists() {
        return Ok((
            "go".to_string(),
            vec!["vet".to_string(), "./...".to_string()],
        ));
    }

    Err(VerifierError::UnsupportedProjectType {
        path: working_dir.to_string(),
    }
    .into())
}

fn parse_typecheck_errors(stdout: &str, stderr: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_typecheck() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let (prog, args) = detect_typecheck_command(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(prog, "cargo");
        assert_eq!(args, vec!["check"]);
    }

    #[test]
    fn detect_ts_typecheck() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let (prog, args) = detect_typecheck_command(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(prog, "npx");
        assert_eq!(args, vec!["tsc", "--noEmit"]);
    }

    #[test]
    fn detect_node_typecheck_via_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let (prog, _) = detect_typecheck_command(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(prog, "npx");
    }

    #[test]
    fn detect_python_typecheck() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        let (prog, args) = detect_typecheck_command(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(prog, "mypy");
        assert_eq!(args, vec!["."]);
    }

    #[test]
    fn detect_go_typecheck() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test").unwrap();
        let (prog, args) = detect_typecheck_command(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(prog, "go");
        assert_eq!(args, vec!["vet", "./..."]);
    }

    #[test]
    fn detect_unsupported_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_typecheck_command(dir.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn parse_errors_extracts_error_lines() {
        let stderr = "src/main.ts(5,3): error TS2322: Type 'string' is not assignable\n";
        let failures = parse_typecheck_errors("", stderr);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("error"));
    }
}
