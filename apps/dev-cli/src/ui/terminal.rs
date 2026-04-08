use console::{Style, Term, style};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use super::Renderer;
use super::event::{CheckStatus, UiEvent};

/// Human-friendly terminal renderer with colors, spinners, and structured output.
pub struct TerminalRenderer {
    term: Term,
    use_unicode: bool,
}

impl TerminalRenderer {
    pub fn new() -> Self {
        let term = Term::stderr();
        let use_unicode = console::colors_enabled_stderr();
        Self { term, use_unicode }
    }

    fn icon_pass(&self) -> &str {
        if self.use_unicode { "✓" } else { "OK" }
    }
    fn icon_fail(&self) -> &str {
        if self.use_unicode { "✗" } else { "ERR" }
    }
    fn icon_warn(&self) -> &str {
        if self.use_unicode { "!" } else { "WARN" }
    }
    fn icon_bullet(&self) -> &str {
        if self.use_unicode { "›" } else { ">" }
    }
    fn icon_done(&self) -> &str {
        if self.use_unicode { "✓" } else { "DONE" }
    }

    /// Create a spinner for long operations. Returns it — caller drives it.
    pub fn spinner(&self, message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        let tick_chars = if self.use_unicode {
            "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
        } else {
            "-\\|/"
        };
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars(tick_chars)
                .template("{spinner} {msg}")
                .expect("valid template"),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        pb
    }

    /// Create a progress bar with known total.
    #[allow(dead_code)]
    pub fn progress_bar(&self, total: u64, message: &str) -> ProgressBar {
        let pb = ProgressBar::new(total);
        let template = if self.use_unicode {
            "{spinner} {msg} {bar:30.cyan/dim} {pos}/{len}"
        } else {
            "{spinner} {msg} [{bar:30}] {pos}/{len}"
        };
        pb.set_style(
            ProgressStyle::default_bar()
                .template(template)
                .expect("valid template")
                .progress_chars("━╸─"),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        pb
    }
}

impl Renderer for TerminalRenderer {
    fn emit(&mut self, event: UiEvent) {
        match event {
            // ── Run ───────────────────────────────────────
            UiEvent::RunStarted {
                provider,
                model,
                request,
                workspace,
            } => {
                let _ = self.term.write_line("");
                let header = format!(
                    "{} {} ({})",
                    style("Run").bold().cyan(),
                    style(&model).bold(),
                    style(&provider).dim()
                );
                let _ = self.term.write_line(&header);
                if let Some(ws) = workspace {
                    let _ = self.term.write_line(&format!(
                        "  {} workspace: {}",
                        self.icon_bullet(),
                        style(ws).cyan()
                    ));
                }
                let _ = self.term.write_line(&format!(
                    "  {} request: {}",
                    self.icon_bullet(),
                    style(&request).white().bold()
                ));
                let _ = self.term.write_line("");
            }

            UiEvent::RunStepCompleted {
                step,
                action_type,
                reason,
                tokens_used,
                tokens_max,
                duration_ms,
            } => {
                let utilization = if tokens_max > 0 {
                    (tokens_used as f64 / tokens_max as f64 * 100.0) as u64
                } else {
                    0
                };

                let action_styled = match action_type.as_str() {
                    "RETRIEVE" => style(&action_type).blue(),
                    "TOOL_CALL" => style(&action_type).magenta(),
                    "VERIFY" => style(&action_type).yellow(),
                    "RESPOND" => style(&action_type).green(),
                    "MEMORY" => style(&action_type).cyan(),
                    "STOP" => style(&action_type).dim(),
                    _ => style(&action_type).white(),
                };

                // Truncate reason to fit terminal (UTF-8 safe)
                let reason_display = if reason.chars().count() > 50 {
                    let truncated: String = reason.chars().take(49).collect();
                    format!("{truncated}…")
                } else {
                    reason
                };

                let _ = self.term.write_line(&format!(
                    "  {:>3} {:<10}  {}  {}  {}",
                    style(format!("[{step}]")).dim(),
                    action_styled,
                    style(&reason_display).white(),
                    style(format!("{duration_ms}ms")).dim(),
                    style(format!("tok {utilization}%")).dim(),
                ));
            }

            UiEvent::RunFinished {
                session_id,
                steps,
                tokens_used,
                tokens_max,
                duration_ms,
                success,
                planning_tokens,
            } => {
                let _ = self.term.write_line("");
                let status = if success {
                    style(format!("{} Done", self.icon_done())).green().bold()
                } else {
                    style(format!("{} Failed", self.icon_fail())).red().bold()
                };
                let _ = self.term.write_line(&format!(
                    "{status}  {steps} steps, {tokens_used}/{tokens_max} tokens, {duration_ms}ms",
                ));
                // Planning overhead display (P2 2.2)
                if planning_tokens > 0 && tokens_used > 0 {
                    let overhead_pct = planning_tokens as f64 / tokens_used as f64 * 100.0;
                    let overhead_str = format!(
                        "  Planning overhead: {:.0}% ({planning_tokens}/{tokens_used} tokens)",
                        overhead_pct
                    );
                    if overhead_pct > 20.0 {
                        let _ = self
                            .term
                            .write_line(&format!("  {} {overhead_str}", style("!").yellow()));
                    } else {
                        let _ = self.term.write_line(&style(overhead_str).dim().to_string());
                    }
                }
                let _ = self
                    .term
                    .write_line(&format!("  session: {}", style(session_id).dim()));
            }

            UiEvent::RunResponse { content } => {
                let _ = self.term.write_line("");
                let _ = self
                    .term
                    .write_line(&format!("{}", style("Response").bold().underlined()));
                let _ = self.term.write_line(&content);
            }

            // ── Index ─────────────────────────────────────
            UiEvent::IndexStarted { path } => {
                // Spinner is handled externally
                let _ = self
                    .term
                    .write_line(&format!("Indexing {}…", style(path.display()).cyan()));
            }

            UiEvent::IndexCompleted {
                files,
                symbols,
                duration_ms,
            } => {
                let _ = self.term.write_line(&format!(
                    "{} {} files, {} symbols ({}ms)",
                    style(self.icon_done()).green().bold(),
                    style(files).bold(),
                    style(symbols).bold(),
                    duration_ms,
                ));
            }

            // ── Search ────────────────────────────────────
            UiEvent::SearchResult {
                rank,
                path,
                score,
                snippet,
            } => {
                let _ = self.term.write_line(&format!(
                    "{}. {} {}",
                    style(rank).bold(),
                    style(&path).cyan(),
                    style(format!("({score:.2})")).dim(),
                ));
                // Indent snippet lines
                for line in snippet.lines() {
                    let _ = self.term.write_line(&format!("   {}", style(line).dim()));
                }
                let _ = self.term.write_line("");
            }

            UiEvent::SearchEmpty { query } => {
                let _ = self.term.write_line(&format!(
                    "{} No results for \"{}\"",
                    self.icon_warn(),
                    style(query).yellow()
                ));
            }

            // ── Doctor ────────────────────────────────────
            UiEvent::DoctorHeader { workspace } => {
                let _ = self.term.write_line(&format!(
                    "{} {}",
                    style("OCO Doctor").bold().cyan(),
                    style(workspace).dim()
                ));
                let _ = self.term.write_line("");
            }

            UiEvent::DoctorCheck {
                name,
                status,
                detail,
            } => {
                let (icon, sty) = match status {
                    CheckStatus::Pass => (self.icon_pass(), Style::new().green()),
                    CheckStatus::Fail => (self.icon_fail(), Style::new().red()),
                    CheckStatus::Warn => (self.icon_warn(), Style::new().yellow()),
                };
                let tag = match status {
                    CheckStatus::Pass => "PASS",
                    CheckStatus::Fail => "FAIL",
                    CheckStatus::Warn => "WARN",
                };
                let detail_str = detail.map(|d| format!(" — {d}")).unwrap_or_default();
                let _ = self.term.write_line(&format!(
                    "  {} {} {}{}",
                    sty.apply_to(icon),
                    sty.apply_to(tag),
                    name,
                    style(detail_str).dim(),
                ));
            }

            UiEvent::DoctorProfile {
                stack,
                build_cmd,
                test_cmd,
            } => {
                let _ = self.term.write_line("");
                let _ = self
                    .term
                    .write_line(&format!("  Stack: {}", style(stack).bold()));
                if let Some(cmd) = build_cmd {
                    let _ = self
                        .term
                        .write_line(&format!("  Build: {}", style(cmd).dim()));
                }
                if let Some(cmd) = test_cmd {
                    let _ = self
                        .term
                        .write_line(&format!("  Test:  {}", style(cmd).dim()));
                }
            }

            UiEvent::DoctorSummary { issues } => {
                let _ = self.term.write_line("");
                if issues == 0 {
                    let _ = self.term.write_line(&format!(
                        "{} All checks passed.",
                        style(self.icon_done()).green().bold()
                    ));
                } else {
                    let _ = self.term.write_line(&format!(
                        "{} {} issue(s) found.",
                        style(self.icon_fail()).red().bold(),
                        issues,
                    ));
                }
            }

            // ── Eval ──────────────────────────────────────
            UiEvent::EvalStarted { scenario_count } => {
                let _ = self.term.write_line(&format!(
                    "Running {} scenarios…",
                    style(scenario_count).bold()
                ));
            }

            UiEvent::EvalScenario {
                name,
                success,
                steps,
                tokens,
                duration_ms,
                tokens_per_step,
            } => {
                let status_icon = if success {
                    style(self.icon_pass()).green()
                } else {
                    style(self.icon_fail()).red()
                };
                let _ = self.term.write_line(&format!(
                    "  {} {:<30} steps={:<3} tok={:<6} {:<6}ms  tok/step={:.0}",
                    status_icon, name, steps, tokens, duration_ms, tokens_per_step,
                ));
            }

            UiEvent::EvalSaved { path } => {
                let _ = self.term.write_line(&format!(
                    "{} Results written to {}",
                    style(self.icon_done()).green().bold(),
                    style(path).cyan(),
                ));
            }

            // ── Serve ─────────────────────────────────────
            UiEvent::ServerListening { host, port } => {
                let _ = self.term.write_line(&format!(
                    "OCO server listening on {}",
                    style(format!("http://{host}:{port}")).cyan().bold()
                ));
            }

            // ── Plan Orchestration ────────────────────────
            UiEvent::PlanOverview {
                step_count,
                parallel_groups,
                critical_path_length,
                estimated_tokens,
                budget_tokens,
                strategy,
                team,
                steps,
            } => {
                let _ = self.term.write_line("");
                let team_str = if let Some((name, topo, count)) = &team {
                    format!(" + {topo} team '{name}' ({count} members)")
                } else {
                    String::new()
                };
                let _ = self.term.write_line(&format!(
                    "  {} Plan: {} steps, {} parallel groups, critical path: {}{}",
                    style("*").cyan().bold(),
                    style(step_count).bold(),
                    style(parallel_groups).bold(),
                    style(critical_path_length).bold(),
                    style(&team_str).dim(),
                ));

                // Build step index for dep name lookup
                let step_names: std::collections::HashMap<&str, &str> =
                    steps.iter().map(|s| (s.id.as_str(), s.name.as_str())).collect();

                // Render each step with deps
                for (i, step) in steps.iter().enumerate() {
                    let model_str = step.preferred_model.as_deref().unwrap_or("default");
                    let verify_marker = if step.verify_after { " +verify" } else { "" };
                    let dep_names: Vec<&str> = step
                        .depends_on
                        .iter()
                        .filter_map(|d| step_names.get(d.as_str()).copied())
                        .collect();
                    let dep_str = if dep_names.is_empty() {
                        String::new()
                    } else {
                        format!("  {} depends: {}", style("<-").dim(), dep_names.join(", "))
                    };

                    let _ = self.term.write_line(&format!(
                        "    {} {:<28} ({}, {}{}){dep_str}",
                        style(format!("[{}]", i + 1)).dim(),
                        style(&step.name).white().bold(),
                        style(model_str).dim(),
                        style(&step.execution_mode).dim(),
                        style(verify_marker).yellow(),
                    ));
                }

                let _ = self.term.write_line(&format!(
                    "    Budget: ~{}k est. / {}k available  Strategy: {}",
                    estimated_tokens / 1000,
                    budget_tokens / 1000,
                    style(&strategy).dim(),
                ));
                let _ = self.term.write_line("");
            }

            UiEvent::PlanStepStarted {
                step_name,
                role,
                execution_mode,
            } => {
                let mode_styled = match execution_mode.as_str() {
                    "inline" => style(&execution_mode).dim(),
                    "subagent" => style(&execution_mode).blue(),
                    "teammate" => style(&execution_mode).magenta(),
                    "mcp_tool" => style(&execution_mode).yellow(),
                    _ => style(&execution_mode).white(),
                };
                let _ = self.term.write_line(&format!(
                    "  {} {:<24} [{}] {}",
                    style(">").cyan(),
                    style(&step_name).white().bold(),
                    style(&role).dim(),
                    mode_styled,
                ));
            }

            UiEvent::PlanStepCompleted {
                step_name,
                success,
                duration_ms,
                tokens_used,
            } => {
                let icon = if success {
                    style(self.icon_done()).green()
                } else {
                    style(self.icon_fail()).red()
                };
                let tok_str = if tokens_used > 0 {
                    format!("  {}tok", tokens_used)
                } else {
                    String::new()
                };
                let _ = self.term.write_line(&format!(
                    "  {} {:<24} {}{}",
                    icon,
                    style(&step_name).white(),
                    style(format!("{duration_ms}ms")).dim(),
                    style(&tok_str).dim(),
                ));
            }

            UiEvent::PlanProgress {
                completed,
                total,
                active_steps,
                budget_used_pct,
            } => {
                let pct = if total > 0 {
                    completed as f32 / total as f32
                } else {
                    0.0
                };
                let filled = (pct * 10.0) as usize;
                let empty = 10_usize.saturating_sub(filled);
                let bar = format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty));
                let active_str = if active_steps.is_empty() {
                    String::new()
                } else {
                    format!("  Active: {}", active_steps.join(", "))
                };
                let _ = self.term.write_line(&format!(
                    "  [{completed}/{total}] {bar} {:.0}%{active_str}  Budget: {budget_used_pct:.0}%",
                    pct * 100.0,
                ));
            }

            UiEvent::PlanVerifyGateResult {
                step_name,
                checks,
                overall_passed,
                replan_triggered,
            } => {
                let icon = if overall_passed {
                    style(self.icon_pass()).green()
                } else {
                    style(self.icon_fail()).red()
                };
                let _ = self.term.write_line(&format!(
                    "  {} Verify [{}]:",
                    icon,
                    style(&step_name).bold(),
                ));
                for (check_type, passed, summary) in &checks {
                    let check_icon = if *passed {
                        style(self.icon_pass()).green()
                    } else {
                        style(self.icon_fail()).red()
                    };
                    let summary_short = if summary.chars().count() > 60 {
                        let truncated: String = summary.chars().take(59).collect();
                        format!("{truncated}…")
                    } else {
                        summary.clone()
                    };
                    let _ = self.term.write_line(&format!(
                        "    {} {}: {}",
                        check_icon,
                        check_type,
                        style(&summary_short).dim(),
                    ));
                }
                if replan_triggered {
                    let _ = self.term.write_line(&format!(
                        "    {} Replan triggered",
                        style("->").yellow().bold(),
                    ));
                }
            }

            UiEvent::PlanReplanTriggered {
                failed_step,
                attempt,
                max_attempts,
                steps_preserved,
                steps_removed,
                steps_added,
            } => {
                let _ = self.term.write_line(&format!(
                    "  {} Replan (attempt {}/{}):",
                    style(self.icon_warn()).yellow().bold(),
                    attempt,
                    max_attempts,
                ));
                let _ = self.term.write_line(&format!(
                    "    Failed: '{}'  Kept: {}  Removed: {}  Added: {}",
                    style(&failed_step).red(),
                    style(steps_preserved).green(),
                    style(steps_removed).red(),
                    style(steps_added).cyan(),
                ));
            }

            UiEvent::TeamStatus {
                team_name,
                members,
                communication,
                completed,
                total,
                messages,
            } => {
                let _ = self.term.write_line(&format!(
                    "\n  {} Team '{}' ({} members, {}) — {}/{} done, {} messages",
                    style("T").magenta().bold(),
                    style(&team_name).bold(),
                    members,
                    style(&communication).dim(),
                    style(completed).green(),
                    total,
                    messages,
                ));
            }

            // ── Trust & Policy ────────────────────────────
            UiEvent::PolicyPackActive { pack } => {
                let _ = self.term.write_line(&format!(
                    "  {} Policy: {}",
                    self.icon_bullet(),
                    style(&pack).cyan().bold(),
                ));
            }

            UiEvent::TrustVerdictFinal { verdict, freshness } => {
                let _ = self.term.write_line("");
                // Map verdict string to symbol + style
                let (symbol, verdict_styled) = match verdict.as_str() {
                    "high" => ("\u{2705}", style(&verdict).green().bold()),
                    "medium" => ("\u{26a0}", style(&verdict).yellow().bold()),
                    "low" => ("\u{274c}", style(&verdict).red().bold()),
                    _ => ("\u{2753}", style(&verdict).dim()),
                };
                let symbol_display = if self.use_unicode { symbol } else { "" };
                let _ = self.term.write_line(&format!(
                    "  Trust: {} {} (freshness: {})",
                    symbol_display,
                    verdict_styled,
                    style(&freshness).dim(),
                ));
            }

            // ── Scorecard Comparison (Q5) ─────────────────
            UiEvent::ScorecardSummary {
                run_id,
                overall_score,
                dimension_count,
            } => {
                let _ = self.term.write_line(&format!(
                    "  {} {}: score={:.2} ({} dimensions)",
                    self.icon_bullet(),
                    style(&run_id).bold(),
                    style(format!("{overall_score:.2}")).cyan(),
                    dimension_count,
                ));
            }

            UiEvent::ComparisonResult {
                baseline_id,
                candidate_id,
                overall_delta,
                regressions,
                improvements,
                verdict,
            } => {
                let _ = self.term.write_line("");
                let (icon, verdict_styled) = match verdict.as_str() {
                    "improved" => (self.icon_pass(), style(&verdict).green().bold()),
                    "stable" => (self.icon_bullet(), style(&verdict).dim()),
                    _ => (self.icon_fail(), style(&verdict).red().bold()),
                };
                let _ = self.term.write_line(&format!(
                    "  {} Compare: {} vs {} -> {} (delta: {:+.2})",
                    icon,
                    style(&baseline_id).dim(),
                    style(&candidate_id).dim(),
                    verdict_styled,
                    overall_delta,
                ));
                let _ = self.term.write_line(&format!(
                    "    {} regression(s), {} improvement(s)",
                    style(regressions).red(),
                    style(improvements).green(),
                ));
            }

            UiEvent::ComparisonDetail {
                dimension,
                baseline_score,
                candidate_score,
                delta,
                kind,
            } => {
                let icon = if kind == "improvement" {
                    style(self.icon_pass()).green()
                } else {
                    style(self.icon_fail()).red()
                };
                let _ = self.term.write_line(&format!(
                    "    {} {:<25} {:.2} -> {:.2} ({:+.2})",
                    icon, dimension, baseline_score, candidate_score, delta,
                ));
            }

            // ── Eval Gate (Q6) ────────────────────────────
            UiEvent::GateHeader {
                baseline_id,
                candidate_id,
                policy,
            } => {
                let _ = self.term.write_line(&format!(
                    "\n  {} Eval Gate: {} vs {}",
                    style(self.icon_bullet()).cyan().bold(),
                    style(&baseline_id).bold(),
                    style(&candidate_id).bold(),
                ));
                let _ = self
                    .term
                    .write_line(&format!("  Policy: {}", style(&policy).dim(),));
                let _ = self.term.write_line("");
                let _ = self.term.write_line(&format!(
                    "  {:<24} {:>8}  {:>9}  {:>6}  {:>5}  {}",
                    "Dimension", "Baseline", "Candidate", "Delta", "Min", "Verdict"
                ));
                let _ = self.term.write_line(
                    "  --------------------------------------------------------------------",
                );
            }

            UiEvent::GateDimensionCheck {
                dimension,
                baseline_score,
                candidate_score,
                delta,
                min_score,
                verdict,
            } => {
                let verdict_styled = match verdict.as_str() {
                    "pass" => style("[PASS]").green(),
                    "warn" => style("[WARN]").yellow(),
                    _ => style("[FAIL]").red().bold(),
                };
                let _ = self.term.write_line(&format!(
                    "  {:<24} {:>8.2}  {:>9.2}  {:>6.2}  {:>5.2}  {}",
                    dimension, baseline_score, candidate_score, delta, min_score, verdict_styled,
                ));
            }

            UiEvent::GateVerdict {
                verdict,
                exit_code,
                reasons,
                failed_count,
                warned_count,
            } => {
                let _ = self.term.write_line("");
                if !reasons.is_empty() {
                    let _ = self
                        .term
                        .write_line(&format!("  Reasons ({}):", reasons.len()));
                    for reason in &reasons {
                        let _ =
                            self.term
                                .write_line(&format!("    {} {}", style("-").dim(), reason,));
                    }
                    let _ = self.term.write_line("");
                }
                let (icon, verdict_styled) = match verdict.as_str() {
                    "pass" => (self.icon_pass(), style(&verdict).green().bold()),
                    "warn" => (self.icon_warn(), style(&verdict).yellow().bold()),
                    _ => (self.icon_fail(), style(&verdict).red().bold()),
                };
                let _ = self.term.write_line(&format!(
                    "  {} Verdict: {} ({} failed, {} warned, exit code {})",
                    icon,
                    verdict_styled,
                    style(failed_count).red(),
                    style(warned_count).yellow(),
                    exit_code,
                ));
            }

            // ── Baseline Freshness (Q8) ──────────────────
            UiEvent::BaselineFreshness {
                freshness,
                age_days,
                recommendation,
            } => {
                let (icon, freshness_styled) = match freshness.as_str() {
                    "fresh" => (self.icon_pass(), style(&freshness).green()),
                    "aging" => (self.icon_warn(), style(&freshness).yellow()),
                    "stale" => (self.icon_fail(), style(&freshness).red().bold()),
                    _ => (self.icon_bullet(), style(&freshness).dim()),
                };
                let age_str = match age_days {
                    Some(d) => format!("{d:.1} days old"),
                    None => "age unknown".to_string(),
                };
                let _ = self.term.write_line(&format!(
                    "  {} Baseline freshness: {} ({})",
                    icon, freshness_styled, age_str,
                ));
                let _ = self
                    .term
                    .write_line(&format!("    {}", style(&recommendation).dim(),));
            }

            // ── Review Packet (Q9) ────────────────────────
            UiEvent::ReviewPacketHeader {
                run_id,
                merge_readiness,
                trust_verdict,
                gate_verdict,
            } => {
                let _ = self.term.write_line("");
                let _ = self.term.write_line(&format!(
                    "  {} OCO Review Packet: {}",
                    style(self.icon_bullet()).cyan().bold(),
                    style(&run_id).bold(),
                ));
                let readiness_styled = match merge_readiness.as_str() {
                    "ready" => style(&merge_readiness).green().bold(),
                    "conditionally_ready" => style(&merge_readiness).yellow().bold(),
                    "not_ready" => style(&merge_readiness).red().bold(),
                    _ => style(&merge_readiness).dim(),
                };
                let _ = self
                    .term
                    .write_line(&format!("    Merge readiness: {}", readiness_styled,));
                if let Some(tv) = trust_verdict {
                    let tv_styled = match tv.as_str() {
                        "high" => style(&tv).green(),
                        "medium" => style(&tv).yellow(),
                        "low" => style(&tv).red(),
                        _ => style(&tv).dim(),
                    };
                    let _ = self.term.write_line(&format!("    Trust: {}", tv_styled));
                }
                if let Some(gv) = gate_verdict {
                    let gv_styled = match gv.as_str() {
                        "pass" => style(&gv).green(),
                        "warn" => style(&gv).yellow(),
                        "fail" => style(&gv).red().bold(),
                        _ => style(&gv).dim(),
                    };
                    let _ = self.term.write_line(&format!("    Gate: {}", gv_styled));
                }
            }

            UiEvent::ReviewPacketScorecard {
                overall_score,
                dimensions,
            } => {
                let _ = self.term.write_line(&format!(
                    "\n    {} Scorecard (overall: {})",
                    self.icon_bullet(),
                    style(format!("{overall_score:.2}")).bold(),
                ));
                for (dim, score) in dimensions {
                    let _ = self
                        .term
                        .write_line(&format!("      {:<24} {:.2}", dim, score,));
                }
            }

            UiEvent::ReviewPacketChanges {
                modified_files,
                key_decisions,
                narrative,
            } => {
                if !modified_files.is_empty() {
                    let _ = self.term.write_line(&format!(
                        "\n    {} Changes ({} files)",
                        self.icon_bullet(),
                        modified_files.len(),
                    ));
                    for f in &modified_files {
                        let _ = self.term.write_line(&format!("      - {f}"));
                    }
                }
                if !key_decisions.is_empty() {
                    let _ = self
                        .term
                        .write_line(&format!("\n    {} Key decisions", self.icon_bullet(),));
                    for d in &key_decisions {
                        let _ = self.term.write_line(&format!("      - {d}"));
                    }
                }
                if let Some(n) = narrative {
                    let _ = self.term.write_line(&format!(
                        "\n    {} Narrative: {}",
                        self.icon_bullet(),
                        style(&n).dim(),
                    ));
                }
            }

            UiEvent::ReviewPacketRisks {
                risks,
                open_questions,
                unavailable_data,
            } => {
                if !risks.is_empty() || !open_questions.is_empty() || !unavailable_data.is_empty() {
                    let _ = self
                        .term
                        .write_line(&format!("\n    {} Open risks", self.icon_bullet()));
                    for r in &risks {
                        let _ = self
                            .term
                            .write_line(&format!("      {} {}", style("!").red(), r,));
                    }
                    for q in &open_questions {
                        let _ =
                            self.term
                                .write_line(&format!("      {} {}", style("?").yellow(), q,));
                    }
                    for u in &unavailable_data {
                        let _ = self
                            .term
                            .write_line(&format!("      {} {}", style("~").dim(), u,));
                    }
                }
            }

            // ── Generic ───────────────────────────────────
            UiEvent::Info { message } => {
                let _ = self.term.write_line(&message);
            }
            UiEvent::Success { message } => {
                let _ = self.term.write_line(&format!(
                    "{} {}",
                    style(self.icon_done()).green().bold(),
                    message,
                ));
            }
            UiEvent::Warning { message } => {
                let _ = self.term.write_line(&format!(
                    "{} {}",
                    style(self.icon_warn()).yellow().bold(),
                    message,
                ));
            }
            UiEvent::Error { message } => {
                let _ = self.term.write_line(&format!(
                    "{} {}",
                    style(self.icon_fail()).red().bold(),
                    message,
                ));
            }
        }
    }
}
