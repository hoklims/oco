use anyhow::Result;
use oco_shared_types::VerificationStrategy;
use tracing::info;

use crate::runner::VerificationOutput;
use crate::runners::{BuildRunner, LintRunner, TestRunner, TypeCheckRunner};

/// Dispatches verification requests to the appropriate runner.
pub struct VerificationDispatcher {
    pub timeout_secs: u64,
}

impl VerificationDispatcher {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }

    /// Dispatch a verification run based on the given strategy.
    pub async fn dispatch(
        &self,
        strategy: VerificationStrategy,
        target: Option<&str>,
        working_dir: &str,
    ) -> Result<VerificationOutput> {
        use crate::runner::VerificationRunner;

        info!(?strategy, ?target, working_dir, "dispatching verification");

        match strategy {
            VerificationStrategy::RunTests => {
                TestRunner::new(self.timeout_secs)
                    .run(target, working_dir)
                    .await
            }
            VerificationStrategy::Build => {
                BuildRunner::new(self.timeout_secs)
                    .run(target, working_dir)
                    .await
            }
            VerificationStrategy::Lint => {
                LintRunner::new(self.timeout_secs)
                    .run(target, working_dir)
                    .await
            }
            VerificationStrategy::TypeCheck => {
                TypeCheckRunner::new(self.timeout_secs)
                    .run(target, working_dir)
                    .await
            }
            VerificationStrategy::Custom { ref command } => {
                run_custom_command(command, working_dir, self.timeout_secs).await
            }
        }
    }
}

impl Default for VerificationDispatcher {
    fn default() -> Self {
        Self::new(300)
    }
}

async fn run_custom_command(
    command: &str,
    working_dir: &str,
    timeout_secs: u64,
) -> Result<VerificationOutput> {
    use std::time::Instant;
    use tokio::process::Command;

    use crate::error::VerifierError;

    info!(command, working_dir, "running custom verification command");

    let start = Instant::now();

    let shell = if cfg!(target_os = "windows") {
        ("cmd", vec!["/C".to_string(), command.to_string()])
    } else {
        ("sh", vec!["-c".to_string(), command.to_string()])
    };

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        Command::new(shell.0)
            .args(&shell.1)
            .current_dir(working_dir)
            .output(),
    )
    .await
    .map_err(|_| VerifierError::Timeout { timeout_secs })?
    .map_err(VerifierError::IoError)?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    let passed = output.status.success();

    let failures = if !passed {
        vec![format!("Custom command exited with code {exit_code}")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_command_success() {
        let dispatcher = VerificationDispatcher::new(30);
        let dir = tempfile::tempdir().unwrap();
        let cmd = if cfg!(target_os = "windows") {
            "cmd /C echo ok"
        } else {
            "echo ok"
        };
        let result = dispatcher
            .dispatch(
                VerificationStrategy::Custom {
                    command: cmd.to_string(),
                },
                None,
                dir.path().to_str().unwrap(),
            )
            .await
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("ok"));
    }

    #[tokio::test]
    async fn custom_command_failure() {
        let dispatcher = VerificationDispatcher::new(30);
        let dir = tempfile::tempdir().unwrap();
        let cmd = if cfg!(target_os = "windows") {
            "cmd /C exit 1"
        } else {
            "exit 1"
        };
        let result = dispatcher
            .dispatch(
                VerificationStrategy::Custom {
                    command: cmd.to_string(),
                },
                None,
                dir.path().to_str().unwrap(),
            )
            .await
            .unwrap();
        assert!(!result.passed);
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn dispatch_unsupported_project_errors() {
        let dispatcher = VerificationDispatcher::new(30);
        let dir = tempfile::tempdir().unwrap();
        let result = dispatcher
            .dispatch(
                VerificationStrategy::RunTests,
                None,
                dir.path().to_str().unwrap(),
            )
            .await;
        assert!(result.is_err());
    }
}
