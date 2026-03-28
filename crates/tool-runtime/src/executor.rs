use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use oco_shared_types::ToolResult;
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::error::ToolRuntimeError;

/// Trait for types that can execute tool invocations.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool by name with the given JSON arguments.
    async fn execute(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult, ToolRuntimeError>;

    /// Return the list of tool names this executor can handle.
    fn supported_tools(&self) -> Vec<String>;
}

// ---------------------------------------------------------------------------
// ShellToolExecutor
// ---------------------------------------------------------------------------

/// Executes tools by spawning shell commands.
pub struct ShellToolExecutor {
    working_dir: PathBuf,
    timeout: Duration,
}

impl ShellToolExecutor {
    /// Create a new executor rooted at `working_dir` with a default timeout.
    pub fn new(working_dir: PathBuf, timeout: Duration) -> Self {
        Self {
            working_dir,
            timeout,
        }
    }

    /// Build a `Command` for the given shell snippet.
    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", command_str]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", command_str]);
            c
        };
        cmd.current_dir(&self.working_dir);
        cmd
    }
}

#[async_trait]
impl ToolExecutor for ShellToolExecutor {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult, ToolRuntimeError> {
        let command_str = arguments
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolRuntimeError::InvalidArguments {
                tool_name: tool_name.to_string(),
                reason: "missing `command` string field".to_string(),
            })?;

        debug!(tool = %tool_name, cmd = %command_str, "executing shell command");

        let start = Instant::now();

        let result = tokio::time::timeout(self.timeout, self.build_command(command_str).output())
            .await
            .map_err(|_| ToolRuntimeError::ExecutionTimeout {
                tool_name: tool_name.to_string(),
                timeout_secs: self.timeout.as_secs(),
            })?
            .map_err(|e| ToolRuntimeError::ExecutionFailed {
                tool_name: tool_name.to_string(),
                reason: e.to_string(),
            })?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();
        let success = result.status.success();

        if !success {
            warn!(tool = %tool_name, code = ?result.status.code(), "command exited with error");
        }

        Ok(ToolResult {
            tool_name: tool_name.to_string(),
            success,
            output: serde_json::json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": result.status.code(),
            }),
            error: if success { None } else { Some(stderr) },
            duration_ms,
        })
    }

    fn supported_tools(&self) -> Vec<String> {
        vec!["shell".to_string(), "bash".to_string()]
    }
}

// ---------------------------------------------------------------------------
// FileToolExecutor
// ---------------------------------------------------------------------------

/// Executes file-system operations (read, write, list) scoped to a workspace.
pub struct FileToolExecutor {
    workspace_root: PathBuf,
}

impl FileToolExecutor {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    /// Resolve a possibly-relative path against the workspace root and ensure
    /// it does not escape the workspace via `..` traversal.
    fn resolve_path(&self, raw: &str) -> Result<PathBuf, ToolRuntimeError> {
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workspace_root.join(raw)
        };

        // Canonicalize what exists; for new files check parent.
        let resolved = if candidate.exists() {
            candidate
                .canonicalize()
                .map_err(|e| ToolRuntimeError::ExecutionFailed {
                    tool_name: "file".to_string(),
                    reason: format!("canonicalize failed: {e}"),
                })?
        } else {
            // For write_file the file may not exist yet – validate parent.
            let parent = candidate
                .parent()
                .ok_or(ToolRuntimeError::InvalidArguments {
                    tool_name: "file".to_string(),
                    reason: "no parent directory".to_string(),
                })?;
            if !parent.exists() {
                return Err(ToolRuntimeError::ExecutionFailed {
                    tool_name: "file".to_string(),
                    reason: format!("parent directory does not exist: {}", parent.display()),
                });
            }
            let canon_parent =
                parent
                    .canonicalize()
                    .map_err(|e| ToolRuntimeError::ExecutionFailed {
                        tool_name: "file".to_string(),
                        reason: format!("canonicalize parent failed: {e}"),
                    })?;
            canon_parent.join(candidate.file_name().unwrap_or_default())
        };

        let canon_root =
            self.workspace_root
                .canonicalize()
                .map_err(|e| ToolRuntimeError::ExecutionFailed {
                    tool_name: "file".to_string(),
                    reason: format!("canonicalize workspace root failed: {e}"),
                })?;

        if !resolved.starts_with(&canon_root) {
            return Err(ToolRuntimeError::PermissionDenied {
                tool_name: "file".to_string(),
                reason: format!(
                    "path escapes workspace: {} is not under {}",
                    resolved.display(),
                    canon_root.display()
                ),
            });
        }

        Ok(resolved)
    }

    async fn read_file(&self, path: &str) -> Result<ToolResult, ToolRuntimeError> {
        let resolved = self.resolve_path(path)?;
        let start = Instant::now();

        let content = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
            ToolRuntimeError::ExecutionFailed {
                tool_name: "read_file".to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(ToolResult {
            tool_name: "read_file".to_string(),
            success: true,
            output: serde_json::json!({
                "path": resolved.display().to_string(),
                "content": content,
                "size_bytes": content.len(),
            }),
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<ToolResult, ToolRuntimeError> {
        let resolved = self.resolve_path(path)?;
        let start = Instant::now();

        // Atomic write: temp file + rename to prevent corruption on crash
        let tmp = resolved.with_extension("oco_tmp");
        tokio::fs::write(&tmp, content)
            .await
            .map_err(|e| ToolRuntimeError::ExecutionFailed {
                tool_name: "write_file".to_string(),
                reason: e.to_string(),
            })?;
        if tokio::fs::rename(&tmp, &resolved).await.is_err() {
            // rename can fail cross-device; fall back to copy+remove
            tokio::fs::copy(&tmp, &resolved).await.map_err(|e| {
                ToolRuntimeError::ExecutionFailed {
                    tool_name: "write_file".to_string(),
                    reason: format!("atomic rename fallback failed: {e}"),
                }
            })?;
            let _ = tokio::fs::remove_file(&tmp).await;
        }

        Ok(ToolResult {
            tool_name: "write_file".to_string(),
            success: true,
            output: serde_json::json!({
                "path": resolved.display().to_string(),
                "bytes_written": content.len(),
            }),
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn list_directory(&self, path: &str) -> Result<ToolResult, ToolRuntimeError> {
        let resolved = self.resolve_path(path)?;
        let start = Instant::now();

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&resolved).await.map_err(|e| {
            ToolRuntimeError::ExecutionFailed {
                tool_name: "list_directory".to_string(),
                reason: e.to_string(),
            }
        })?;

        while let Some(entry) =
            dir.next_entry()
                .await
                .map_err(|e| ToolRuntimeError::ExecutionFailed {
                    tool_name: "list_directory".to_string(),
                    reason: e.to_string(),
                })?
        {
            let meta = entry.metadata().await.ok();
            entries.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                "size_bytes": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            }));
        }

        Ok(ToolResult {
            tool_name: "list_directory".to_string(),
            success: true,
            output: serde_json::json!({
                "path": resolved.display().to_string(),
                "entries": entries,
            }),
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[async_trait]
impl ToolExecutor for FileToolExecutor {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult, ToolRuntimeError> {
        match tool_name {
            "read_file" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolRuntimeError::InvalidArguments {
                        tool_name: tool_name.to_string(),
                        reason: "missing `path` string field".to_string(),
                    })?;
                self.read_file(path).await
            }
            "write_file" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolRuntimeError::InvalidArguments {
                        tool_name: tool_name.to_string(),
                        reason: "missing `path` string field".to_string(),
                    })?;
                let content = arguments
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolRuntimeError::InvalidArguments {
                        tool_name: tool_name.to_string(),
                        reason: "missing `content` string field".to_string(),
                    })?;
                self.write_file(path, content).await
            }
            "list_directory" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                self.list_directory(path).await
            }
            _ => Err(ToolRuntimeError::ToolNotFound {
                name: tool_name.to_string(),
            }),
        }
    }

    fn supported_tools(&self) -> Vec<String> {
        vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "list_directory".to_string(),
        ]
    }
}
