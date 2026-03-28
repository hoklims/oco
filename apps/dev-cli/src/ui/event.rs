use std::path::PathBuf;

use uuid::Uuid;

/// Summary of a step for DAG display in the plan overview.
#[derive(Debug, Clone)]
pub struct UiStepSummary {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub execution_mode: String,
    pub depends_on: Vec<Uuid>,
    pub verify_after: bool,
    pub estimated_tokens: u32,
    pub preferred_model: Option<String>,
}

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

    // ── Plan Orchestration ────────────────────────────────
    /// Full plan overview with DAG structure — shown once at plan start.
    PlanOverview {
        step_count: usize,
        parallel_groups: usize,
        critical_path_length: u32,
        estimated_tokens: u32,
        budget_tokens: u64,
        strategy: String,
        team: Option<(String, String, usize)>, // (name, topology, member_count)
        steps: Vec<UiStepSummary>,
    },

    /// A plan step started executing.
    PlanStepStarted {
        step_name: String,
        role: String,
        execution_mode: String,
    },

    /// A plan step completed (success or failure).
    PlanStepCompleted {
        step_name: String,
        success: bool,
        duration_ms: u64,
        tokens_used: u64,
    },

    /// Live progress bar during plan execution.
    PlanProgress {
        completed: usize,
        total: usize,
        active_steps: Vec<String>,
        budget_used_pct: f32,
    },

    /// A verify gate was evaluated.
    PlanVerifyGateResult {
        step_name: String,
        checks: Vec<(String, bool, String)>, // (check_type, passed, summary)
        overall_passed: bool,
        replan_triggered: bool,
    },

    /// Replanning triggered — shows what changed.
    PlanReplanTriggered {
        failed_step: String,
        attempt: u32,
        max_attempts: u32,
        steps_preserved: usize,
        steps_removed: usize,
        steps_added: usize,
    },

    TeamStatus {
        team_name: String,
        members: u32,
        communication: String,
        completed: u32,
        total: u32,
        messages: u32,
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
