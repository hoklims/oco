mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use ui::{CheckStatus, OutputFormat, Renderer, UiEvent, UiStepSummary};

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
        /// Run without the dashboard UI (API/MCP only)
        #[arg(long)]
        headless: bool,
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
        /// Resume from a previous run's mission memory (session ID or "last")
        #[arg(long)]
        resume: Option<String>,
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
    /// Compare two evaluation result files (Q5 scorecard comparison)
    EvalCompare {
        /// Baseline results file (JSON from `oco eval --output`)
        baseline: String,
        /// Candidate results file (JSON from `oco eval --output`)
        candidate: String,
        /// Output as JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Run a quality gate: compare candidate against baseline with pass/warn/fail verdict (Q6/Q7)
    ///
    /// Q7: When baseline or policy are omitted, reads [gate] config from oco.toml.
    EvalGate {
        /// Baseline file (omit to use repo config's baseline_path)
        baseline: Option<String>,
        /// Candidate file (scorecard.json, eval results JSON, or a run directory)
        candidate: Option<String>,
        /// Gate policy: strict, balanced, lenient (omit to use repo config)
        #[arg(long)]
        policy: Option<String>,
        /// Output full result as JSON
        #[arg(long)]
        json: bool,
        /// Write a review report (Markdown + JSON) to a directory
        #[arg(long)]
        report: Option<String>,
        /// Workspace path (to find oco.toml and .oco/)
        #[arg(long, default_value = ".")]
        workspace: String,
    },
    /// Save a baseline from a run's scorecard or eval results (Q6/Q7)
    ///
    /// Q7: When --output is omitted, uses the baseline_path from repo config.
    BaselineSave {
        /// Source: run ID ("last"), eval results file, or scorecard.json path
        source: String,
        /// Baseline name (e.g., "v0.5-stable")
        #[arg(long)]
        name: String,
        /// Output path (omit to use repo config's baseline_path)
        #[arg(long)]
        output: Option<String>,
        /// Workspace path (for resolving run IDs and repo config)
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Promote a candidate to become the new baseline (Q11)
    ///
    /// Loads the current baseline, evaluates the candidate against it,
    /// backs up the old baseline, saves the new one, and appends an
    /// audit trail entry to `.oco/baseline-history.json`.
    ///
    /// If the gate verdict is Fail (recommendation: reject), the promotion
    /// is aborted unless --force is provided.
    BaselinePromote {
        /// Source: run ID ("last"), eval results file, or scorecard.json path
        source: String,
        /// Name for the new baseline (e.g., "v2-stable")
        #[arg(long)]
        name: String,
        /// Reason for the promotion (shown in audit trail)
        #[arg(long)]
        reason: Option<String>,
        /// Optional description for the new baseline
        #[arg(long)]
        description: Option<String>,
        /// Workspace path (for resolving run IDs and repo config)
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Output the promotion record as JSON
        #[arg(long)]
        json: bool,
        /// Force promotion even when recommendation is reject
        #[arg(long)]
        force: bool,
    },
    /// Show the baseline promotion audit trail (Q11)
    BaselineHistory {
        /// Workspace path
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Output as Markdown
        #[arg(long)]
        markdown: bool,
        /// Maximum entries to show (most recent first)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Prune old entries from the baseline history, keeping only the most recent N
    BaselineHistoryPrune {
        /// Number of entries to keep (most recent)
        #[arg(long)]
        keep: usize,
        /// Workspace path
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Preview what would be removed without modifying anything
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
        /// Show mission memory (durable handoff artifact) instead of trace
        #[arg(long)]
        mission: bool,
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
    /// Display a run's mission memory as a handoff document
    Handoff {
        /// Run/session ID (or "last" for most recent)
        id: String,
        /// Workspace path (to find .oco/runs/)
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Output as JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Generate a unified review packet for a run (Q9 merge-readiness bundle)
    ReviewPack {
        /// Run/session ID (or "last" for most recent)
        id: String,
        /// Workspace path (to find .oco/runs/ and oco.toml)
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Output full packet as JSON
        #[arg(long)]
        json: bool,
        /// Output as Markdown
        #[arg(long)]
        markdown: bool,
        /// Write review packet files (relative paths resolve from --workspace)
        #[arg(long)]
        save: Option<Option<String>>,
    },
    /// Compare two runs' scorecards (Q5 regression detection)
    Compare {
        /// First run ID (or "last" for most recent)
        baseline: String,
        /// Second run ID
        candidate: String,
        /// Workspace path (to find .oco/runs/)
        #[arg(long, default_value = ".")]
        workspace: String,
        /// Output as JSON instead of text
        #[arg(long)]
        json: bool,
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
        Commands::Serve {
            host,
            port,
            headless,
        } => cmd_serve(&mut *r, host, port, headless).await?,
        Commands::Run {
            request,
            workspace,
            provider,
            model,
            max_steps: _,
            resume,
        } => {
            cmd_run(
                &mut *r, out_format, request, workspace, provider, model, resume,
            )
            .await?
        }
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
        Commands::EvalCompare {
            baseline,
            candidate,
            json,
        } => cmd_eval_compare(&mut *r, baseline, candidate, json)?,
        Commands::EvalGate {
            baseline,
            candidate,
            policy,
            json,
            report,
            workspace,
        } => {
            let exit_code = cmd_eval_gate(
                &mut *r, baseline, candidate, policy, json, report, workspace,
            )?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Commands::BaselineSave {
            source,
            name,
            output,
            workspace,
            description,
        } => cmd_baseline_save(&mut *r, source, name, output, workspace, description)?,
        Commands::BaselinePromote {
            source,
            name,
            reason,
            description,
            workspace,
            json,
            force,
        } => cmd_baseline_promote(
            &mut *r,
            source,
            name,
            reason,
            description,
            workspace,
            json,
            force,
        )?,
        Commands::BaselineHistory {
            workspace,
            json,
            markdown,
            limit,
        } => cmd_baseline_history(&mut *r, workspace, json, markdown, limit)?,
        Commands::BaselineHistoryPrune {
            keep,
            workspace,
            dry_run,
            json,
        } => cmd_baseline_history_prune(&mut *r, workspace, keep, dry_run, json)?,
        Commands::Doctor { workspace } => cmd_doctor(&mut *r, workspace)?,
        Commands::Runs { action } => match action {
            RunsAction::Show {
                id,
                workspace,
                mission,
            } => {
                if mission {
                    cmd_runs_handoff(&mut *r, id, workspace, false)?;
                } else {
                    cmd_runs_show(&mut *r, id, workspace)?;
                }
            }
            RunsAction::List { workspace, limit } => cmd_runs_list(&mut *r, workspace, limit)?,
            RunsAction::Handoff {
                id,
                workspace,
                json,
            } => cmd_runs_handoff(&mut *r, id, workspace, json)?,
            RunsAction::ReviewPack {
                id,
                workspace,
                json,
                markdown,
                save,
            } => cmd_runs_review_pack(&mut *r, id, workspace, json, markdown, save)?,
            RunsAction::Compare {
                baseline,
                candidate,
                workspace,
                json,
            } => cmd_runs_compare(&mut *r, baseline, candidate, workspace, json)?,
        },
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════
// Command implementations
// ═══════════════════════════════════════════════════════════

async fn cmd_serve(r: &mut dyn Renderer, host: String, port: u16, headless: bool) -> Result<()> {
    let mut config =
        oco_orchestrator_core::OrchestratorConfig::load_from_dir(&std::env::current_dir()?);
    config.bind_address = host;
    config.port = port;

    let mut server = oco_mcp_server::McpServer::new(config);

    // Dashboard: skip entirely in headless mode.
    let has_dashboard = if headless {
        false
    } else {
        let dashboard_candidates = [
            PathBuf::from("apps/dashboard/dist"),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("dashboard")))
                .unwrap_or_default(),
        ];
        if let Some(dir) = std::env::var("OCO_DASHBOARD_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                dashboard_candidates
                    .iter()
                    .find(|d| d.join("index.html").exists())
                    .cloned()
            })
        {
            server = server.with_dashboard_dir(dir);
            true
        } else {
            false
        }
    };

    // Bind first to get the real port (especially when --port 0).
    let (listener, app) = server.bind().await?;
    let real_addr = listener.local_addr()?;

    r.emit(UiEvent::ServerListening {
        host: real_addr.ip().to_string(),
        port: real_addr.port(),
    });

    if headless {
        eprintln!("Running in headless mode (API/MCP only, no dashboard)");
    } else if has_dashboard {
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
    resume: Option<String>,
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

    let run_profile = config.profile.clone();
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

    // Load and apply mission memory from a previous run if --resume was specified.
    if let Some(ref resume_id) = resume {
        let ws_for_resume = workspace.as_deref().unwrap_or(".");
        let run_dir = resolve_run_dir(resume_id, ws_for_resume)?;
        let mission_path = run_dir.join("mission.json");
        if !mission_path.exists() {
            anyhow::bail!(
                "no mission.json in run {}. Cannot resume without a mission memory.",
                run_dir.display()
            );
        }
        let mission = oco_shared_types::MissionMemory::load_from(&mission_path)
            .map_err(|e| anyhow::anyhow!("failed to load mission memory: {e}"))?;
        r.emit(UiEvent::Info {
            message: format!(
                "Resuming from session {} ({} facts, {} hypotheses, {} questions)",
                mission.session_id.0,
                mission.facts.len(),
                mission.hypotheses.len(),
                mission.open_questions.len(),
            ),
        });
        orchestrator.with_resume_mission(mission);
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
            oco_shared_types::OrchestrationEvent::PlanGenerated {
                plan_id,
                step_count,
                parallel_group_count,
                critical_path_length,
                estimated_total_tokens,
                ref strategy,
                ref team,
                ref steps,
            } => {
                r.emit(UiEvent::PlanOverview {
                    step_count,
                    parallel_groups: parallel_group_count,
                    critical_path_length,
                    estimated_tokens: estimated_total_tokens as u32,
                    budget_tokens: 0, // not available here
                    strategy: strategy.clone(),
                    team: team
                        .as_ref()
                        .map(|t| (t.name.clone(), t.topology.clone(), t.member_count)),
                    steps: steps
                        .iter()
                        .map(|s| UiStepSummary {
                            id: s.id,
                            name: s.name.clone(),
                            role: s.role.clone(),
                            execution_mode: s.execution_mode.clone(),
                            depends_on: s.depends_on.clone(),
                            verify_after: s.verify_after,
                            estimated_tokens: s.estimated_tokens,
                            preferred_model: s.preferred_model.clone(),
                        })
                        .collect(),
                });
                let _ = plan_id; // referenced in trace_events
            }
            oco_shared_types::OrchestrationEvent::PlanStepStarted {
                ref step_name,
                ref role,
                ref execution_mode,
                ..
            } => {
                r.emit(UiEvent::PlanStepStarted {
                    step_name: step_name.clone(),
                    role: role.clone(),
                    execution_mode: execution_mode.clone(),
                });
            }
            oco_shared_types::OrchestrationEvent::PlanStepCompleted {
                ref step_name,
                success,
                duration_ms,
                tokens_used,
                ..
            } => {
                r.emit(UiEvent::PlanStepCompleted {
                    step_name: step_name.clone(),
                    success,
                    duration_ms,
                    tokens_used,
                });
            }
            oco_shared_types::OrchestrationEvent::PlanProgress {
                completed,
                total,
                ref active_steps,
                budget_used_pct,
                ..
            } => {
                r.emit(UiEvent::PlanProgress {
                    completed,
                    total,
                    active_steps: active_steps.iter().map(|(_, name)| name.clone()).collect(),
                    budget_used_pct,
                });
            }
            oco_shared_types::OrchestrationEvent::VerifyGateResult {
                ref step_name,
                ref checks,
                overall_passed,
                replan_triggered,
                ..
            } => {
                r.emit(UiEvent::PlanVerifyGateResult {
                    step_name: step_name.clone(),
                    checks: checks
                        .iter()
                        .map(|c| (c.check_type.clone(), c.passed, c.summary.clone()))
                        .collect(),
                    overall_passed,
                    replan_triggered,
                });
            }
            oco_shared_types::OrchestrationEvent::ReplanTriggered {
                ref failed_step_name,
                attempt,
                max_attempts,
                steps_preserved,
                steps_removed,
                steps_added,
            } => {
                r.emit(UiEvent::PlanReplanTriggered {
                    failed_step: failed_step_name.clone(),
                    attempt,
                    max_attempts,
                    steps_preserved,
                    steps_removed,
                    steps_added,
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
        &run_profile,
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

/// Save run artifacts (trace.jsonl, summary.json, mission.json) to .oco/runs/<id>/
fn save_run_artifacts(
    session_id: &str,
    state: &oco_orchestrator_core::OrchestrationState,
    profile: &oco_shared_types::RepoProfile,
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

    // mission.json — durable mission memory for handoff/resume
    let mission = state.create_mission_memory(profile);
    let has_mission_content = mission.has_content();
    if has_mission_content {
        mission
            .save_to(&run_dir.join("mission.json"))
            .map_err(|e| anyhow::anyhow!("failed to save mission memory: {e}"))?;
    }

    // scorecard.json — Q5 evaluation scorecard via canonical ScorecardBuilder
    {
        let replan_count = state
            .memory
            .planner_state
            .as_ref()
            .map(|ps| ps.replan_count)
            .unwrap_or(0);

        let verified_count = state
            .verification
            .runs
            .iter()
            .filter(|r| r.passed)
            .flat_map(|r| r.covered_files.iter())
            .collect::<std::collections::HashSet<_>>()
            .len();

        let error_count = state
            .observations
            .iter()
            .filter(|o| matches!(o.kind, oco_shared_types::ObservationKind::Error { .. }))
            .count();

        // Load per-repo scorecard weight overrides from oco.toml (if present).
        let weights = oco_orchestrator_core::load_scorecard_weights(&base).unwrap_or_default();

        let scorecard = oco_orchestrator_core::ScorecardBuilder::new(session_id)
            .success(success)
            .trust_verdict(mission.trust_verdict)
            .file_counts(state.verification.modified_files.len(), verified_count)
            .mission_continuity(has_mission_content)
            .cost(
                state.session.budget.tokens_used,
                state.session.step_count,
                duration_ms,
                state.session.budget.tool_calls_used,
                state.session.budget.verify_cycles_used,
            )
            .replans(replan_count)
            .errors(error_count, state.session.step_count)
            .with_weights(weights)
            .build();

        atomic_write(
            &run_dir.join("scorecard.json"),
            serde_json::to_string_pretty(&scorecard)?,
        )?;
    }

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

    // Check oco.toml — capture result for downstream gate check
    let config_path = ws_path.join("oco.toml");
    let parsed_config = if config_path.exists() {
        match oco_orchestrator_core::OrchestratorConfig::from_file(&config_path) {
            Ok(cfg) => {
                r.emit(UiEvent::DoctorCheck {
                    name: "oco.toml".into(),
                    status: CheckStatus::Pass,
                    detail: Some("valid".into()),
                });
                Some(cfg)
            }
            Err(e) => {
                r.emit(UiEvent::DoctorCheck {
                    name: "oco.toml".into(),
                    status: CheckStatus::Fail,
                    detail: Some(format!("parse error: {e}")),
                });
                issues += 1;
                None
            }
        }
    } else {
        r.emit(UiEvent::DoctorCheck {
            name: "oco.toml".into(),
            status: CheckStatus::Warn,
            detail: Some("not found — using defaults".into()),
        });
        None
    };

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

    // Q7: Check gate configuration — uses parsed config when available,
    // defaults when no file exists, and skips when oco.toml was invalid
    // (to avoid contradictory "oco.toml: FAIL" + "gate config: PASS").
    {
        let gate = if let Some(ref cfg) = parsed_config {
            cfg.gate.clone()
        } else if config_path.exists() {
            // oco.toml exists but failed to parse — signal the inconsistency
            r.emit(UiEvent::DoctorCheck {
                name: "gate config".into(),
                status: CheckStatus::Fail,
                detail: Some("skipped — oco.toml is invalid".into()),
            });
            issues += 1;
            // Skip baseline check too
            oco_shared_types::GateConfig::default() // for the let-binding; we already emitted
        } else {
            oco_shared_types::GateConfig::default()
        };

        // Only run the detailed gate checks if oco.toml was absent or valid
        if !config_path.exists() || parsed_config.is_some() {
            match gate.validate() {
                Ok(()) => {
                    r.emit(UiEvent::DoctorCheck {
                        name: "gate config".into(),
                        status: CheckStatus::Pass,
                        detail: Some(format!(
                            "policy={}, baseline={}",
                            gate.default_policy, gate.baseline_path
                        )),
                    });
                }
                Err(e) => {
                    r.emit(UiEvent::DoctorCheck {
                        name: "gate config".into(),
                        status: CheckStatus::Fail,
                        detail: Some(format!("invalid: {e}")),
                    });
                    issues += 1;
                }
            }
            let baseline_full = ws_path.join(&gate.baseline_path);
            if baseline_full.exists() {
                r.emit(UiEvent::DoctorCheck {
                    name: "gate baseline".into(),
                    status: CheckStatus::Pass,
                    detail: Some(gate.baseline_path.clone()),
                });
            } else {
                r.emit(UiEvent::DoctorCheck {
                    name: "gate baseline".into(),
                    status: CheckStatus::Warn,
                    detail: Some(format!(
                        "{} not found — run `oco baseline-save` first",
                        gate.baseline_path
                    )),
                });
            }
        }
    }

    // Detect repo profile
    let profile = oco_shared_types::RepoProfile::detect(&ws_path);
    r.emit(UiEvent::DoctorProfile {
        stack: profile.stack.clone(),
        build_cmd: profile.build_command.clone(),
        test_cmd: profile.test_command.clone(),
    });

    // Claude Code Integration
    r.emit(UiEvent::DoctorHeader {
        workspace: "Claude Code Integration".into(),
    });

    let claude_caps = oco_claude_adapter::ClaudeCapabilities::load_cached(&ws_path)
        .unwrap_or_else(oco_claude_adapter::ClaudeCapabilities::none);

    for check in claude_caps.doctor_report() {
        let status = match check.status {
            oco_claude_adapter::DoctorStatus::Pass => CheckStatus::Pass,
            oco_claude_adapter::DoctorStatus::Warn => CheckStatus::Warn,
            oco_claude_adapter::DoctorStatus::Fail => CheckStatus::Fail,
        };
        r.emit(UiEvent::DoctorCheck {
            name: check.name,
            status,
            detail: Some(check.detail),
        });
    }

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

/// Resolve a run directory from an ID (or "last") and a workspace path.
fn resolve_run_dir(id: &str, workspace: &str) -> Result<PathBuf> {
    let runs_dir = PathBuf::from(workspace).join(".oco").join("runs");

    if id == "last" {
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
            .ok_or_else(|| anyhow::anyhow!("no runs found in {}", runs_dir.display()))
    } else {
        let dir = runs_dir.join(id);
        if !dir.exists() {
            anyhow::bail!("run {} not found", dir.display());
        }
        Ok(dir)
    }
}

/// Display a run's mission memory as a handoff document.
fn cmd_runs_handoff(
    r: &mut dyn Renderer,
    id: String,
    workspace: String,
    json_output: bool,
) -> Result<()> {
    let run_dir = resolve_run_dir(&id, &workspace)?;
    let mission_path = run_dir.join("mission.json");

    if !mission_path.exists() {
        r.emit(UiEvent::Info {
            message: format!(
                "No mission memory found for this run.\n  Expected: {}",
                mission_path.display()
            ),
        });
        return Ok(());
    }

    let mission = oco_shared_types::MissionMemory::load_from(&mission_path)
        .map_err(|e| anyhow::anyhow!("failed to load mission memory: {e}"))?;

    if json_output {
        let json = serde_json::to_string_pretty(&mission)?;
        r.emit(UiEvent::Info { message: json });
    } else {
        r.emit(UiEvent::Info {
            message: mission.to_handoff_text(),
        });
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════
// Q5: Scorecard comparison commands
// ═══════════════════════════════════════════════════════════

/// Compare two eval result files and show regression analysis.
fn cmd_eval_compare(
    r: &mut dyn Renderer,
    baseline_path: String,
    candidate_path: String,
    json_output: bool,
) -> Result<()> {
    use oco_shared_types::{BatchComparison, ScenarioResult};

    // Load baseline results
    let baseline_content = std::fs::read_to_string(&baseline_path)
        .map_err(|e| anyhow::anyhow!("failed to read baseline '{}': {e}", baseline_path))?;
    let baseline_envelope: serde_json::Value = serde_json::from_str(&baseline_content)?;
    let baseline_results: Vec<ScenarioResult> =
        serde_json::from_value(baseline_envelope["results"].clone())
            .map_err(|e| anyhow::anyhow!("failed to parse baseline results: {e}"))?;

    // Load candidate results
    let candidate_content = std::fs::read_to_string(&candidate_path)
        .map_err(|e| anyhow::anyhow!("failed to read candidate '{}': {e}", candidate_path))?;
    let candidate_envelope: serde_json::Value = serde_json::from_str(&candidate_content)?;
    let candidate_results: Vec<ScenarioResult> =
        serde_json::from_value(candidate_envelope["results"].clone())
            .map_err(|e| anyhow::anyhow!("failed to parse candidate results: {e}"))?;

    // Build scorecards from results (no workspace context → default weights)
    let default_weights = oco_shared_types::ScorecardWeights::default();
    let baseline_scorecards: Vec<oco_shared_types::RunScorecard> = baseline_results
        .iter()
        .map(|sr| scorecard_from_scenario_result(sr, &default_weights))
        .collect();
    let candidate_scorecards: Vec<oco_shared_types::RunScorecard> = candidate_results
        .iter()
        .map(|sr| scorecard_from_scenario_result(sr, &default_weights))
        .collect();

    let batch = BatchComparison::from_paired(&baseline_scorecards, &candidate_scorecards);

    if json_output {
        let json = serde_json::to_string_pretty(&batch)?;
        r.emit(UiEvent::Info { message: json });
    } else {
        r.emit(UiEvent::Info {
            message: format!("Eval Comparison: {} vs {}", baseline_path, candidate_path),
        });
        r.emit(UiEvent::Info {
            message: format!(
                "  {} scenario(s) compared: {} improved, {} stable, {} regressed",
                batch.total_scenarios,
                batch.improved_count,
                batch.stable_count,
                batch.regressed_count,
            ),
        });

        for cmp in &batch.comparisons {
            r.emit(UiEvent::ComparisonResult {
                baseline_id: cmp.baseline_id.clone(),
                candidate_id: cmp.candidate_id.clone(),
                overall_delta: cmp.overall_delta,
                regressions: cmp.regressions.len(),
                improvements: cmp.improvements.len(),
                verdict: cmp.verdict.label().to_string(),
            });

            for reg in &cmp.regressions {
                r.emit(UiEvent::ComparisonDetail {
                    dimension: reg.dimension.label().to_string(),
                    baseline_score: reg.baseline_score,
                    candidate_score: reg.candidate_score,
                    delta: reg.delta,
                    kind: "regression".to_string(),
                });
            }
            for imp in &cmp.improvements {
                r.emit(UiEvent::ComparisonDetail {
                    dimension: imp.dimension.label().to_string(),
                    baseline_score: imp.baseline_score,
                    candidate_score: imp.candidate_score,
                    delta: imp.delta,
                    kind: "improvement".to_string(),
                });
            }
        }

        // Overall verdict
        let verdict_str = batch.overall_verdict.label();
        let symbol = batch.overall_verdict.symbol();
        r.emit(UiEvent::Info {
            message: format!("\n  Overall: {symbol} {verdict_str}"),
        });
    }

    Ok(())
}

/// Load both gate and review config from `oco.toml` in one pass.
///
/// Avoids double-parsing and ensures that a broken `[review]` section is not
/// mis-attributed to the gate config (or vice versa).  Returns defaults for
/// both sections when no config file exists.
fn load_review_pack_config(
    ws: &Path,
) -> Result<(
    oco_shared_types::GateConfig,
    oco_shared_types::ReviewConfig,
    oco_shared_types::ScorecardWeights,
)> {
    let config_path = ws.join("oco.toml");
    if !config_path.exists() {
        return Ok((
            oco_shared_types::GateConfig::default(),
            oco_shared_types::ReviewConfig::default(),
            oco_shared_types::ScorecardWeights::default(),
        ));
    }
    let config = oco_orchestrator_core::OrchestratorConfig::from_file(&config_path)
        .map_err(|e| anyhow::anyhow!("cannot load oco.toml: {e}"))?;
    Ok((config.gate, config.review, config.scorecard))
}

/// Generate a unified review packet for a run (Q9).
fn cmd_runs_review_pack(
    r: &mut dyn Renderer,
    id: String,
    workspace: String,
    json_output: bool,
    markdown_output: bool,
    save: Option<Option<String>>,
) -> Result<()> {
    let run_dir = resolve_run_dir(&id, &workspace)?;
    let run_id = run_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| id.clone());

    let ws = Path::new(&workspace);
    let (gate_cfg, review_cfg, scorecard_weights) = load_review_pack_config(ws)?;

    let packet = oco_orchestrator_core::build_review_packet_with_config(
        &run_dir,
        &run_id,
        &gate_cfg,
        ws,
        &scorecard_weights,
        &review_cfg.merge_readiness,
    )?;

    // Resolve output format: CLI flags override config default_format.
    let use_json = json_output || (!markdown_output && review_cfg.default_format == "json");
    let use_markdown = !use_json && (markdown_output || review_cfg.default_format == "markdown");

    if use_json {
        let json = packet.to_json().map_err(|e| anyhow::anyhow!("{e}"))?;
        r.emit(UiEvent::Info { message: json });
    } else if use_markdown {
        r.emit(UiEvent::Info {
            message: packet.to_markdown(),
        });
    } else {
        // Structured terminal output via UiEvents
        r.emit(UiEvent::ReviewPacketHeader {
            run_id: packet.run_id.clone(),
            merge_readiness: packet.merge_readiness.label().to_string(),
            trust_verdict: packet.trust_verdict.map(|tv| tv.label().to_string()),
            gate_verdict: packet.gate_verdict.map(|gv| gv.label().to_string()),
        });

        if let Some(ref sc) = packet.scorecard {
            r.emit(UiEvent::ReviewPacketScorecard {
                overall_score: sc.overall_score,
                dimensions: sc
                    .dimensions
                    .iter()
                    .map(|d| (d.dimension.label().to_string(), d.score))
                    .collect(),
            });
        }

        r.emit(UiEvent::ReviewPacketChanges {
            modified_files: packet.changes.modified_files.clone(),
            key_decisions: packet.changes.key_decisions.clone(),
            narrative: packet.changes.narrative.clone(),
        });

        r.emit(UiEvent::ReviewPacketRisks {
            risks: packet.open_risks.risks.clone(),
            open_questions: packet.open_risks.open_questions.clone(),
            unavailable_data: packet.open_risks.unavailable_data.clone(),
        });

        // Show baseline freshness if available
        if let Some(ref bf) = packet.baseline_freshness {
            r.emit(UiEvent::BaselineFreshness {
                freshness: bf.freshness.label().to_string(),
                age_days: bf.age_days,
                recommendation: bf.recommendation.clone(),
            });
        }
    }

    // Resolve whether to save: explicit --save overrides config auto_save.
    let effective_save: Option<Option<String>> = match save {
        Some(_) => save, // User passed --save explicitly — honour as-is
        None if review_cfg.auto_save => Some(None), // Config auto_save triggers save with no explicit dir
        None => None,
    };

    // Save to disk if saving was resolved
    if let Some(save_dir) = effective_save {
        // Resolve target directory: explicit --save <dir> > config output_dir > run_dir.
        // Relative paths are always resolved from the workspace root (--workspace).
        let target_dir = match save_dir {
            Some(dir) => {
                let p = PathBuf::from(&dir);
                if p.is_absolute() { p } else { ws.join(p) }
            }
            None => match review_cfg.output_dir {
                Some(ref dir) => ws.join(dir),
                None => run_dir.clone(),
            },
        };

        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir)?;
        }

        let json_path = target_dir.join("review-packet.json");
        let md_path = target_dir.join("review-packet.md");

        packet
            .save_to(&json_path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        packet
            .save_markdown(&md_path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        r.emit(UiEvent::Success {
            message: format!(
                "Review packet saved to {} and {}",
                json_path.display(),
                md_path.display(),
            ),
        });
    }

    Ok(())
}

/// Compare two runs' scorecards from their persisted artifacts.
fn cmd_runs_compare(
    r: &mut dyn Renderer,
    baseline_id: String,
    candidate_id: String,
    workspace: String,
    json_output: bool,
) -> Result<()> {
    use oco_shared_types::ScorecardComparison;

    let baseline_dir = resolve_run_dir(&baseline_id, &workspace)?;
    let candidate_dir = resolve_run_dir(&candidate_id, &workspace)?;

    let weights = load_weights_for_workspace(&workspace);
    let baseline_sc = load_or_build_scorecard(&baseline_dir, &baseline_id, &weights)?;
    let candidate_sc = load_or_build_scorecard(&candidate_dir, &candidate_id, &weights)?;

    let comparison = ScorecardComparison::compare(&baseline_sc, &candidate_sc);

    if json_output {
        let json = serde_json::to_string_pretty(&comparison)?;
        r.emit(UiEvent::Info { message: json });
    } else {
        r.emit(UiEvent::ScorecardSummary {
            run_id: baseline_sc.run_id.clone(),
            overall_score: baseline_sc.overall_score,
            dimension_count: baseline_sc.dimensions.len(),
        });
        r.emit(UiEvent::ScorecardSummary {
            run_id: candidate_sc.run_id.clone(),
            overall_score: candidate_sc.overall_score,
            dimension_count: candidate_sc.dimensions.len(),
        });

        r.emit(UiEvent::ComparisonResult {
            baseline_id: comparison.baseline_id.clone(),
            candidate_id: comparison.candidate_id.clone(),
            overall_delta: comparison.overall_delta,
            regressions: comparison.regressions.len(),
            improvements: comparison.improvements.len(),
            verdict: comparison.verdict.label().to_string(),
        });

        for reg in &comparison.regressions {
            r.emit(UiEvent::ComparisonDetail {
                dimension: reg.dimension.label().to_string(),
                baseline_score: reg.baseline_score,
                candidate_score: reg.candidate_score,
                delta: reg.delta,
                kind: "regression".to_string(),
            });
        }
        for imp in &comparison.improvements {
            r.emit(UiEvent::ComparisonDetail {
                dimension: imp.dimension.label().to_string(),
                baseline_score: imp.baseline_score,
                candidate_score: imp.candidate_score,
                delta: imp.delta,
                kind: "improvement".to_string(),
            });
        }
    }

    Ok(())
}

/// Build a scorecard from a ScenarioResult using the canonical ScorecardBuilder.
fn scorecard_from_scenario_result(
    sr: &oco_shared_types::ScenarioResult,
    weights: &oco_shared_types::ScorecardWeights,
) -> oco_shared_types::RunScorecard {
    oco_orchestrator_core::ScorecardBuilder::new(&sr.scenario_name)
        .with_scenario_result(sr)
        .with_weights(weights.clone())
        .build()
}

/// Load per-repo scorecard weight overrides from `oco.toml`.
///
/// Returns `ScorecardWeights::default()` (all `None`) on any failure — this is
/// intentional: missing or broken config should never block scorecard loading.
fn load_weights_for_workspace(workspace: &str) -> oco_shared_types::ScorecardWeights {
    oco_orchestrator_core::load_scorecard_weights(Path::new(workspace)).unwrap_or_default()
}

/// Load a scorecard from disk, or reconstruct honestly from available artifacts.
///
/// Priority: scorecard.json > reconstruction from summary.json + mission.json + trace.jsonl.
/// Dimensions without data are left at the ScorecardBuilder's documented defaults
/// (which produce honest "no data" details), not fabricated scores.
///
/// `weights` are applied when reconstructing from summary.json. When loading a
/// pre-existing scorecard.json the weights have already been baked in at save time.
fn load_or_build_scorecard(
    run_dir: &Path,
    run_id: &str,
    weights: &oco_shared_types::ScorecardWeights,
) -> Result<oco_shared_types::RunScorecard> {
    use oco_shared_types::TrustVerdict;

    // Try scorecard.json first (saved by oco run since Q5).
    let scorecard_path = run_dir.join("scorecard.json");
    if scorecard_path.exists() {
        let content = std::fs::read_to_string(&scorecard_path)?;
        let sc: oco_shared_types::RunScorecard = serde_json::from_str(&content)?;
        return Ok(sc);
    }

    // Reconstruct from available artifacts.
    let summary_path = run_dir.join("summary.json");
    if !summary_path.exists() {
        anyhow::bail!("no scorecard.json or summary.json in {}", run_dir.display());
    }
    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&summary_path)?)?;

    let success = summary["success"].as_bool().unwrap_or(false);
    let tokens = summary["tokens_used"].as_u64().unwrap_or(0);
    let steps = summary["steps"].as_u64().unwrap_or(0) as u32;
    let duration_ms = summary["duration_ms"].as_u64().unwrap_or(0);

    let trust = match summary.get("trust_verdict").and_then(|v| v.as_str()) {
        Some("high") => TrustVerdict::High,
        Some("medium") => TrustVerdict::Medium,
        Some("low") => TrustVerdict::Low,
        _ => TrustVerdict::None,
    };

    let mut builder = oco_orchestrator_core::ScorecardBuilder::new(run_id)
        .success(success)
        .trust_verdict(trust)
        .cost(tokens, steps, duration_ms, 0, 0)
        .with_weights(weights.clone());

    // Mission memory — use the real artifact if present.
    let mission_path = run_dir.join("mission.json");
    if mission_path.exists()
        && let Ok(mission) = oco_shared_types::MissionMemory::load_from(&mission_path)
    {
        builder = builder.with_mission_memory(&mission);
    }

    // Replan count — reconstruct from trace.jsonl by counting ReplanTriggered events.
    let trace_path = run_dir.join("trace.jsonl");
    if trace_path.exists()
        && let Ok(content) = std::fs::read_to_string(&trace_path)
    {
        let replan_count = content
            .lines()
            .filter(|line| line.contains("\"replan_triggered\""))
            .count() as u32;
        builder = builder.replans(replan_count);
    }

    // Note: VerificationCoverage and ErrorRate are left at builder defaults
    // because summary.json doesn't carry file-level or error-level data.
    // The builder will produce honest "no data" details for those dimensions.

    Ok(builder.build())
}

// ═══════════════════════════════════════════════════════════
// Q6: Eval gate and baseline commands
// ════════════════════════════════════════════════════════���══

/// Parse a policy name into a `GatePolicy`, failing on unknown values.
fn parse_policy_name(name: &str) -> Result<oco_shared_types::GatePolicy> {
    match name {
        "strict" => Ok(oco_shared_types::GatePolicy::strict()),
        "balanced" => Ok(oco_shared_types::GatePolicy::default_balanced()),
        "lenient" => Ok(oco_shared_types::GatePolicy::lenient()),
        other => anyhow::bail!(
            "unknown gate policy '{}', expected: strict, balanced, lenient",
            other
        ),
    }
}

/// Run a quality gate: compare candidate against baseline with pass/warn/fail verdict.
/// Returns the exit code (0=pass, 1=warn, 2=fail).
///
/// Q7: When `baseline_path` or `policy_name` are `None`, reads `[gate]` from `oco.toml`.
/// Q8: `--report <dir>` generates a review artifact (Markdown + JSON).
fn cmd_eval_gate(
    r: &mut dyn Renderer,
    baseline_path: Option<String>,
    candidate_path: Option<String>,
    policy_name: Option<String>,
    json_output: bool,
    report_dir: Option<String>,
    workspace: String,
) -> Result<i32> {
    use oco_shared_types::{BaselineFreshnessCheck, GateResult, GateReviewArtifact};

    // Load repo gate config from oco.toml (Q7) — strict: fail on invalid config
    let ws = Path::new(&workspace);
    let gate_cfg = oco_orchestrator_core::load_gate_config_strict(ws)
        .map_err(|e| anyhow::anyhow!("cannot resolve gate config: {e}"))?;

    // Resolve policy: explicit CLI arg > repo config > balanced default
    let policy = if let Some(ref name) = policy_name {
        parse_policy_name(name)?
    } else {
        gate_cfg.resolve_policy()
    };

    // Resolve baseline path: explicit CLI arg > repo config
    let effective_baseline = baseline_path.unwrap_or_else(|| {
        let cfg_path = ws.join(&gate_cfg.baseline_path);
        cfg_path.to_string_lossy().to_string()
    });

    // Resolve candidate: explicit CLI arg > "last" run
    let effective_candidate = candidate_path.unwrap_or_else(|| "last".to_string());

    // Load baseline scorecard — supports both EvalBaseline and raw RunScorecard
    let baseline_sc = load_baseline_scorecard(&effective_baseline)?;

    // Load candidate scorecard — supports scorecard.json, eval results, run dir, or "last"
    let weights = load_weights_for_workspace(&workspace);
    let candidate_sc = if effective_candidate == "last" {
        let run_dir = resolve_run_dir("last", &workspace)?;
        load_or_build_scorecard(&run_dir, "last", &weights)?
    } else {
        load_candidate_scorecard(&effective_candidate, &weights)?
    };

    // Evaluate gate
    let result = GateResult::evaluate(&baseline_sc, &candidate_sc, &policy);

    if json_output {
        let json = serde_json::to_string_pretty(&result)?;
        r.emit(UiEvent::Info { message: json });
    } else {
        // Structured output via UI events
        r.emit(UiEvent::GateHeader {
            baseline_id: result.baseline_id.clone(),
            candidate_id: result.candidate_id.clone(),
            policy: format!("{:?}", policy.strategy),
        });

        for check in &result.dimension_checks {
            r.emit(UiEvent::GateDimensionCheck {
                dimension: check.dimension.label().to_string(),
                baseline_score: check.baseline_score,
                candidate_score: check.candidate_score,
                delta: check.delta,
                min_score: check.min_score,
                verdict: check.verdict.label().to_string(),
            });
        }

        r.emit(UiEvent::GateVerdict {
            verdict: result.verdict.label().to_string(),
            exit_code: result.verdict.exit_code(),
            reasons: result.reasons.clone(),
            failed_count: result.failed_dimension_count(),
            warned_count: result.warned_dimension_count(),
        });

        // Q8: Show baseline freshness status in terminal (non-JSON) mode.
        // Works for both EvalBaseline (with created_at) and raw RunScorecard (Unknown).
        let freshness = resolve_freshness(&effective_baseline, &gate_cfg)?;
        r.emit(UiEvent::BaselineFreshness {
            freshness: freshness.freshness.label().to_string(),
            age_days: freshness.age_days,
            recommendation: freshness.recommendation.clone(),
        });
    }

    // Q8: Generate review artifact report if --report is provided.
    // Supports both EvalBaseline and raw RunScorecard baselines (Unknown freshness).
    if let Some(ref dir) = report_dir {
        let report_path = PathBuf::from(dir);

        if !report_path.exists() {
            std::fs::create_dir_all(&report_path)
                .map_err(|e| anyhow::anyhow!("failed to create report directory '{}': {e}", dir))?;
        }

        let (baseline_name, freshness) = match load_eval_baseline(&effective_baseline)? {
            Some(eval_baseline) => {
                let name = eval_baseline.name.clone();
                let fc = BaselineFreshnessCheck::from_baseline(
                    &eval_baseline,
                    gate_cfg.fresh_days,
                    gate_cfg.stale_days,
                );
                (name, fc)
            }
            None => {
                // Raw RunScorecard — use run_id as name, Unknown freshness
                (
                    result.baseline_id.clone(),
                    BaselineFreshnessCheck::unknown(),
                )
            }
        };

        let artifact =
            GateReviewArtifact::generate_with_name(result.clone(), &baseline_name, freshness);

        let md_path = report_path.join("gate-report.md");
        let json_path = report_path.join("gate-report.json");

        artifact
            .save_markdown(&md_path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        artifact
            .save_json(&json_path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        r.emit(UiEvent::Success {
            message: format!(
                "Review report written to {} and {}",
                md_path.display(),
                json_path.display(),
            ),
        });
    }

    Ok(result.verdict.exit_code())
}

/// Try to load a file as a full `EvalBaseline` (with `created_at` metadata).
/// Returns `Ok(None)` if the file is a raw `RunScorecard` without baseline metadata.
fn load_eval_baseline(path: &str) -> Result<Option<oco_shared_types::EvalBaseline>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read baseline '{}': {e}", path))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse baseline JSON: {e}"))?;

    // EvalBaseline has both "scorecard" and "name" fields
    if value.get("scorecard").is_some() && value.get("name").is_some() {
        let baseline: oco_shared_types::EvalBaseline = serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("failed to parse as EvalBaseline: {e}"))?;
        return Ok(Some(baseline));
    }

    // Raw RunScorecard — no baseline metadata available
    Ok(None)
}

/// Resolve baseline freshness: EvalBaseline → real check, raw RunScorecard → Unknown.
fn resolve_freshness(
    baseline_path: &str,
    gate_cfg: &oco_shared_types::GateConfig,
) -> Result<oco_shared_types::BaselineFreshnessCheck> {
    match load_eval_baseline(baseline_path)? {
        Some(eval_baseline) => Ok(oco_shared_types::BaselineFreshnessCheck::from_baseline(
            &eval_baseline,
            gate_cfg.fresh_days,
            gate_cfg.stale_days,
        )),
        None => Ok(oco_shared_types::BaselineFreshnessCheck::unknown()),
    }
}

/// Load a scorecard from a baseline file (EvalBaseline JSON) or a raw RunScorecard JSON.
fn load_baseline_scorecard(path: &str) -> Result<oco_shared_types::RunScorecard> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read baseline '{}': {e}", path))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse baseline JSON: {e}"))?;

    // Try EvalBaseline first (has "scorecard" field)
    if value.get("scorecard").is_some() && value.get("name").is_some() {
        let baseline: oco_shared_types::EvalBaseline = serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("failed to parse as EvalBaseline: {e}"))?;
        return Ok(baseline.scorecard);
    }

    // Try raw RunScorecard (has "run_id" and "dimensions")
    if value.get("run_id").is_some() && value.get("dimensions").is_some() {
        let sc: oco_shared_types::RunScorecard = serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("failed to parse as RunScorecard: {e}"))?;
        return Ok(sc);
    }

    anyhow::bail!(
        "baseline file '{}' is neither an EvalBaseline nor a RunScorecard",
        path
    )
}

/// Load a candidate scorecard from various sources.
///
/// `weights` are applied when reconstructing from eval results or summary.json.
/// Pre-existing scorecard.json and EvalBaseline files already have weights baked in.
fn load_candidate_scorecard(
    path: &str,
    weights: &oco_shared_types::ScorecardWeights,
) -> Result<oco_shared_types::RunScorecard> {
    let p = PathBuf::from(path);

    // If it's a directory (run dir), load scorecard.json from it
    if p.is_dir() {
        let scorecard_path = p.join("scorecard.json");
        if scorecard_path.exists() {
            let content = std::fs::read_to_string(&scorecard_path)?;
            let sc: oco_shared_types::RunScorecard = serde_json::from_str(&content)?;
            return Ok(sc);
        }
        // Fall back to reconstruct from summary
        return load_or_build_scorecard(
            &p,
            &p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            weights,
        );
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read candidate '{}': {e}", path))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse candidate JSON: {e}"))?;

    // Try EvalBaseline format
    if value.get("scorecard").is_some() && value.get("name").is_some() {
        let baseline: oco_shared_types::EvalBaseline = serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("failed to parse as EvalBaseline: {e}"))?;
        return Ok(baseline.scorecard);
    }

    // Try raw RunScorecard
    if value.get("run_id").is_some() && value.get("dimensions").is_some() {
        let sc: oco_shared_types::RunScorecard = serde_json::from_value(value)?;
        return Ok(sc);
    }

    // Try eval results envelope (has "results" array) — aggregate all scenarios into a suite scorecard
    if let Some(results) = value.get("results").and_then(|r| r.as_array()) {
        let scenario_results: Vec<oco_shared_types::ScenarioResult> =
            serde_json::from_value(serde_json::Value::Array(results.clone()))
                .map_err(|e| anyhow::anyhow!("failed to parse eval results array: {e}"))?;

        if scenario_results.is_empty() {
            anyhow::bail!(
                "eval results file '{}' contains an empty results array",
                path
            );
        }

        let scorecards: Vec<oco_shared_types::RunScorecard> = scenario_results
            .iter()
            .map(|sr| scorecard_from_scenario_result(sr, weights))
            .collect();

        return oco_shared_types::RunScorecard::aggregate(&scorecards)
            .ok_or_else(|| anyhow::anyhow!("failed to aggregate scorecards from '{}'", path));
    }

    anyhow::bail!(
        "candidate file '{}' is not a recognized format (EvalBaseline, RunScorecard, or eval results)",
        path
    )
}

/// Save a baseline from a run's scorecard or eval results.
///
/// Q7: When `output` is `None`, reads baseline_path from repo config.
fn cmd_baseline_save(
    r: &mut dyn Renderer,
    source: String,
    name: String,
    output: Option<String>,
    workspace: String,
    description: Option<String>,
) -> Result<()> {
    use oco_shared_types::EvalBaseline;

    // Q7: resolve output path from repo config when not explicitly provided — strict
    let effective_output = match output {
        Some(o) => o,
        None => {
            let ws = Path::new(&workspace);
            let gate_cfg = oco_orchestrator_core::load_gate_config_strict(ws)
                .map_err(|e| anyhow::anyhow!("cannot resolve gate config: {e}"))?;
            let cfg_path = ws.join(&gate_cfg.baseline_path);
            cfg_path.to_string_lossy().to_string()
        }
    };
    let output_path = PathBuf::from(&effective_output);

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let weights = load_weights_for_workspace(&workspace);
    let (scorecard, source_label) = if source == "last" || uuid::Uuid::parse_str(&source).is_ok() {
        // Load from run dir
        let run_dir = resolve_run_dir(&source, &workspace)?;
        let sc = load_or_build_scorecard(&run_dir, &source, &weights)?;
        (sc, format!("run:{source}"))
    } else {
        // Try loading as a file
        let sc = load_candidate_scorecard(&source, &weights)?;
        (sc, format!("file:{source}"))
    };

    let mut baseline = EvalBaseline::from_scorecard(name.clone(), scorecard, source_label);
    if let Some(desc) = description {
        baseline = baseline.with_description(desc);
    }

    baseline
        .save_to(&output_path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    r.emit(UiEvent::Success {
        message: format!(
            "Baseline '{}' saved to {} (overall: {:.2})",
            name,
            output_path.display(),
            baseline.scorecard.overall_score,
        ),
    });

    Ok(())
}

// ═══════════════════════════════════════════════════════════
// Q11: Baseline promotion & audit trail
// ═══════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
fn cmd_baseline_promote(
    r: &mut dyn Renderer,
    source: String,
    name: String,
    reason: Option<String>,
    description: Option<String>,
    workspace: String,
    json: bool,
    force: bool,
) -> Result<()> {
    let ws = Path::new(&workspace);

    // Resolve scorecard from source (same logic as baseline-save)
    let weights = load_weights_for_workspace(&workspace);
    let (scorecard, source_label) = if source == "last" || uuid::Uuid::parse_str(&source).is_ok() {
        let run_dir = resolve_run_dir(&source, &workspace)?;
        let sc = load_or_build_scorecard(&run_dir, &source, &weights)?;
        (sc, format!("run:{source}"))
    } else {
        let sc = load_candidate_scorecard(&source, &weights)?;
        (sc, format!("file:{source}"))
    };

    let result = oco_orchestrator_core::promote_baseline(
        ws,
        scorecard,
        name.clone(),
        source_label,
        reason,
        description,
        force,
    );

    // Handle PromotionRejected: show the record + diff, then abort with exit code 2
    let record = match result {
        Ok(record) => record,
        Err(oco_orchestrator_core::config::ConfigError::PromotionRejected(record)) => {
            if json {
                // In JSON mode, output the record with an "aborted" wrapper
                let wrapper = serde_json::json!({
                    "status": "rejected",
                    "message": "promotion rejected — use --force to override",
                    "record": record,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&wrapper)
                        .map_err(|e| anyhow::anyhow!("serialize: {e}"))?
                );
            } else {
                r.emit(UiEvent::Error {
                    message: format!(
                        "{} Promotion rejected: '{}' → '{}' (gate: {})",
                        record.recommendation.symbol(),
                        record.old_baseline_name,
                        record.new_baseline_name,
                        record
                            .gate_verdict
                            .map_or("n/a".to_string(), |v| v.symbol().to_string()),
                    ),
                });
                println!();
                println!("{}", record.diff.to_report());
                println!();
                r.emit(UiEvent::Info {
                    message: "Use --force to override this safety check.".to_string(),
                });
            }
            std::process::exit(2);
        }
        Err(e) => return Err(anyhow::anyhow!("promotion failed: {e}")),
    };

    if json {
        let json_str = serde_json::to_string_pretty(&record)
            .map_err(|e| anyhow::anyhow!("failed to serialize promotion record: {e}"))?;
        println!("{json_str}");
    } else {
        // Terminal-friendly output
        r.emit(UiEvent::Success {
            message: format!(
                "{} Promoted '{}' → '{}' (overall: {:.2} → {:.2}, {:+.2})",
                record.recommendation.symbol(),
                record.old_baseline_name,
                record.new_baseline_name,
                record.diff.old_overall,
                record.diff.new_overall,
                record.diff.overall_delta,
            ),
        });
        if let Some(gv) = record.gate_verdict {
            r.emit(UiEvent::Info {
                message: format!("Gate verdict: {}", gv.symbol()),
            });
        }
        if let Some(bf) = record.baseline_freshness {
            r.emit(UiEvent::Info {
                message: format!("Old baseline freshness: {}", bf.symbol()),
            });
        }
        if record.recommendation != oco_shared_types::PromotionRecommendation::Promote {
            r.emit(UiEvent::Warning {
                message: format!(
                    "Recommendation: {} — review the diff before relying on this baseline",
                    record.recommendation.label(),
                ),
            });
        }

        // Show diff table
        println!();
        println!("{}", record.diff.to_report());
    }

    Ok(())
}

fn cmd_baseline_history(
    r: &mut dyn Renderer,
    workspace: String,
    json: bool,
    markdown: bool,
    limit: usize,
) -> Result<()> {
    let ws = Path::new(&workspace);
    let history = oco_orchestrator_core::load_baseline_history(ws)
        .map_err(|e| anyhow::anyhow!("failed to load baseline history: {e}"))?;

    if history.is_empty() {
        r.emit(UiEvent::Info {
            message: "No baseline promotions recorded yet.".to_string(),
        });
        return Ok(());
    }

    if json {
        // In JSON mode with limit, only output the last N entries
        let limited = if limit < history.len() {
            let mut h = history.clone();
            let start = h.entries.len().saturating_sub(limit);
            h.entries = h.entries[start..].to_vec();
            h
        } else {
            history
        };
        let json_str = limited.to_json().map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("{json_str}");
    } else if markdown {
        // Apply limit: slice entries the same way as JSON and terminal
        let limited = if limit < history.len() {
            let mut h = history.clone();
            let start = h.entries.len().saturating_sub(limit);
            h.entries = h.entries[start..].to_vec();
            h
        } else {
            history
        };
        println!("{}", limited.to_markdown());
    } else {
        // Terminal output — show recent entries
        let recent = history.recent(limit);
        r.emit(UiEvent::Info {
            message: format!(
                "Baseline History ({} total, showing {})",
                history.len(),
                recent.len()
            ),
        });
        for entry in &recent {
            println!();
            println!("#{}", entry.sequence);
            println!("{}", entry.promotion.to_summary());
        }
    }

    Ok(())
}

fn cmd_baseline_history_prune(
    r: &mut dyn Renderer,
    workspace: String,
    keep: usize,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let ws = Path::new(&workspace);
    let mut history = oco_orchestrator_core::load_baseline_history(ws)
        .map_err(|e| anyhow::anyhow!("failed to load baseline history: {e}"))?;

    if history.is_empty() {
        r.emit(UiEvent::Info {
            message: "No baseline promotions recorded. Nothing to prune.".to_string(),
        });
        return Ok(());
    }

    if dry_run {
        let to_remove = history.prune_preview(keep);
        if to_remove.is_empty() {
            r.emit(UiEvent::Info {
                message: format!(
                    "Nothing to prune: {} entries, keeping {}.",
                    history.len(),
                    keep
                ),
            });
        } else if json {
            let entries: Vec<_> = to_remove.iter().map(|e| &e.promotion).collect();
            let json_str = serde_json::to_string_pretty(&serde_json::json!({
                "dry_run": true,
                "total": history.len(),
                "keep": keep,
                "would_remove": to_remove.len(),
                "entries": entries,
            }))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{json_str}");
        } else {
            r.emit(UiEvent::Info {
                message: format!(
                    "Dry run: would remove {} of {} entries (keeping {}):",
                    to_remove.len(),
                    history.len(),
                    keep
                ),
            });
            for entry in &to_remove {
                println!(
                    "  #{}: {}",
                    entry.sequence, entry.promotion.new_baseline_name
                );
            }
        }
        return Ok(());
    }

    let total_before = history.len();
    let removed = history.prune(keep);

    if removed == 0 {
        r.emit(UiEvent::Info {
            message: format!(
                "Nothing to prune: {} entries, keeping {}.",
                total_before, keep
            ),
        });
        return Ok(());
    }

    // Save the pruned history
    let history_path = ws.join(oco_orchestrator_core::DEFAULT_HISTORY_PATH);
    history
        .save_to(&history_path)
        .map_err(|e| anyhow::anyhow!("failed to save pruned history: {e}"))?;

    if json {
        let json_str = serde_json::to_string_pretty(&serde_json::json!({
            "pruned": removed,
            "remaining": history.len(),
            "total_before": total_before,
        }))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("{json_str}");
    } else {
        r.emit(UiEvent::Info {
            message: format!(
                "Pruned {} entries. {} entries remaining (was {}).",
                removed,
                history.len(),
                total_before,
            ),
        });
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
