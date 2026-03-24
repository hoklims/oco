use anyhow::Result;
use async_trait::async_trait;
use oco_shared_types::VerificationStrategy;
use serde::{Deserialize, Serialize};

/// Output produced by a verification run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationOutput {
    pub passed: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub failures: Vec<String>,
}

/// Trait implemented by all verification runners.
#[async_trait]
pub trait VerificationRunner: Send + Sync {
    async fn run(&self, target: Option<&str>, working_dir: &str) -> Result<VerificationOutput>;
    fn strategy(&self) -> VerificationStrategy;
}
