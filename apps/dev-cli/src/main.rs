use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "oco", version, about = "Open Context Orchestrator — Dev CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    /// Output format (human, json)
    #[arg(long, default_value = "human", global = true)]
    format: String,
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
        /// LLM provider (stub, anthropic, ollama)
        #[arg(long, default_value = "stub")]
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
    /// v2: Run evaluation scenarios
    Eval {
        /// Path to scenarios JSONL file
        scenarios: String,
        /// Output results to file (JSON)
        #[arg(long)]
        output: Option<String>,
        /// LLM provider (default: stub)
        #[arg(long, default_value = "stub")]
        provider: String,
    },
    /// v2: Check plugin health and configuration
    Doctor {
        /// Workspace path to check
        #[arg(long, default_value = ".")]
        workspace: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize telemetry
    oco_telemetry::init_tracing(oco_telemetry::TelemetryConfig {
        log_level: cli.log_level.clone(),
        json_output: cli.format == "json",
        trace_file: None,
    })?;

    match cli.command {
        Commands::Serve { host, port } => {
            let mut config = oco_orchestrator_core::OrchestratorConfig::load_from_dir(
                &std::env::current_dir()?,
            );
            config.bind_address = host;
            config.port = port;

            let server = oco_mcp_server::McpServer::new(config);
            server.run().await?;
        }
        Commands::Run {
            request,
            workspace,
            provider,
            model,
            max_steps: _,
        } => {
            let mut config = oco_orchestrator_core::OrchestratorConfig::default();
            config.default_budget.max_duration_secs = 120;

            // Select LLM provider
            let llm: Arc<dyn oco_orchestrator_core::llm::LlmProvider> = match provider.as_str() {
                "anthropic" => {
                    let model_name = model.unwrap_or_else(|| config.llm.model.clone());
                    let anthropic_config =
                        oco_orchestrator_core::llm::AnthropicConfig::from_env(
                            &model_name,
                            None,
                        )?;
                    Arc::new(oco_orchestrator_core::llm::AnthropicProvider::new(
                        anthropic_config,
                    )?)
                }
                "ollama" => {
                    let model_name =
                        model.unwrap_or_else(|| "llama3.2".to_string());
                    let ollama_config =
                        oco_orchestrator_core::llm::OllamaConfig::new(&model_name);
                    Arc::new(oco_orchestrator_core::llm::OllamaProvider::new(
                        ollama_config,
                    )?)
                }
                _ => Arc::new(oco_orchestrator_core::llm::StubLlmProvider {
                    model: model.unwrap_or_else(|| config.llm.model.clone()),
                }),
            };

            println!("Provider: {} ({})", llm.provider_name(), llm.model_name());

            let mut orchestrator =
                oco_orchestrator_core::OrchestrationLoop::new(config, llm);

            // Index workspace if provided
            if let Some(ref ws) = workspace {
                let ws_path = PathBuf::from(ws).canonicalize().unwrap_or_else(|_| PathBuf::from(ws));
                println!("Indexing workspace: {}", ws_path.display());
                orchestrator.with_workspace(ws_path);
            }

            println!("Request: {request}");
            println!("---");

            let state = orchestrator.run(request, workspace).await?;

            // Print results
            println!("---");
            println!("Session: {}", state.session.id.0);
            println!("Steps: {}", state.session.step_count);
            println!(
                "Tokens used: {} / {}",
                state.session.budget.tokens_used,
                state.session.budget.max_total_tokens
            );

            if cli.format == "json" {
                let output = serde_json::json!({
                    "session_id": state.session.id.0.to_string(),
                    "steps": state.session.step_count,
                    "complexity": state.task_complexity,
                    "tokens_used": state.session.budget.tokens_used,
                    "traces": state.traces,
                    "final_response": state.observations.iter().rev()
                        .find(|o| matches!(o.source, oco_shared_types::ObservationSource::LlmResponse))
                        .and_then(|o| if let oco_shared_types::ObservationKind::Text { content, .. } = &o.kind { Some(content.clone()) } else { None })
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                // Print decision trace
                println!("\nDecision trace:");
                for trace in &state.traces {
                    let action_type = match &trace.action {
                        oco_shared_types::OrchestratorAction::Respond { .. } => "RESPOND",
                        oco_shared_types::OrchestratorAction::Retrieve { .. } => "RETRIEVE",
                        oco_shared_types::OrchestratorAction::ToolCall { .. } => "TOOL_CALL",
                        oco_shared_types::OrchestratorAction::Verify { .. } => "VERIFY",
                        oco_shared_types::OrchestratorAction::Stop { .. } => "STOP",
                    };
                    println!(
                        "  [{:>2}] {:<10} | {:<50} | conf: {:.2}",
                        trace.step, action_type, trace.reason, trace.knowledge_confidence
                    );
                }

                // Print final response if available
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

                if let Some(response) = final_response {
                    println!("\nResponse:\n{response}");
                }
            }
        }
        Commands::Index { path } => {
            let ws_path = PathBuf::from(&path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&path));
            println!("Indexing {}...", ws_path.display());

            let mut runtime = oco_orchestrator_core::OrchestratorRuntime::new(ws_path);
            let result = runtime.index_workspace()?;

            println!(
                "Done: {} files indexed, {} symbols extracted",
                result.file_count, result.symbol_count
            );
        }
        Commands::Search {
            query,
            workspace,
            limit,
        } => {
            let ws_path = PathBuf::from(&workspace)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&workspace));

            let mut runtime = oco_orchestrator_core::OrchestratorRuntime::new(ws_path);
            runtime.index_workspace()?;

            let results = runtime.search(&query, limit)?;

            if results.is_empty() {
                println!("No results for \"{query}\"");
            } else {
                for (i, r) in results.iter().enumerate() {
                    println!("{}. {} (score: {:.2})", i + 1, r.path, r.score);
                    println!("   {}", r.snippet.replace('\n', "\n   "));
                    println!();
                }
            }
        }
        Commands::Status { url } => {
            let resp = reqwest::get(format!("{url}/api/v1/status")).await?;
            let body: serde_json::Value = resp.json().await?;
            if cli.format == "json" {
                println!("{}", serde_json::to_string_pretty(&body)?);
            } else {
                println!("Status: {}", body["status"]);
                println!("Steps: {}", body["steps"]);
                println!("Tokens used: {}", body["tokens_used"]);
            }
        }
        Commands::Trace { session_id, url } => {
            let resp =
                reqwest::get(format!("{url}/api/v1/sessions/{session_id}/trace")).await?;
            let body: serde_json::Value = resp.json().await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Commands::Init { output } => {
            let path = PathBuf::from(&output);
            if path.exists() {
                anyhow::bail!("{output} already exists. Remove it first or use --output to specify a different path.");
            }
            let config = oco_orchestrator_core::OrchestratorConfig::default();
            let toml_str = config.to_toml()?;
            std::fs::write(&path, toml_str)?;
            println!("Created {output}");
            println!("Edit it to configure LLM provider, budget limits, etc.");
        }
        Commands::Classify { prompt, workspace } => {
            let ws_path = PathBuf::from(&workspace);

            // Gather lightweight workspace signals
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

            // Determine task type from keywords
            let prompt_lower = prompt.to_lowercase();
            let task_type = if prompt_lower.contains("refactor") || prompt_lower.contains("rename") {
                "refactor"
            } else if prompt_lower.contains("bug") || prompt_lower.contains("fix") || prompt_lower.contains("debug") {
                "bugfix"
            } else if prompt_lower.contains("test") {
                "testing"
            } else if prompt_lower.contains("implement") || prompt_lower.contains("create") || prompt_lower.contains("add") {
                "feature"
            } else if prompt_lower.contains("explain") || prompt_lower.contains("what") || prompt_lower.contains("how") {
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

            // Collect priority files: check for recently modified files in workspace
            let priority_files: Vec<String> = collect_priority_files(&ws_path, &prompt);

            let output = serde_json::json!({
                "complexity": complexity,
                "task_type": task_type,
                "needs_verification": needs_verification,
                "priority_files": priority_files,
                "workspace_signals": signals,
            });

            if cli.format == "json" {
                println!("{}", serde_json::to_string(&output)?);
            } else {
                println!("Complexity: {:?}", complexity);
                println!("Task type: {task_type}");
                println!("Needs verification: {needs_verification}");
                if !priority_files.is_empty() {
                    println!("Priority files: {}", priority_files.join(", "));
                }
            }
        }
        Commands::GateCheck {
            tool,
            input,
            policy,
        } => {
            let write_policy = match policy.as_str() {
                "allow_all" => oco_policy_engine::WritePolicy::AllowAll,
                "deny_destructive" => oco_policy_engine::WritePolicy::DenyDestructive,
                _ => oco_policy_engine::WritePolicy::RequireConfirmation,
            };

            let gate = oco_policy_engine::PolicyGate::new(write_policy);

            // Parse JSON input — fail-closed on invalid JSON
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

            // Check if this is a shell/bash tool with a command argument
            let decision = if tool.to_lowercase() == "bash" || tool.to_lowercase() == "shell" {
                let command = input_json
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if command.is_empty() {
                    // Missing command for shell tool — deny
                    oco_shared_types::ToolGateDecision::Deny {
                        reason: "shell tool called without a command field".to_string(),
                    }
                } else {
                    gate.evaluate_command(command)
                }
            } else {
                // Build a ToolDescriptor from the tool name and input
                // Known write tools
                let tool_lower = tool.to_lowercase();
                let is_write = matches!(
                    tool_lower.as_str(),
                    "edit" | "write" | "file_write" | "file_delete" | "directory_delete"
                        | "notebookedit" | "multiedit"
                );
                let is_destructive = matches!(
                    tool_lower.as_str(),
                    "file_delete" | "directory_delete" | "git_reset" | "git_force_push"
                );
                // Fail-closed: unknown tools that aren't in the known-safe read list
                // require confirmation
                let known_read = matches!(
                    tool_lower.as_str(),
                    "read" | "glob" | "grep" | "bash" | "shell" | "web_search"
                        | "web_fetch" | "list_files"
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

            if cli.format == "json" {
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
        }
        Commands::Eval {
            scenarios,
            output,
            provider,
        } => {
            let config = oco_orchestrator_core::OrchestratorConfig::load_from_dir(
                &std::env::current_dir()?,
            );

            let llm: Arc<dyn oco_orchestrator_core::llm::LlmProvider> = match provider.as_str() {
                "anthropic" => {
                    let anthropic_config =
                        oco_orchestrator_core::llm::AnthropicConfig::from_env(
                            &config.llm.model,
                            None,
                        )?;
                    Arc::new(oco_orchestrator_core::llm::AnthropicProvider::new(
                        anthropic_config,
                    )?)
                }
                _ => Arc::new(oco_orchestrator_core::llm::StubLlmProvider {
                    model: config.llm.model.clone(),
                }),
            };

            let scenario_path = PathBuf::from(&scenarios);
            let loaded = oco_orchestrator_core::eval::load_scenarios(&scenario_path)?;
            println!("Loaded {} scenarios from {scenarios}", loaded.len());

            let results =
                oco_orchestrator_core::eval::run_all(&loaded, llm, &config).await;

            let metrics = oco_orchestrator_core::eval::aggregate_metrics(&results);

            if let Some(ref output_path) = output {
                let json = serde_json::to_string_pretty(&metrics)?;
                std::fs::write(output_path, json)?;
                println!("Results written to {output_path}");
            } else {
                for m in &metrics {
                    println!(
                        "{}: success={}, steps={}, tokens={}, duration={}ms, tokens/step={:.0}",
                        m.scenario_name,
                        m.success,
                        m.step_count,
                        m.total_tokens,
                        m.duration_ms,
                        m.token_per_step,
                    );
                }
            }
        }
        Commands::Doctor { workspace } => {
            let ws_path = PathBuf::from(&workspace);
            println!("OCO Doctor — checking {}", ws_path.display());
            println!();

            let mut issues = 0u32;

            // Check oco.toml
            let config_path = ws_path.join("oco.toml");
            if config_path.exists() {
                match oco_orchestrator_core::OrchestratorConfig::from_file(&config_path) {
                    Ok(_) => println!("  [PASS] oco.toml is valid"),
                    Err(e) => {
                        println!("  [FAIL] oco.toml parse error: {e}");
                        issues += 1;
                    }
                }
            } else {
                println!("  [WARN] oco.toml not found — using defaults");
            }

            // Check .oco directory
            let oco_dir = ws_path.join(".oco");
            if oco_dir.exists() {
                println!("  [PASS] .oco/ directory exists");
                let db_path = oco_dir.join("index.db");
                if db_path.exists() {
                    println!("  [PASS] index.db exists");
                } else {
                    println!("  [WARN] index.db not found — run `oco index .` first");
                }
            } else {
                println!("  [WARN] .oco/ directory not found — workspace not indexed");
            }

            // Check .claude/ directory
            let claude_dir = ws_path.join(".claude");
            if claude_dir.exists() {
                println!("  [PASS] .claude/ directory exists");

                // Check hooks
                let hooks_dir = claude_dir.join("hooks").join("scripts");
                if hooks_dir.exists() {
                    let hook_files = ["pre-tool-use.sh", "post-tool-use.sh", "user-prompt-submit.sh", "stop.sh"];
                    for hook in &hook_files {
                        if hooks_dir.join(hook).exists() {
                            println!("  [PASS] hook {hook}");
                        } else {
                            println!("  [WARN] hook {hook} missing");
                        }
                    }
                } else {
                    println!("  [WARN] .claude/hooks/scripts/ not found");
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
                                println!("  [PASS] settings.json has hooks configured");
                            } else {
                                println!("  [WARN] settings.json missing hooks section");
                            }
                            if v.get("mcpServers").is_some() {
                                println!("  [PASS] settings.json has MCP servers configured");
                            } else {
                                println!("  [WARN] settings.json missing MCP servers");
                            }
                        }
                        None => {
                            println!("  [FAIL] settings.json is invalid JSON");
                            issues += 1;
                        }
                    }
                } else {
                    println!("  [FAIL] .claude/settings.json not found");
                    issues += 1;
                }

                // Check skills
                let skills_dir = claude_dir.join("skills");
                if skills_dir.exists() {
                    let count = std::fs::read_dir(&skills_dir)
                        .map(|entries| entries.filter_map(|e| e.ok()).count())
                        .unwrap_or(0);
                    println!("  [PASS] {count} skills found");
                }
            } else {
                println!("  [WARN] .claude/ directory not found — not a Claude Code plugin");
            }

            // Detect repo profile
            let profile = oco_shared_types::RepoProfile::detect(&ws_path);
            println!();
            println!("  Stack: {}", profile.stack);
            if let Some(ref cmd) = profile.build_command {
                println!("  Build: {cmd}");
            }
            if let Some(ref cmd) = profile.test_command {
                println!("  Test:  {cmd}");
            }

            println!();
            if issues == 0 {
                println!("All checks passed.");
            } else {
                println!("{issues} issue(s) found.");
            }
        }
        Commands::Observe {
            tool,
            status,
            output,
        } => {
            let timestamp = chrono::Utc::now();

            // Write observation to local telemetry log
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

            // Append to observations log (newline-delimited JSON)
            let log_path = oco_dir.join("observations.jsonl");
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)?;
            writeln!(file, "{}", serde_json::to_string(&observation)?)?;

            let result = serde_json::json!({
                "recorded": true,
                "timestamp": timestamp.to_rfc3339(),
                "tool": tool,
                "status": status,
                "log": log_path.display().to_string(),
            });

            if cli.format == "json" {
                println!("{}", serde_json::to_string(&result)?);
            } else {
                println!("Recorded: {tool} ({status}) at {}", timestamp.to_rfc3339());
            }
        }
    }

    Ok(())
}

/// Collect priority files by scanning for files matching keywords in the prompt.
fn collect_priority_files(workspace: &std::path::Path, prompt: &str) -> Vec<String> {
    let mut files = Vec::new();
    let prompt_lower = prompt.to_lowercase();

    // Extract potential file names or module names from the prompt
    let words: Vec<&str> = prompt_lower.split_whitespace().collect();

    // Quick scan: look for recently modified files in src/ that match prompt keywords
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

    // Limit to top 5
    files.truncate(5);
    files
}
