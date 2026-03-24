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

                // Truncate reason to fit terminal
                let reason_display = if reason.len() > 50 {
                    format!("{}…", &reason[..49])
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
