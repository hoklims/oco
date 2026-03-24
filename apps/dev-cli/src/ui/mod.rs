pub mod event;
pub mod jsonl;
pub mod quiet;
pub mod terminal;

pub use event::{CheckStatus, UiEvent};

/// Output format for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Jsonl,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Self {
        match s {
            "json" => Self::Json,
            "jsonl" => Self::Jsonl,
            _ => Self::Human,
        }
    }
}

/// Trait for rendering UI events. Decouples what happened from how it's shown.
pub trait Renderer {
    fn emit(&mut self, event: UiEvent);
}

/// Create the appropriate renderer based on output format.
pub fn create_renderer(format: OutputFormat) -> Box<dyn Renderer> {
    match format {
        OutputFormat::Human => Box::new(terminal::TerminalRenderer::new()),
        OutputFormat::Json | OutputFormat::Jsonl => Box::new(jsonl::JsonlRenderer::new()),
    }
}
