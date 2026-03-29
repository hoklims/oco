//! Runtime context that holds all live service instances for the orchestration loop.
//!
//! This wires together retrieval, code-intel, tool-runtime, verifier,
//! and context-engine into a single cohesive runtime.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use oco_code_intel::SymbolIndexer;
use oco_context_engine::{ContextBuilder, TokenEstimator};
use oco_retrieval::FtsIndex;
use oco_shared_types::{
    AssembledContext, ContextItem, ContextPriority, ContextSource, Observation, ObservationKind,
    ObservationSource, TierSelector, ToolDescriptor, VerificationStrategy, VerificationTier,
};
use oco_tool_runtime::{
    FileToolExecutor, ObservationNormalizer, ShellToolExecutor, ToolExecutor, ToolRegistry,
};
use oco_verifier::VerificationDispatcher;
use tracing::{info, warn};

use crate::config::OrchestratorConfig;
use crate::ml_client::MlWorkerClient;

/// Default system prompt used when none is supplied via config.
const DEFAULT_SYSTEM_PROMPT: &str = "You are an expert coding assistant. Analyze the provided context and respond to the user's request. \
Be precise, cite file paths and line numbers when relevant.";

/// Holds all runtime services needed by the orchestration loop.
pub struct OrchestratorRuntime {
    /// Workspace root path.
    pub workspace_root: PathBuf,
    /// Full-text search index.
    fts_index: Option<FtsIndex>,
    /// Symbol indexer for code intelligence.
    symbol_indexer: SymbolIndexer,
    /// Shell command executor.
    shell_executor: ShellToolExecutor,
    /// File operations executor.
    file_executor: FileToolExecutor,
    /// Tool registry.
    pub tool_registry: ToolRegistry,
    /// Verification dispatcher.
    verifier: VerificationDispatcher,
    /// Whether the workspace has been indexed.
    pub indexed: bool,
    /// Optional ML worker client for embeddings / reranking.
    pub ml_client: Option<MlWorkerClient>,
    /// Orchestrator configuration.
    pub config: OrchestratorConfig,
}

impl OrchestratorRuntime {
    /// Create a new runtime for a given workspace.
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_config(workspace_root, OrchestratorConfig::default())
    }

    /// Create a new runtime with explicit configuration.
    pub fn with_config(workspace_root: PathBuf, config: OrchestratorConfig) -> Self {
        // Try to connect to ML worker if configured.
        let ml_client = config.ml_worker_url.as_deref().and_then(|url| {
            match MlWorkerClient::new(url) {
                Ok(client) => {
                    info!(url = %url, "ML worker client configured");
                    Some(client)
                }
                Err(e) => {
                    warn!(url = %url, error = %e, "Failed to create ML worker client, continuing without ML");
                    None
                }
            }
        });

        let tool_registry = ToolRegistry::new();

        // Register built-in tools.
        tool_registry.register(ToolDescriptor {
            name: "shell".into(),
            description: "Execute a shell command".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["command"]
            }),
            is_write: true,
            requires_confirmation: false,
            timeout_secs: 30,
            tags: vec!["builtin".into(), "shell".into()],
        });

        tool_registry.register(ToolDescriptor {
            name: "read_file".into(),
            description: "Read the contents of a file".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }),
            is_write: false,
            requires_confirmation: false,
            timeout_secs: 10,
            tags: vec!["builtin".into(), "file".into()],
        });

        tool_registry.register(ToolDescriptor {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
            is_write: true,
            requires_confirmation: false,
            timeout_secs: 10,
            tags: vec!["builtin".into(), "file".into()],
        });

        tool_registry.register(ToolDescriptor {
            name: "list_directory".into(),
            description: "List the contents of a directory".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list" }
                },
                "required": ["path"]
            }),
            is_write: false,
            requires_confirmation: false,
            timeout_secs: 10,
            tags: vec!["builtin".into(), "file".into()],
        });

        Self {
            shell_executor: ShellToolExecutor::new(workspace_root.clone(), Duration::from_secs(30)),
            file_executor: FileToolExecutor::new(workspace_root.clone()),
            workspace_root,
            fts_index: None,
            symbol_indexer: SymbolIndexer::new(),
            tool_registry,
            verifier: VerificationDispatcher::new(30),
            indexed: false,
            ml_client,
            config,
        }
    }

    /// Index the workspace: scan files, build FTS index, extract symbols.
    pub fn index_workspace(&mut self) -> Result<IndexResult> {
        self.index_workspace_with_progress(None)
    }

    /// Index the workspace with optional progress callback (files_done, symbols_so_far).
    pub fn index_workspace_with_progress(
        &mut self,
        on_progress: Option<&dyn Fn(u32, u32)>,
    ) -> Result<IndexResult> {
        let ws = self.workspace_root.to_string_lossy().to_string();
        info!(workspace = %ws, "Indexing workspace");

        // Initialize FTS index
        let db_path = self.workspace_root.join(".oco").join("index.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let fts = FtsIndex::new(&db_path.to_string_lossy())?;

        // Scan source files
        let extensions = &[
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "cpp", "h", "hpp", "cs", "rb",
            "swift", "kt", "scala", "md", "toml", "yaml", "yml", "json",
        ];

        let mut file_count = 0u32;
        let mut symbol_count = 0u32;
        let mut batch = Vec::new();
        let workspace_root = self.workspace_root.clone();

        // Collect files first (avoids borrow conflict with symbol_indexer)
        let mut collected_files: Vec<(PathBuf, String)> = Vec::new();
        self.walk_dir(&workspace_root, extensions, &mut |path, content| {
            collected_files.push((path.to_path_buf(), content));
        })?;

        for (path, content) in &collected_files {
            let rel_path = path
                .strip_prefix(&workspace_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let id = rel_path.clone();

            // Add to FTS batch
            batch.push((id, rel_path.clone(), content.clone()));

            // Index symbols
            let lang =
                oco_code_intel::language_from_path(&path.to_string_lossy()).unwrap_or("text");
            if let Ok(symbols) = self.symbol_indexer.index_file(&rel_path, content, lang) {
                symbol_count += symbols.len() as u32;
            }

            file_count += 1;

            if let Some(cb) = on_progress {
                cb(file_count, symbol_count);
            }
        }

        // Batch insert into FTS
        if !batch.is_empty() {
            fts.index_documents_batch(batch)?;
        }

        self.fts_index = Some(fts);
        self.indexed = true;

        let result = IndexResult {
            file_count,
            symbol_count,
        };
        info!(
            files = file_count,
            symbols = symbol_count,
            "Indexing complete"
        );
        Ok(result)
    }

    /// Search the indexed workspace using FTS5.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>> {
        let fts = self
            .fts_index
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace not indexed"))?;

        let results = fts.search(query, limit)?;
        Ok(results
            .into_iter()
            .map(|r| SearchResult {
                path: r.path,
                snippet: r.snippet,
                score: r.rank,
            })
            .collect())
    }

    /// Find symbols by name across the indexed workspace.
    pub fn find_symbol(&self, name: &str) -> Vec<SymbolMatch> {
        self.symbol_indexer
            .find_symbol(name)
            .into_iter()
            .map(|(path, sym)| SymbolMatch {
                path,
                name: sym.name,
                kind: format!("{:?}", sym.kind),
                line: sym.line,
                signature: sym.signature,
            })
            .collect()
    }

    /// Execute a retrieval action and return an observation.
    ///
    /// When an ML worker is available, embeds the query and uses
    /// [`HybridRetriever`] (FTS + vector via RRF).  Otherwise falls back
    /// to FTS-only search.
    pub async fn execute_retrieval(&self, query: &str, max_results: u32) -> Result<Observation> {
        // Try hybrid retrieval when we have both an FTS index and ML worker.
        let search_results =
            if let (Some(_fts), Some(ml)) = (self.fts_index.as_ref(), self.ml_client.as_ref()) {
                // Attempt to get embeddings from the ML worker.
                match ml.embed(&[query.to_string()]).await {
                    Ok(embeddings) if !embeddings.is_empty() => {
                        use oco_retrieval::{HybridRetriever, InMemoryVectorBackend};

                        // Build a transient HybridRetriever with the current FTS index.
                        // The vector backend is empty (we only use the query embedding for
                        // the vector-side scoring), but the RRF fusion still benefits from
                        // having the FTS ranking combined with the embedding distance.
                        //
                        // NOTE: for a full implementation the vector store should be
                        // pre-populated during indexing; this wiring is intentionally
                        // incremental.
                        let fts_clone = FtsIndex::new(
                            &self
                                .workspace_root
                                .join(".oco")
                                .join("index.db")
                                .to_string_lossy(),
                        )?;
                        let vec_backend = InMemoryVectorBackend::new();
                        let retriever = HybridRetriever::new(fts_clone, vec_backend);

                        let hybrid_results = retriever
                            .retrieve(
                                query,
                                embeddings.into_iter().next().unwrap(),
                                1.0,
                                1.0,
                                max_results,
                            )
                            .await;

                        match hybrid_results {
                            Ok(results) => results
                                .into_iter()
                                .map(|r| SearchResult {
                                    path: r.path,
                                    snippet: r.content,
                                    score: r.score,
                                })
                                .collect(),
                            Err(e) => {
                                warn!(error = %e, "Hybrid retrieval failed, falling back to FTS");
                                self.search(query, max_results)?
                            }
                        }
                    }
                    Ok(_) => {
                        warn!("ML worker returned empty embeddings, falling back to FTS");
                        self.search(query, max_results)?
                    }
                    Err(e) => {
                        warn!(error = %e, "ML embed call failed, falling back to FTS");
                        self.search(query, max_results)?
                    }
                }
            } else {
                // FTS-only fallback.
                self.search(query, max_results)?
            };

        // Try symbol lookup
        let query_words: Vec<&str> = query.split_whitespace().collect();
        let mut symbol_results = Vec::new();
        for word in &query_words {
            let symbols = self.find_symbol(word);
            symbol_results.extend(symbols);
        }

        // Build observation content
        let mut content = String::new();

        if !search_results.is_empty() {
            content.push_str("## Code Search Results\n\n");
            for (i, r) in search_results.iter().enumerate() {
                content.push_str(&format!(
                    "### {} (score: {:.2})\n```\n{}\n```\n\n",
                    r.path, r.score, r.snippet
                ));
                if i >= 5 {
                    break;
                }
            }
        }

        if !symbol_results.is_empty() {
            content.push_str("## Symbol Matches\n\n");
            for sym in symbol_results.iter().take(10) {
                content.push_str(&format!(
                    "- **{}** ({}) in `{}` line {}{}\n",
                    sym.name,
                    sym.kind,
                    sym.path,
                    sym.line,
                    sym.signature
                        .as_ref()
                        .map(|s| format!(": `{s}`"))
                        .unwrap_or_default()
                ));
            }
            content.push('\n');
        }

        if content.is_empty() {
            content = format!("No results found for query: {query}");
        }

        let source_type = if self.ml_client.is_some() {
            "hybrid+symbols"
        } else {
            "fts5+symbols"
        };

        let token_estimate = TokenEstimator::estimate_tokens(&content);
        Ok(Observation::new(
            ObservationSource::Retrieval {
                source_type: source_type.into(),
            },
            ObservationKind::Text {
                content,
                metadata: Some(serde_json::json!({
                    "fts_results": search_results.len(),
                    "symbol_results": symbol_results.len(),
                    "query": query,
                })),
            },
            token_estimate,
        ))
    }

    /// Execute a shell tool call.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<Observation> {
        let result = match tool_name {
            "shell" | "bash" | "command" => {
                let command = arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("echo 'no command specified'");
                self.shell_executor
                    .execute("shell", serde_json::json!({"command": command}))
                    .await?
            }
            "read_file" | "file_read" => {
                let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                self.file_executor
                    .execute(
                        "read_file",
                        serde_json::json!({"operation": "read_file", "path": path}),
                    )
                    .await?
            }
            "write_file" | "file_write" => {
                let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = arguments
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.file_executor
                    .execute(
                        "write_file",
                        serde_json::json!({"operation": "write_file", "path": path, "content": content}),
                    )
                    .await?
            }
            "list_directory" | "ls" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                self.file_executor
                    .execute(
                        "list_directory",
                        serde_json::json!({"operation": "list_directory", "path": path}),
                    )
                    .await?
            }
            _ => {
                return Ok(Observation::new(
                    ObservationSource::System,
                    ObservationKind::Error {
                        message: format!("Unknown tool: {tool_name}"),
                        recoverable: true,
                    },
                    20,
                ));
            }
        };

        Ok(ObservationNormalizer::normalize(&result))
    }

    /// Execute a verification action.
    /// v2: Uses profile commands when available, falls back to auto-detection.
    pub async fn execute_verification(
        &self,
        strategy: &VerificationStrategy,
        target: Option<&str>,
    ) -> Result<Observation> {
        let ws = self.workspace_root.to_string_lossy().to_string();

        // v2: Override with profile-specific commands when set.
        let effective_strategy = match strategy {
            VerificationStrategy::RunTests if self.config.profile.test_command.is_some() => {
                VerificationStrategy::Custom {
                    command: self.config.profile.test_command.clone().unwrap(),
                }
            }
            VerificationStrategy::Build if self.config.profile.build_command.is_some() => {
                VerificationStrategy::Custom {
                    command: self.config.profile.build_command.clone().unwrap(),
                }
            }
            VerificationStrategy::Lint if self.config.profile.lint_command.is_some() => {
                VerificationStrategy::Custom {
                    command: self.config.profile.lint_command.clone().unwrap(),
                }
            }
            VerificationStrategy::TypeCheck if self.config.profile.typecheck_command.is_some() => {
                VerificationStrategy::Custom {
                    command: self.config.profile.typecheck_command.clone().unwrap(),
                }
            }
            other => other.clone(),
        };

        let output = self
            .verifier
            .dispatch(effective_strategy, target, &ws)
            .await?;

        let token_estimate = TokenEstimator::estimate_tokens(&output.stdout)
            + TokenEstimator::estimate_tokens(&output.stderr);

        Ok(Observation::new(
            ObservationSource::Verification {
                strategy: format!("{strategy:?}"),
            },
            ObservationKind::VerificationResult {
                passed: output.passed,
                output: if output.stdout.len() > 2000 {
                    format!("{}...(truncated)", &output.stdout[..2000])
                } else {
                    output.stdout
                },
                failures: output.failures,
            },
            token_estimate.min(2000),
        ))
    }

    /// Execute tiered verification based on which files changed.
    ///
    /// Uses [`TierSelector`] to pick Light/Standard/Thorough, then runs
    /// each strategy in order (stopping on first failure).
    /// Returns one [`Observation`] per strategy executed.
    pub async fn execute_verification_tiered(
        &self,
        changed_files: &[String],
    ) -> Result<Vec<Observation>> {
        let tier = TierSelector::select(changed_files);
        info!(
            ?tier,
            file_count = changed_files.len(),
            "tiered verification selected"
        );

        let ws = self.workspace_root.to_string_lossy().to_string();
        let result = self.verifier.dispatch_tiered(tier, &ws).await?;

        let observations: Vec<Observation> = result
            .results
            .iter()
            .map(|(strategy, output)| {
                let token_estimate = TokenEstimator::estimate_tokens(&output.stdout)
                    + TokenEstimator::estimate_tokens(&output.stderr);

                Observation::new(
                    ObservationSource::Verification {
                        strategy: format!("{strategy:?}"),
                    },
                    ObservationKind::VerificationResult {
                        passed: output.passed,
                        output: if output.stdout.len() > 2000 {
                            format!("{}...(truncated)", &output.stdout[..2000])
                        } else {
                            output.stdout.clone()
                        },
                        failures: output.failures.clone(),
                    },
                    token_estimate.min(2000),
                )
            })
            .collect();

        Ok(observations)
    }

    /// Get the verification tier for a set of changed files (for callers
    /// that want to inspect the tier before running verification).
    pub fn select_verification_tier(changed_files: &[String]) -> VerificationTier {
        TierSelector::select(changed_files)
    }

    /// Build assembled context for an LLM call.
    /// v2: Accepts `current_step` for staleness-aware assembly.
    pub fn build_context(
        &self,
        user_request: &str,
        observations: &[Observation],
        pinned: &[String],
        budget_tokens: u32,
        current_step: u32,
    ) -> AssembledContext {
        self.build_context_with_complexity(
            user_request,
            observations,
            pinned,
            budget_tokens,
            current_step,
            None,
        )
    }

    /// Build context with task-complexity-aware category budgets.
    pub fn build_context_with_complexity(
        &self,
        user_request: &str,
        observations: &[Observation],
        pinned: &[String],
        budget_tokens: u32,
        current_step: u32,
        task_complexity: Option<oco_shared_types::TaskComplexity>,
    ) -> AssembledContext {
        let mut builder = ContextBuilder::new(budget_tokens).with_staleness(current_step, 8); // Half-life of 8 steps

        // v2: Apply task-aware category budgets.
        if let Some(complexity) = task_complexity {
            builder = builder.with_category_budgets(
                oco_context_engine::CategoryBudgets::for_complexity(complexity),
            );
        }

        let system_prompt = self
            .config
            .system_prompt
            .as_deref()
            .unwrap_or(DEFAULT_SYSTEM_PROMPT);

        builder = builder
            .with_system_prompt(system_prompt)
            .with_user_request(user_request);

        // v2: Infer step from observation index (earlier observations = lower step).
        let obs_count = observations.len() as u32;

        // Add retrieved content as context items
        let retrieved_items: Vec<ContextItem> = observations
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o.source, ObservationSource::Retrieval { .. }))
            .filter_map(|(idx, o)| {
                if let ObservationKind::Text { content, .. } = &o.kind {
                    let step = current_step.saturating_sub(obs_count.saturating_sub(idx as u32));
                    Some(ContextItem {
                        key: o.id.to_string(),
                        label: "Retrieved context".into(),
                        content: content.clone(),
                        token_estimate: o.token_estimate,
                        priority: ContextPriority::High,
                        source: ContextSource::SearchResult {
                            query: "retrieval".into(),
                        },
                        pinned: false,
                        relevance: o.relevance_score.unwrap_or(0.5),
                        added_at: o.timestamp,
                        added_at_step: step,
                    })
                } else {
                    None
                }
            })
            .collect();

        if !retrieved_items.is_empty() {
            builder = builder.with_retrieved_items(retrieved_items);
        }

        // Add tool outputs
        let tool_items: Vec<ContextItem> = observations
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o.source, ObservationSource::ToolExecution { .. }))
            .filter_map(|(idx, o)| {
                let content = match &o.kind {
                    ObservationKind::Text { content, .. } => content.clone(),
                    ObservationKind::Structured { data } => {
                        serde_json::to_string_pretty(data).unwrap_or_default()
                    }
                    ObservationKind::CodeSnippet {
                        content,
                        file_path,
                        start_line,
                        ..
                    } => {
                        format!("// {file_path}:{start_line}\n{content}")
                    }
                    _ => return None,
                };
                let step = current_step.saturating_sub(obs_count.saturating_sub(idx as u32));
                Some(ContextItem {
                    key: o.id.to_string(),
                    label: "Tool output".into(),
                    content,
                    token_estimate: o.token_estimate,
                    priority: ContextPriority::Medium,
                    source: ContextSource::ToolOutput {
                        tool_name: "tool".into(),
                    },
                    pinned: false,
                    relevance: 0.6,
                    added_at: o.timestamp,
                    added_at_step: step,
                })
            })
            .collect();

        if !tool_items.is_empty() {
            builder = builder.with_tool_outputs(tool_items);
        }

        // Add pinned items (always current step — always fresh)
        let pinned_items: Vec<ContextItem> = pinned
            .iter()
            .enumerate()
            .map(|(i, content)| ContextItem {
                key: format!("pinned_{i}"),
                label: format!("Pinned context {i}"),
                content: content.clone(),
                token_estimate: TokenEstimator::estimate_tokens(content),
                priority: ContextPriority::Pinned,
                source: ContextSource::PinnedByUser,
                pinned: true,
                relevance: 1.0,
                added_at: chrono::Utc::now(),
                added_at_step: current_step,
            })
            .collect();

        if !pinned_items.is_empty() {
            builder = builder.with_pinned(pinned_items);
        }

        builder.build()
    }

    // --- Private helpers ---

    fn walk_dir(
        &self,
        dir: &Path,
        extensions: &[&str],
        callback: &mut dyn FnMut(&Path, String),
    ) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Skip hidden dirs, node_modules, target, .git, etc.
        if dir_name.starts_with('.')
            || dir_name == "node_modules"
            || dir_name == "target"
            || dir_name == "__pycache__"
            || dir_name == ".git"
            || dir_name == "dist"
            || dir_name == "build"
        {
            // Allow .oco dir to be skipped only for non-root
            if dir != self.workspace_root {
                return Ok(());
            }
        }

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.walk_dir(&path, extensions, callback)?;
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default();
                if extensions.contains(&ext.as_str()) {
                    // Skip large files (>100KB)
                    if let Ok(meta) = std::fs::metadata(&path)
                        && meta.len() > 100_000
                    {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        callback(&path, content);
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct IndexResult {
    pub file_count: u32,
    pub symbol_count: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub path: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct SymbolMatch {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub signature: Option<String>,
}
