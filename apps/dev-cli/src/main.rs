mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use ui::{CheckStatus, OutputFormat, Renderer, UiEvent};

#[derive(Parser)]
#[command(name = "oco", version, about = "Open Context Orchestrator — Dev CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    /// Output format (human, json, jsonl)
    #[arg(long, default_value = "human", global = true)]
    format: String,

    /// Suppress all output except final result
    #[arg(long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the OCO server
    Serve {
        /// Bind address
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port
        #[arg(long, default_value_t = 3000)]
        port: u16,
    },
    /// Run a one-shot orchestration request
    Run {
        /// The request to process
        request: String,
        /// Workspace root path
        #[arg(long)]
        workspace: Option<String>,
        /// LLM provider: claude-code (default), anthropic, ollama, stub
        #[arg(long, default_value = "claude-code")]
        provider: String,
        /// Model name
        #[arg(long)]
        model: Option<String>,
        /// Maximum steps
        #[arg(long, default_value_t = 25)]
        max_steps: u32,
    },
    /// Index a workspace for retrieval
    Index {
        /// Path to the workspace to index
        path: String,
    },
    /// Search the indexed codebase
    Search {
        /// Search query
        query: String,
        /// Workspace path to search in
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Maximum results
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Show the status of the running server
    Status {
        /// Server URL
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
    },
    /// Display a session's decision trace
    Trace {
        /// Session ID
        session_id: String,
        /// Server URL
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
    },
    /// Generate a default oco.toml configuration file
    Init {
        /// Output path (defaults to ./oco.toml)
        #[arg(long, default_value = "oco.toml")]
        output: String,
    },
    /// Classify a task's complexity (used by UserPromptSubmit hook)
    Classify {
        /// The user prompt to classify
        prompt: String,
        /// Workspace root path
        #[arg(long, default_value = ".")]
        workspace: String,
    },
    /// Check tool policy gate (used by PreToolUse hook)
    GateCheck {
        /// Tool name
        #[arg(long)]
        tool: String,
        /// Tool input as JSON string
        #[arg(long, default_value = "{}")]
        input: String,
        /// Write policy level (allow_all, require_confirmation, deny_destructive)
        #[arg(long, default_value = "require_confirmation")]
        policy: String,
    },
    /// Record a tool observation (used by PostToolUse hook)
    Observe {
        /// Tool name
        #[arg(long)]
        tool: String,
        /// Observation status (ok, error)
        #[arg(long, default_value = "ok")]
        status: String,
        /// Optional output data (JSON string)
        #[arg(long)]
        output: Option<String>,
    },
    /// Run evaluation scenarios
    Eval {
        /// Path to scenarios JSONL file
        scenarios: String,
        /// Output results to file (JSON)
        #[arg(long)]
        output: Option<String>,
        /// LLM provider: claude-code (default), anthropic, ollama, stub
        #[arg(long, default_value = "claude-code")]
        provider: String,
    },
    /// Check plugin health and configuration
    Doctor {
        /// Workspace path to check
        #[arg(long, default_value = ".")]
        workspace: String,
    },
    /// Show a past run's trace and summary
    Runs {
        #[command(subcommand)]
        action: RunsAction,
    },
}

#[derive(Subcommand)]
enum RunsAction {
    /// Show a past run's trace
    Show {
        /// Run/session ID (or "last" for most recent)
        id: String,
        /// Workspace path (to find .oco/runs/)
        #[arg(long, default_value = ".")]
        workspace: String,
    },
    /// List recent runs
    List {
        /// Workspace path
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Max entries
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let out_format = OutputFormat::from_str(&cli.format);

    // Initialize telemetry.
    // In human mode: redirect logs to .oco/oco.log to keep the terminal clean.
    // In quiet mode: suppress all logs except errors.
    let log_to_file = if out_format == OutputFormat::Human && !cli.quiet {
        Some(".oco/oco.log".to_string())
    } else {
        None
    };
    oco_telemetry::init_tracing(oco_telemetry::TelemetryConfig {
        log_level: cli.log_level.clone(),
        json_output: matches!(out_format, OutputFormat::Json | OutputFormat::Jsonl),
        trace_file: None,
        log_to_file,
        quiet: cli.quiet,
    })?;

    let mut r: Box<dyn Renderer> = if cli.quiet {
        Box::new(ui::quiet::QuietRenderer::new())
    } else {
        ui::create_renderer(out_format)
    };

    match cli.command {
        Commands::Serve { host, port } => cmd_serve(&mut *r, host, port).await?,
        Commands::Run {
            request,
            workspace,
            provider,
            model,
            max_steps: _,
        } => cmd_run(&mut *r, out_format, request, workspace, provider, model).await?,
        Commands::Index { path } => cmd_index(&mut *r, path)?,
        Commands::Search {
            query,
            workspace,
            limit,
        } => cmd_search(&mut *r, out_format, query, workspace, limit)?,
        Commands::Status { url } => cmd_status(&mut *r, out_format, url).await?,
        Commands::Trace { session_id, url } => cmd_trace(session_id, url).await?,
        Commands::Init { output } => cmd_init(&mut *r, output)?,
        Commands::Classify { prompt, workspace } => cmd_classify(out_format, prompt, workspace)?,
        Commands::GateCheck {
            tool,
            input,
            policy,
        } => cmd_gate_check(out_format, tool, input, policy)?,
        Commands::Observe {
            tool,
            status,
            output,
        } => cmd_observe(&mut *r, out_format, tool, status, output)?,
        Commands::Eval {
            scenarios,
            output,
            provider,
        } => cmd_eval(&mut *r, out_format, scenarios, output, provider).await?,
        Commands::Doctor { workspace } => cmd_doctor(&mut *r, workspace)?,
        Commands::Runs { action } => match action {
            RunsAction::Show { id, workspace } => cmd_runs_show(&mut *r, id, workspace)?,
            RunsAction::List { workspace, limit } => cmd_runs_list(&mut *r, workspace, limit)?,
        },
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════
// Command implementations
// ═══════════════════════════════════════════════════════════

async fn cmd_serve(r: &mut dyn Renderer, host: String, port: u16) -> Result<()> {
    let mut config =
        oco_orchestrator_core::OrchestratorConfig::load_from_dir(&std::env::current_dir()?);
    config.bind_address = host;
    config.port = port;

    let mut server = oco_mcp_server::McpServer::new(config);

    // Auto-detect dashboard dist directory.
    let dashboard_candidates = [
        PathBuf::from("apps/dashboard/dist"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("dashboard")))
            .unwrap_or_default(),
    ];
    let has_dashboard = if let Some(dir) = std::env::var("OCO_DASHBOARD_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            dashboard_candidates
                .iter()
                .find(|d| d.join("index.html").exists())
                .cloned()
        }) {
        server = server.with_dashboard_dir(dir);
        true
    } else {
        false
    };

    // Bind first to get the real port (especially when --port 0).
    let (listener, app) = server.bind().await?;
    let real_addr = listener.local_addr()?;

    r.emit(UiEvent::ServerListening {
        host: real_addr.ip().to_string(),
        port: real_addr.port(),
    });

    if has_dashboard {
        eprintln!("Dashboard: http://{real_addr}/dashboard");
    }

    axum::serve(listener, app).await?;
    Ok(())
}

async fn cmd_run(
    r: &mut dyn Renderer,
    format: OutputFormat,
    request: String,
    workspace: Option<String>,
    provider: String,
    model: Option<String>,
) -> Result<()> {
    let mut config = oco_orchestrator_core::OrchestratorConfig::default();
    config.default_budget.max_duration_secs = 120;

    let base_llm: Arc<dyn oco_orchestrator_core::llm::LlmProvider> = match provider.as_str() {
        "claude-code" => {
            let model_name = model.unwrap_or_else(|| "sonnet".to_string());
            let cc_config = oco_orchestrator_core::llm::ClaudeCodeConfig::new(&model_name);
            Arc::new(oco_orchestrator_core::llm::ClaudeCodeProvider::new(
                cc_config,
            ))
        }
        "anthropic" => {
            let model_name = model.unwrap_or_else(|| config.llm.model.clone());
            let anthropic_config =
                oco_orchestrator_core::llm::AnthropicConfig::from_env(&model_name, None)?;
            Arc::new(oco_orchestrator_core::llm::AnthropicProvider::new(
                anthropic_config,
            )?)
        }
        "ollama" => {
            let model_name = model.unwrap_or_else(|| "llama3.2".to_string());
            let ollama_config = oco_orchestrator_core::llm::OllamaConfig::new(&model_name);
            Arc::new(oco_orchestrator_core::llm::OllamaProvider::new(
                ollama_config,
            )?)
        }
        "stub" => Arc::new(oco_orchestrator_core::llm::StubLlmProvider {
            model: model.unwrap_or_else(|| config.llm.model.clone()),
        }),
        other => {
            anyhow::bail!(
                "unknown provider '{other}'. Available: claude-code, anthropic, ollama, stub"
            );
        }
    };

    // Wrap with retry logic for rate-limit resilience (uses config.llm.max_retries).
    let llm: Arc<dyn oco_orchestrator_core::llm::LlmProvider> = Arc::new(
        oco_orchestrator_core::RetryingLlmProvider::new(base_llm, config.llm.max_retries),
    );

    r.emit(UiEvent::RunStarted {
        provider: llm.provider_name().to_string(),
        model: llm.model_name().to_string(),
        request: request.clone(),
        workspace: workspace.clone(),
    });

    let mut orchestrator = oco_orchestrator_core::OrchestrationLoop::new(config, llm);

    // Index workspace if provided
    if let Some(ref ws) = workspace {
        let ws_path = PathBuf::from(ws)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(ws));

        let t = Instant::now();
        let spinner = if format == OutputFormat::Human {
            let tr = ui::terminal::TerminalRenderer::new();
            Some(tr.spinner(&format!("Indexing {}…", ws_path.display())))
        } else {
            r.emit(UiEvent::IndexStarted {
                path: ws_path.clone(),
            });
            None
        };

        orchestrator.with_workspace(ws_path.clone());

        if let Some(sp) = spinner {
            sp.finish_and_clear();
            r.emit(UiEvent::IndexCompleted {
                files: 0,
                symbols: 0,
                duration_ms: t.elapsed().as_millis() as u64,
            });
        }
    }

    // Set up live event channel
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::unbounded_channel::<oco_shared_types::OrchestrationEvent>();
    orchestrator.with_event_channel(event_tx);

    // Spawn the orchestration loop on a separate task (possible now that FtsIndex is Send)
    let run_start = Instant::now();
    let run_handle = tokio::spawn(async move { orchestrator.run(request, workspace).await });

    // Consume events in real time as the loop executes
    let mut trace_events = Vec::new();
    while let Some(event) = event_rx.recv().await {
        trace_events.push(event.clone());
        match event {
            oco_shared_types::OrchestrationEvent::StepCompleted {
                step,
                ref action,
                ref reason,
                duration_ms,
                ref budget_snapshot,
                ..
            } => {
                let action_type = match action {
                    oco_shared_types::OrchestratorAction::Respond { .. } => "RESPOND",
                    oco_shared_types::OrchestratorAction::Retrieve { .. } => "RETRIEVE",
                    oco_shared_types::OrchestratorAction::ToolCall { .. } => "TOOL_CALL",
                    oco_shared_types::OrchestratorAction::Verify { .. } => "VERIFY",
                    oco_shared_types::OrchestratorAction::UpdateMemory { .. } => "MEMORY",
                    oco_shared_types::OrchestratorAction::Stop { .. } => "STOP",
                    oco_shared_types::OrchestratorAction::Plan { .. } => "PLAN",
                    oco_shared_types::OrchestratorAction::Delegate { .. } => "DELEGATE",
                    oco_shared_types::OrchestratorAction::Message { .. } => "MESSAGE",
                    oco_shared_types::OrchestratorAction::Replan { .. } => "REPLAN",
                };
                let tokens_max = budget_snapshot.tokens_used + budget_snapshot.tokens_remaining;
                r.emit(UiEvent::RunStepCompleted {
                    step,
                    action_type: action_type.to_string(),
                    reason: reason.clone(),
                    tokens_used: budget_snapshot.tokens_used,
                    tokens_max,
                    duration_ms,
                });
            }
            oco_shared_types::OrchestrationEvent::BudgetWarning {
                ref resource,
                utilization,
            } => {
                r.emit(UiEvent::Warning {
                    message: format!(
                        "Budget warning: {} at {:.0}% utilization",
                        resource,
                        utilization * 100.0
                    ),
                });
            }
            _ => {}
        }
    }

    // Get the final state from the spawned task
    let state = run_handle.await??;
    let run_duration = run_start.elapsed().as_millis() as u64;

    // Derive success from the terminal action, not session.status (which may not be updated)
    let success = state.action_history.iter().rev().any(|a| {
        matches!(
            a,
            oco_shared_types::OrchestratorAction::Stop {
                reason: oco_shared_types::StopReason::TaskComplete
            }
        )
    });

    let session_id = state.session.id.0.to_string();

    r.emit(UiEvent::RunFinished {
        session_id: session_id.clone(),
        steps: state.session.step_count,
        tokens_used: state.session.budget.tokens_used,
        tokens_max: state.session.budget.max_total_tokens,
        duration_ms: run_duration,
        success,
    });

    // Extract and display final response
    let final_response = state
        .observations
        .iter()
        .rev()
        .find(|o| matches!(o.source, oco_shared_types::ObservationSource::LlmResponse))
        .and_then(|o| {
            if let oco_shared_types::ObservationKind::Text { content, .. } = &o.kind {
                Some(content.clone())
            } else {
                None
            }
        });

    if let Some(ref response) = final_response {
        r.emit(UiEvent::RunResponse {
            content: response.clone(),
        });
    }

    // Save run artifacts — non-fatal, warn on failure
    if let Err(e) = save_run_artifacts(
        &session_id,
        &state,
        &trace_events,
        run_duration,
        success,
        &final_response,
    ) {
        r.emit(UiEvent::Warning {
            message: format!("Failed to save run artifacts: {e}"),
        });
    }

    Ok(())
}

/// Save run artifacts (trace.jsonl, summary.json) to .oco/runs/<id>/
fn save_run_artifacts(
    session_id: &str,
    state: &oco_orchestrator_core::OrchestrationState,
    events: &[oco_shared_types::OrchestrationEvent],
    duration_ms: u64,
    success: bool,
    final_response: &Option<String>,
) -> Result<()> {
    // Write under workspace root when available (matches where `runs show/list` looks)
    let base = state
        .session
        .workspace_root
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let run_dir = base.join(".oco").join("runs").join(session_id);
    std::fs::create_dir_all(&run_dir)?;

    // trace.jsonl — one event per line
    {
        use std::io::Write;
        let mut f = std::fs::File::create(run_dir.join("trace.jsonl"))?;
        for event in events {
            writeln!(f, "{}", serde_json::to_string(event)?)?;
        }
    }

    // summary.json
    let summary = serde_json::json!({
        "session_id": session_id,
        "request": state.session.user_request,
        "workspace": state.session.workspace_root,
        "status": format!("{:?}", state.session.status),
        "complexity": format!("{:?}", state.task_complexity),
        "steps": state.session.step_count,
        "tokens_used": state.session.budget.tokens_used,
        "tokens_max": state.session.budget.max_total_tokens,
        "duration_ms": duration_ms,
        "success": success,
        "final_response": final_response,
        "created_at": state.session.created_at.to_rfc3339(),
        "external_session_id": state.session.external_session_id,
    });
    atomic_write(
        &run_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;

    Ok(())
}

/// Write to a temp file then rename — prevents corrupted files on crash.
fn atomic_write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).or_else(|_| {
        // rename can fail cross-device; fall back to copy+remove
        let r = std::fs::copy(&tmp, path).map(|_| ());
        let _ = std::fs::remove_file(&tmp); // always attempt cleanup
        r.map_err(anyhow::Error::from)
    })
}

fn cmd_index(r: &mut dyn Renderer, path: String) -> Result<()> {
    let ws_path = PathBuf::from(&path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&path));

    let tr = ui::terminal::TerminalRenderer::new();
    let spinner = tr.spinner(&format!("Indexing {}…", ws_path.display()));

    let t = Instant::now();
    let mut runtime = oco_orchestrator_core::OrchestratorRuntime::new(ws_path.clone());
    let result = runtime.index_workspace()?;
    let duration = t.elapsed().as_millis() as u64;

    spinner.finish_and_clear();

    r.emit(UiEvent::IndexCompleted {
        files: result.file_count,
        symbols: result.symbol_count,
        duration_ms: duration,
    });

    Ok(())
}

fn cmd_search(
    r: &mut dyn Renderer,
    format: OutputFormat,
    query: String,
    workspace: String,
    limit: u32,
) -> Result<()> {
    let ws_path = PathBuf::from(&workspace)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&workspace));

    let tr = ui::terminal::TerminalRenderer::new();
    let spinner = tr.spinner("Indexing & searching…");

    let mut runtime = oco_orchestrator_core::OrchestratorRuntime::new(ws_path);
    runtime.index_workspace()?;

    let results = runtime.search(&query, limit)?;
    spinner.finish_and_clear();

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string(&results)?);
        return Ok(());
    }

    if results.is_empty() {
        r.emit(UiEvent::SearchEmpty { query });
    } else {
        for (i, res) in results.iter().enumerate() {
            r.emit(UiEvent::SearchResult {
                rank: i + 1,
                path: res.path.clone(),
                score: res.score,
                snippet: res.snippet.clone(),
            });
        }
    }
    Ok(())
}

async fn cmd_status(r: &mut dyn Renderer, format: OutputFormat, url: String) -> Result<()> {
    let resp = reqwest::get(format!("{url}/api/v1/status")).await?;
    let body: serde_json::Value = resp.json().await?;
    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        r.emit(UiEvent::Info {
            message: format!(
                "Status: {}  Steps: {}  Tokens: {}",
                body["status"], body["steps"], body["tokens_used"]
            ),
        });
    }
    Ok(())
}

async fn cmd_trace(session_id: String, url: String) -> Result<()> {
    let resp = reqwest::get(format!("{url}/api/v1/sessions/{session_id}/trace")).await?;
    let body: serde_json::Value = resp.json().await?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

fn cmd_init(r: &mut dyn Renderer, output: String) -> Result<()> {
    let path = PathBuf::from(&output);
    if path.exists() {
        anyhow::bail!(
            "{output} already exists. Remove it first or use --output to specify a different path."
        );
    }
    let config = oco_orchestrator_core::OrchestratorConfig::default();
    let toml_str = config.to_toml()?;
    atomic_write(&path, toml_str)?;
    r.emit(UiEvent::Success {
        message: format!("Created {output} — edit to configure provider, budgets, etc."),
    });
    Ok(())
}

fn cmd_classify(format: OutputFormat, prompt: String, workspace: String) -> Result<()> {
    let ws_path = PathBuf::from(&workspace);

    let mut signals = Vec::new();
    if ws_path.join("Cargo.toml").exists() {
        signals.push("rust workspace".to_string());
    }
    if ws_path.join("package.json").exists() {
        signals.push("node workspace".to_string());
    }
    if ws_path.join("pyproject.toml").exists() {
        signals.push("python workspace".to_string());
    }

    let complexity = oco_policy_engine::TaskClassifier::classify(&prompt, &signals);

    let prompt_lower = prompt.to_lowercase();
    let task_type = if prompt_lower.contains("refactor") || prompt_lower.contains("rename") {
        "refactor"
    } else if prompt_lower.contains("bug")
        || prompt_lower.contains("fix")
        || prompt_lower.contains("debug")
    {
        "bugfix"
    } else if prompt_lower.contains("test") {
        "testing"
    } else if prompt_lower.contains("implement")
        || prompt_lower.contains("create")
        || prompt_lower.contains("add")
    {
        "feature"
    } else if prompt_lower.contains("explain")
        || prompt_lower.contains("what")
        || prompt_lower.contains("how")
    {
        "exploration"
    } else {
        "unknown"
    };

    let needs_verification = matches!(
        complexity,
        oco_shared_types::TaskComplexity::Medium
            | oco_shared_types::TaskComplexity::High
            | oco_shared_types::TaskComplexity::Critical
    ) && task_type != "exploration";

    let priority_files: Vec<String> = collect_priority_files(&ws_path, &prompt);

    let output = serde_json::json!({
        "complexity": complexity,
        "task_type": task_type,
        "needs_verification": needs_verification,
        "priority_files": priority_files,
        "workspace_signals": signals,
    });

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("Complexity: {:?}", complexity);
        println!("Task type: {task_type}");
        println!("Needs verification: {needs_verification}");
        if !priority_files.is_empty() {
            println!("Priority files: {}", priority_files.join(", "));
        }
    }
    Ok(())
}

fn cmd_gate_check(format: OutputFormat, tool: String, input: String, policy: String) -> Result<()> {
    let write_policy = match policy.as_str() {
        "allow_all" => oco_policy_engine::WritePolicy::AllowAll,
        "deny_destructive" => oco_policy_engine::WritePolicy::DenyDestructive,
        _ => oco_policy_engine::WritePolicy::RequireConfirmation,
    };

    let gate = oco_policy_engine::PolicyGate::new(write_policy);

    let input_json: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => {
            let output = serde_json::json!({
                "decision": "deny",
                "reason": format!("Invalid JSON input: {e}")
            });
            println!("{}", serde_json::to_string(&output)?);
            return Ok(());
        }
    };

    let decision = if tool.to_lowercase() == "bash" || tool.to_lowercase() == "shell" {
        let command = input_json
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if command.is_empty() {
            oco_shared_types::ToolGateDecision::Deny {
                reason: "shell tool called without a command field".to_string(),
            }
        } else {
            gate.evaluate_command(command)
        }
    } else {
        let tool_lower = tool.to_lowercase();
        let is_write = matches!(
            tool_lower.as_str(),
            "edit"
                | "write"
                | "file_write"
                | "file_delete"
                | "directory_delete"
                | "notebookedit"
                | "multiedit"
        );
        let is_destructive = matches!(
            tool_lower.as_str(),
            "file_delete" | "directory_delete" | "git_reset" | "git_force_push"
        );
        let known_read = matches!(
            tool_lower.as_str(),
            "read" | "glob" | "grep" | "bash" | "shell" | "web_search" | "web_fetch" | "list_files"
        );
        let requires_confirmation = is_destructive || (!is_write && !known_read);

        let descriptor = oco_shared_types::ToolDescriptor {
            name: tool.clone(),
            description: String::new(),
            input_schema: serde_json::json!({}),
            is_write: is_write || (!known_read && !is_destructive),
            requires_confirmation,
            timeout_secs: 30,
            tags: if is_destructive {
                vec!["destructive".to_string()]
            } else if is_write {
                vec!["write".to_string()]
            } else {
                vec!["read".to_string()]
            },
        };
        gate.evaluate(&descriptor)
    };

    let output = match &decision {
        oco_shared_types::ToolGateDecision::Allow => {
            serde_json::json!({"decision": "allow"})
        }
        oco_shared_types::ToolGateDecision::RequireConfirmation { reason } => {
            serde_json::json!({"decision": "confirm", "reason": reason})
        }
        oco_shared_types::ToolGateDecision::Deny { reason } => {
            serde_json::json!({"decision": "deny", "reason": reason})
        }
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        match &decision {
            oco_shared_types::ToolGateDecision::Allow => {
                println!("ALLOW: tool '{tool}' permitted");
            }
            oco_shared_types::ToolGateDecision::RequireConfirmation { reason } => {
                println!("CONFIRM: {reason}");
            }
            oco_shared_types::ToolGateDecision::Deny { reason } => {
                println!("DENY: {reason}");
            }
        }
    }
    Ok(())
}

fn cmd_observe(
    r: &mut dyn Renderer,
    format: OutputFormat,
    tool: String,
    status: String,
    output: Option<String>,
) -> Result<()> {
    let timestamp = chrono::Utc::now();

    let oco_dir = PathBuf::from(".oco");
    if !oco_dir.exists() {
        std::fs::create_dir_all(&oco_dir)?;
    }

    let observation = serde_json::json!({
        "timestamp": timestamp.to_rfc3339(),
        "tool": tool,
        "status": status,
        "output_preview": output.as_deref().map(|o| {
            o.chars().take(200).collect::<String>()
        }),
    });

    let log_path = oco_dir.join("observations.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(file, "{}", serde_json::to_string(&observation)?)?;

    if format == OutputFormat::Json {
        let result = serde_json::json!({
            "recorded": true,
            "timestamp": timestamp.to_rfc3339(),
            "tool": tool,
            "status": status,
            "log": log_path.display().to_string(),
        });
        println!("{}", serde_json::to_string(&result)?);
    } else {
        r.emit(UiEvent::Success {
            message: format!("Recorded: {tool} ({status}) at {}", timestamp.to_rfc3339()),
        });
    }
    Ok(())
}

async fn cmd_eval(
    r: &mut dyn Renderer,
    format: OutputFormat,
    scenarios: String,
    output: Option<String>,
    provider: String,
) -> Result<()> {
    let config =
        oco_orchestrator_core::OrchestratorConfig::load_from_dir(&std::env::current_dir()?);

    let base_llm: Arc<dyn oco_orchestrator_core::llm::LlmProvider> = match provider.as_str() {
        "claude-code" => Arc::new(oco_orchestrator_core::llm::ClaudeCodeProvider::new(
            oco_orchestrator_core::llm::ClaudeCodeConfig::new("sonnet"),
        )),
        "anthropic" => {
            let anthropic_config =
                oco_orchestrator_core::llm::AnthropicConfig::from_env(&config.llm.model, None)?;
            Arc::new(oco_orchestrator_core::llm::AnthropicProvider::new(
                anthropic_config,
            )?)
        }
        "ollama" => {
            let ollama_config = oco_orchestrator_core::llm::OllamaConfig::new(&config.llm.model);
            Arc::new(oco_orchestrator_core::llm::OllamaProvider::new(
                ollama_config,
            )?)
        }
        "stub" => Arc::new(oco_orchestrator_core::llm::StubLlmProvider {
            model: config.llm.model.clone(),
        }),
        other => {
            anyhow::bail!(
                "unknown provider '{other}'. Available: claude-code, anthropic, ollama, stub"
            );
        }
    };

    // Wrap with retry logic for rate-limit resilience.
    let llm: Arc<dyn oco_orchestrator_core::llm::LlmProvider> = Arc::new(
        oco_orchestrator_core::RetryingLlmProvider::new(base_llm, config.llm.max_retries),
    );

    let scenario_path = PathBuf::from(&scenarios);
    let loaded = oco_orchestrator_core::eval::load_scenarios(&scenario_path)?;

    r.emit(UiEvent::EvalStarted {
        scenario_count: loaded.len(),
    });

    let results = oco_orchestrator_core::eval::run_all(&loaded, llm, &config).await;
    let metrics = oco_orchestrator_core::eval::aggregate_metrics(&results);

    if let Some(ref output_path) = output {
        // Stable envelope: metrics for dashboards, results for debugging.
        let envelope = serde_json::json!({
            "metrics": &metrics,
            "results": &results,
        });
        let file = std::fs::File::create(output_path)?;
        serde_json::to_writer_pretty(file, &envelope)?;
        r.emit(UiEvent::EvalSaved {
            path: output_path.clone(),
        });
    } else if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&metrics)?);
    } else {
        for m in &metrics {
            r.emit(UiEvent::EvalScenario {
                name: m.scenario_name.clone(),
                success: m.success,
                steps: m.step_count,
                tokens: m.total_tokens,
                duration_ms: m.duration_ms,
                tokens_per_step: m.token_per_step,
            });
        }
    }
    Ok(())
}

fn cmd_doctor(r: &mut dyn Renderer, workspace: String) -> Result<()> {
    let ws_path = PathBuf::from(&workspace);

    r.emit(UiEvent::DoctorHeader {
        workspace: ws_path.display().to_string(),
    });

    let mut issues = 0u32;

    // Check oco.toml
    let config_path = ws_path.join("oco.toml");
    if config_path.exists() {
        match oco_orchestrator_core::OrchestratorConfig::from_file(&config_path) {
            Ok(_) => r.emit(UiEvent::DoctorCheck {
                name: "oco.toml".into(),
                status: CheckStatus::Pass,
                detail: Some("valid".into()),
            }),
            Err(e) => {
                r.emit(UiEvent::DoctorCheck {
                    name: "oco.toml".into(),
                    status: CheckStatus::Fail,
                    detail: Some(format!("parse error: {e}")),
                });
                issues += 1;
            }
        }
    } else {
        r.emit(UiEvent::DoctorCheck {
            name: "oco.toml".into(),
            status: CheckStatus::Warn,
            detail: Some("not found — using defaults".into()),
        });
    }

    // Check .oco directory
    let oco_dir = ws_path.join(".oco");
    if oco_dir.exists() {
        r.emit(UiEvent::DoctorCheck {
            name: ".oco/ directory".into(),
            status: CheckStatus::Pass,
            detail: None,
        });
        let db_path = oco_dir.join("index.db");
        if db_path.exists() {
            r.emit(UiEvent::DoctorCheck {
                name: "index.db".into(),
                status: CheckStatus::Pass,
                detail: None,
            });
        } else {
            r.emit(UiEvent::DoctorCheck {
                name: "index.db".into(),
                status: CheckStatus::Warn,
                detail: Some("not found — run `oco index .` first".into()),
            });
        }
    } else {
        r.emit(UiEvent::DoctorCheck {
            name: ".oco/ directory".into(),
            status: CheckStatus::Warn,
            detail: Some("workspace not indexed".into()),
        });
    }

    // Check .claude/ directory
    let claude_dir = ws_path.join(".claude");
    if claude_dir.exists() {
        r.emit(UiEvent::DoctorCheck {
            name: ".claude/ directory".into(),
            status: CheckStatus::Pass,
            detail: None,
        });

        // Check hooks
        let hooks_dir = claude_dir.join("hooks");
        if hooks_dir.exists() {
            let hook_files = [
                "pre-tool-use.mjs",
                "post-tool-use.mjs",
                "user-prompt-submit.cjs",
                "stop.mjs",
            ];
            for hook in &hook_files {
                if hooks_dir.join(hook).exists() {
                    r.emit(UiEvent::DoctorCheck {
                        name: format!("hook {hook}"),
                        status: CheckStatus::Pass,
                        detail: None,
                    });
                } else {
                    r.emit(UiEvent::DoctorCheck {
                        name: format!("hook {hook}"),
                        status: CheckStatus::Warn,
                        detail: Some("missing".into()),
                    });
                }
            }
        } else {
            r.emit(UiEvent::DoctorCheck {
                name: ".claude/hooks/".into(),
                status: CheckStatus::Warn,
                detail: Some("not found".into()),
            });
        }

        // Check settings.json
        let settings = claude_dir.join("settings.json");
        if settings.exists() {
            match std::fs::read_to_string(&settings)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                Some(v) => {
                    if v.get("hooks").is_some() {
                        r.emit(UiEvent::DoctorCheck {
                            name: "settings.json hooks".into(),
                            status: CheckStatus::Pass,
                            detail: None,
                        });
                    } else {
                        r.emit(UiEvent::DoctorCheck {
                            name: "settings.json hooks".into(),
                            status: CheckStatus::Warn,
                            detail: Some("missing hooks section".into()),
                        });
                    }
                    if v.get("mcpServers").is_some() {
                        r.emit(UiEvent::DoctorCheck {
                            name: "settings.json MCP servers".into(),
                            status: CheckStatus::Pass,
                            detail: None,
                        });
                    } else {
                        r.emit(UiEvent::DoctorCheck {
                            name: "settings.json MCP servers".into(),
                            status: CheckStatus::Warn,
                            detail: Some("missing MCP servers".into()),
                        });
                    }
                }
                None => {
                    r.emit(UiEvent::DoctorCheck {
                        name: "settings.json".into(),
                        status: CheckStatus::Fail,
                        detail: Some("invalid JSON".into()),
                    });
                    issues += 1;
                }
            }
        } else {
            r.emit(UiEvent::DoctorCheck {
                name: ".claude/settings.json".into(),
                status: CheckStatus::Fail,
                detail: Some("not found".into()),
            });
            issues += 1;
        }

        // Check skills
        let skills_dir = claude_dir.join("skills");
        if skills_dir.exists() {
            let count = std::fs::read_dir(&skills_dir)
                .map(|entries| entries.filter_map(|e| e.ok()).count())
                .unwrap_or(0);
            r.emit(UiEvent::DoctorCheck {
                name: "skills".into(),
                status: CheckStatus::Pass,
                detail: Some(format!("{count} found")),
            });
        }
    } else {
        r.emit(UiEvent::DoctorCheck {
            name: ".claude/ directory".into(),
            status: CheckStatus::Warn,
            detail: Some("not a Claude Code plugin".into()),
        });
    }

    // Detect repo profile
    let profile = oco_shared_types::RepoProfile::detect(&ws_path);
    r.emit(UiEvent::DoctorProfile {
        stack: profile.stack.clone(),
        build_cmd: profile.build_command.clone(),
        test_cmd: profile.test_command.clone(),
    });

    r.emit(UiEvent::DoctorSummary { issues });
    Ok(())
}

fn cmd_runs_show(r: &mut dyn Renderer, id: String, workspace: String) -> Result<()> {
    let runs_dir = PathBuf::from(&workspace).join(".oco").join("runs");

    let run_dir = if id == "last" {
        // Find most recent run by directory mtime
        let mut entries: Vec<_> = std::fs::read_dir(&runs_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(|e| {
            std::cmp::Reverse(
                e.metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
        });
        entries
            .first()
            .map(|e| e.path())
            .ok_or_else(|| anyhow::anyhow!("no runs found in {}", runs_dir.display()))?
    } else {
        runs_dir.join(&id)
    };

    if !run_dir.exists() {
        anyhow::bail!("run {} not found", run_dir.display());
    }

    // Read summary
    let summary_path = run_dir.join("summary.json");
    if summary_path.exists() {
        let summary: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&summary_path)?)?;

        r.emit(UiEvent::RunStarted {
            provider: String::new(),
            model: String::new(),
            request: summary["request"].as_str().unwrap_or("").to_string(),
            workspace: summary["workspace"].as_str().map(|s| s.to_string()),
        });

        r.emit(UiEvent::RunFinished {
            session_id: summary["session_id"].as_str().unwrap_or("").to_string(),
            steps: summary["steps"].as_u64().unwrap_or(0) as u32,
            tokens_used: summary["tokens_used"].as_u64().unwrap_or(0),
            tokens_max: summary["tokens_max"].as_u64().unwrap_or(0),
            duration_ms: summary["duration_ms"].as_u64().unwrap_or(0),
            success: summary["success"].as_bool().unwrap_or(false),
        });

        if let Some(resp) = summary["final_response"].as_str() {
            r.emit(UiEvent::RunResponse {
                content: resp.to_string(),
            });
        }
    }

    // Read and replay trace events
    let trace_path = run_dir.join("trace.jsonl");
    if trace_path.exists() {
        r.emit(UiEvent::Info {
            message: format!("\nTrace: {}", trace_path.display()),
        });
        let content = std::fs::read_to_string(&trace_path)?;
        for line in content.lines() {
            if let Ok(oco_shared_types::OrchestrationEvent::StepCompleted {
                step,
                ref action,
                ref reason,
                duration_ms,
                ref budget_snapshot,
                ..
            }) = serde_json::from_str::<oco_shared_types::OrchestrationEvent>(line)
            {
                let action_type = match action {
                    oco_shared_types::OrchestratorAction::Respond { .. } => "RESPOND",
                    oco_shared_types::OrchestratorAction::Retrieve { .. } => "RETRIEVE",
                    oco_shared_types::OrchestratorAction::ToolCall { .. } => "TOOL_CALL",
                    oco_shared_types::OrchestratorAction::Verify { .. } => "VERIFY",
                    oco_shared_types::OrchestratorAction::UpdateMemory { .. } => "MEMORY",
                    oco_shared_types::OrchestratorAction::Stop { .. } => "STOP",
                    oco_shared_types::OrchestratorAction::Plan { .. } => "PLAN",
                    oco_shared_types::OrchestratorAction::Delegate { .. } => "DELEGATE",
                    oco_shared_types::OrchestratorAction::Message { .. } => "MESSAGE",
                    oco_shared_types::OrchestratorAction::Replan { .. } => "REPLAN",
                };
                r.emit(UiEvent::RunStepCompleted {
                    step,
                    action_type: action_type.to_string(),
                    reason: reason.clone(),
                    tokens_used: budget_snapshot.tokens_used,
                    tokens_max: budget_snapshot.tokens_used + budget_snapshot.tokens_remaining,
                    duration_ms,
                });
            }
        }
    }

    Ok(())
}

fn cmd_runs_list(r: &mut dyn Renderer, workspace: String, limit: usize) -> Result<()> {
    let runs_dir = PathBuf::from(&workspace).join(".oco").join("runs");
    if !runs_dir.exists() {
        r.emit(UiEvent::Info {
            message: "No runs found.".into(),
        });
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&runs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| {
        std::cmp::Reverse(
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        )
    });

    for entry in entries.into_iter().take(limit) {
        let summary_path = entry.path().join("summary.json");
        if let Ok(content) = std::fs::read_to_string(&summary_path)
            && let Ok(summary) = serde_json::from_str::<serde_json::Value>(&content)
        {
            let id = summary["session_id"].as_str().unwrap_or("?");
            let request = summary["request"].as_str().unwrap_or("?");
            let steps = summary["steps"].as_u64().unwrap_or(0);
            let success = summary["success"].as_bool().unwrap_or(false);
            let duration = summary["duration_ms"].as_u64().unwrap_or(0);

            let status = if success { "ok" } else { "fail" };
            let req_display = if request.chars().count() > 50 {
                let prefix: String = request.chars().take(49).collect();
                format!("{prefix}…")
            } else {
                request.to_string()
            };
            r.emit(UiEvent::Info {
                message: format!(
                    "  {:<8} {:<5} {:>3} steps  {:>6}ms  {}",
                    &id[..8.min(id.len())],
                    status,
                    steps,
                    duration,
                    req_display,
                ),
            });
        }
    }

    Ok(())
}

/// Collect priority files by scanning for files matching keywords in the prompt.
fn collect_priority_files(workspace: &std::path::Path, prompt: &str) -> Vec<String> {
    let mut files = Vec::new();
    let prompt_lower = prompt.to_lowercase();

    let words: Vec<&str> = prompt_lower.split_whitespace().collect();

    let src_dir = workspace.join("src");
    if src_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&src_dir)
    {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let stem = name.rsplit('.').next_back().unwrap_or(&name);
            if words.iter().any(|w| w.len() > 3 && stem.contains(w)) {
                files.push(
                    entry
                        .path()
                        .strip_prefix(workspace)
                        .unwrap_or(&entry.path())
                        .display()
                        .to_string(),
                );
            }
        }
    }

    files.truncate(5);
    files
}
