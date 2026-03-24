use std::path::PathBuf;

/// Structured events emitted by CLI commands.
/// The core never knows how these are rendered — that's the renderer's job.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum UiEvent {
    // ── Run ───────────────────────────────────────────────
    RunStarted {
        provider: String,
        model: String,
        request: String,
        workspace: Option<String>,
    },
    RunStepCompleted {
        step: u32,
        action_type: String,
        reason: String,
        tokens_used: u64,
        tokens_max: u64,
        duration_ms: u64,
    },
    RunFinished {
        session_id: String,
        steps: u32,
        tokens_used: u64,
        tokens_max: u64,
        duration_ms: u64,
        success: bool,
    },
    RunResponse {
        content: String,
    },

    // ── Index ─────────────────────────────────────────────
    IndexStarted {
        path: PathBuf,
    },
    IndexCompleted {
        files: u32,
        symbols: u32,
        duration_ms: u64,
    },

    // ── Search ────────────────────────────────────────────
    SearchResult {
        rank: usize,
        path: String,
        score: f64,
        snippet: String,
    },
    SearchEmpty {
        query: String,
    },

    // ── Doctor ────────────────────────────────────────────
    DoctorHeader {
        workspace: String,
    },
    DoctorCheck {
        name: String,
        status: CheckStatus,
        detail: Option<String>,
    },
    DoctorProfile {
        stack: String,
        build_cmd: Option<String>,
        test_cmd: Option<String>,
    },
    DoctorSummary {
        issues: u32,
    },

    // ── Eval ──────────────────────────────────────────────
    EvalStarted {
        scenario_count: usize,
    },
    EvalScenario {
        name: String,
        success: bool,
        steps: u32,
        tokens: u64,
        duration_ms: u64,
        tokens_per_step: f64,
    },
    EvalSaved {
        path: String,
    },

    // ── Serve ─────────────────────────────────────────────
    ServerListening {
        host: String,
        port: u16,
    },

    // ── Generic ───────────────────────────────────────────
    Info {
        message: String,
    },
    Success {
        message: String,
    },
    Warning {
        message: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
}
