use super::Renderer;
use super::event::UiEvent;

/// Quiet renderer: only emits high-level results
/// (RunFinished, RunResponse, DoctorSummary, IndexCompleted, Error).
/// Everything else is silently discarded.
pub struct QuietRenderer {
    term: console::Term,
}

impl QuietRenderer {
    pub fn new() -> Self {
        Self {
            term: console::Term::stderr(),
        }
    }
}

impl Renderer for QuietRenderer {
    fn emit(&mut self, event: UiEvent) {
        match event {
            UiEvent::RunFinished {
                session_id,
                steps,
                tokens_used,
                tokens_max,
                duration_ms,
                success,
                planning_tokens: _,
            } => {
                let status = if success { "ok" } else { "fail" };
                let _ = self.term.write_line(&format!(
                    "{status} {session_id} {steps}steps {tokens_used}/{tokens_max}tok {duration_ms}ms"
                ));
            }
            UiEvent::RunResponse { content } => {
                // Final response always goes to stdout
                println!("{content}");
            }
            UiEvent::DoctorSummary { issues } if issues > 0 => {
                let _ = self.term.write_line(&format!("{issues} issue(s)"));
            }
            UiEvent::IndexCompleted {
                files,
                symbols,
                duration_ms,
            } => {
                let _ = self.term.write_line(&format!(
                    "{files} files, {symbols} symbols, {duration_ms}ms"
                ));
            }
            UiEvent::Error { message } => {
                let _ = self.term.write_line(&format!("error: {message}"));
            }
            _ => {} // Silently discard everything else
        }
    }
}
