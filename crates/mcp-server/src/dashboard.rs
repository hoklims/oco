//! Dashboard API — SSE event stream + REST replay control.
//!
//! Implements the GPT-5.4-reviewed architecture:
//! - SSE for data plane (live session events + replay stream)
//! - REST for control plane (create/pause/resume/seek/speed replays)
//! - Cursor-based reconnect via `?after_seq=N`
//! - Heartbeat keepalive every 15s

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use oco_orchestrator_core::replay::LoadedTrace;
use oco_shared_types::dashboard::DashboardEvent;

use crate::server::AppState;

// ── Request / response types ─────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateReplayRequest {
    /// Path to the run directory (`.oco/runs/<id>/`).
    pub run_dir: String,
    /// Initial playback speed (default: 1.0).
    #[serde(default = "default_speed")]
    pub speed: f64,
}

fn default_speed() -> f64 {
    1.0
}

#[derive(Debug, Serialize)]
pub struct ReplayResponse {
    pub replay_id: String,
    pub event_count: usize,
    pub stream_url: String,
}

#[derive(Debug, Deserialize)]
pub struct SpeedRequest {
    pub speed: f64,
}

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    /// Resume from this sequence number (cursor-based reconnect).
    #[serde(default)]
    pub after_seq: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotResponse {
    pub events: Vec<DashboardEvent>,
    pub current_seq: u64,
}

#[derive(Debug, Serialize)]
pub struct ReplayListItem {
    pub replay_id: String,
    pub event_count: usize,
}

// ── Router ───────────────────────────────────────────────────

/// Dashboard sub-router. Mounted under `/api/v1/dashboard`.
pub fn dashboard_router() -> Router<Arc<AppState>> {
    Router::new()
        // Run history (scans .oco/runs/ on disk)
        .route("/runs", get(list_runs))
        // Replay CRUD + control
        .route("/replays", post(create_replay).get(list_replays))
        .route("/replays/{replay_id}/stream", get(replay_stream))
        .route("/replays/{replay_id}/snapshot", get(replay_snapshot))
        .route("/replays/{replay_id}/pause", post(pause_replay))
        .route("/replays/{replay_id}/resume", post(resume_replay))
        .route("/replays/{replay_id}/speed", post(set_speed))
        .route("/replays/{replay_id}", axum::routing::delete(delete_replay))
}

// ── Run history types ────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RunSummary {
    pub id: String,
    pub request: String,
    pub status: String,
    pub complexity: String,
    pub steps: u32,
    pub tokens_used: u64,
    pub tokens_max: u64,
    pub duration_ms: u64,
    pub success: bool,
    pub created_at: String,
    pub run_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    /// Workspace root to scan for .oco/runs/. Defaults to ".".
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_workspace() -> String {
    ".".into()
}

fn default_limit() -> usize {
    20
}

// ── Handlers ─────────────────────────────────────────────────

/// `GET /api/v1/dashboard/runs` — list recent runs from disk.
async fn list_runs(
    Query(query): Query<RunsQuery>,
) -> Json<Vec<RunSummary>> {
    // Try the provided workspace, then CWD.
    let workspace = std::path::Path::new(&query.workspace);
    let workspace = if workspace.join(".oco").exists() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| workspace.to_path_buf())
    };
    let runs_dir = workspace.join(".oco").join("runs");

    let mut runs = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&runs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let summary_path = path.join("summary.json");
            if !summary_path.exists() {
                continue;
            }
            if let Ok(val) = std::fs::read_to_string(&summary_path)
                .ok()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                .ok_or(())
            {
                runs.push(RunSummary {
                    id: val["session_id"].as_str().unwrap_or("?").into(),
                    request: val["request"].as_str().unwrap_or("").into(),
                    status: val["status"].as_str().unwrap_or("unknown").into(),
                    complexity: val["complexity"].as_str().unwrap_or("?").into(),
                    steps: val["steps"].as_u64().unwrap_or(0) as u32,
                    tokens_used: val["tokens_used"].as_u64().unwrap_or(0),
                    tokens_max: val["tokens_max"].as_u64().unwrap_or(0),
                    duration_ms: val["duration_ms"].as_u64().unwrap_or(0),
                    success: val["success"].as_bool().unwrap_or(false),
                    created_at: val["created_at"].as_str().unwrap_or("").into(),
                    run_dir: path.to_string_lossy().into(),
                });
            }
        }
    }

    // Sort by created_at descending.
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    runs.truncate(query.limit);

    Json(runs)
}

/// `POST /api/v1/dashboard/replays` — create a new replay session.
async fn create_replay(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateReplayRequest>,
) -> Result<(StatusCode, Json<ReplayResponse>), (StatusCode, Json<serde_json::Value>)> {
    let run_dir = PathBuf::from(&req.run_dir);

    let trace = LoadedTrace::from_run_dir(&run_dir).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    let (replay_id, session) = state.replay_registry.create(&trace).await;

    // Set initial speed.
    if req.speed != 1.0 {
        session.controls().set_speed(req.speed);
    }

    let event_count = session.event_count();

    // Spawn the replay loop in the background.
    let session_for_run = Arc::clone(&session);
    tokio::spawn(async move {
        session_for_run.run().await;
    });

    Ok((
        StatusCode::CREATED,
        Json(ReplayResponse {
            replay_id: replay_id.to_string(),
            event_count,
            stream_url: format!("/api/v1/dashboard/replays/{replay_id}/stream"),
        }),
    ))
}

/// `GET /api/v1/dashboard/replays` — list active replays.
async fn list_replays(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<ReplayListItem>> {
    let mut items = Vec::new();
    for id in state.replay_registry.list().await {
        if let Some(session) = state.replay_registry.get(&id).await {
            items.push(ReplayListItem {
                replay_id: id.to_string(),
                event_count: session.event_count(),
            });
        }
    }
    Json(items)
}

/// `GET /api/v1/dashboard/replays/{replay_id}/stream` — SSE event stream.
///
/// Supports cursor-based reconnect via `?after_seq=N` query parameter
/// and the standard `Last-Event-ID` header.
async fn replay_stream(
    State(state): State<Arc<AppState>>,
    Path(replay_id): Path<String>,
    Query(query): Query<StreamQuery>,
    headers: axum::http::HeaderMap,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
    (StatusCode, Json<serde_json::Value>),
> {
    let id = parse_uuid(&replay_id)?;
    let session = get_session(&state, &id).await?;

    // Determine cursor: prefer query param, then Last-Event-ID header.
    let after_seq = query.after_seq.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    });

    // First, send all pre-existing events (catch-up).
    // On first connect (no cursor): send everything (replay may already be done).
    // On reconnect (with cursor): send only what was missed.
    let catchup: Vec<DashboardEvent> = if let Some(seq) = after_seq {
        session.events_after_seq(seq).to_vec()
    } else {
        session.events().to_vec()
    };

    // Then subscribe to live broadcast for new events.
    let mut rx = session.subscribe();

    let stream = async_stream::stream! {
        // Phase 1: catch-up events.
        for event in catchup {
            let json = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(Event::default()
                .event(event_type_name(&event.kind))
                .id(event.seq.to_string())
                .data(json));
        }

        // Phase 2: live broadcast events.
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok(Event::default()
                        .event(event_type_name(&event.kind))
                        .id(event.seq.to_string())
                        .data(json));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // Client too slow — send a warning event.
                    yield Ok(Event::default()
                        .event("lagged")
                        .data(format!("{{\"skipped\": {n}}}")));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // Replay finished.
                    yield Ok(Event::default()
                        .event("finished")
                        .data("{}"));
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("heartbeat"),
    ))
}

/// `GET /api/v1/dashboard/replays/{replay_id}/snapshot` — get all events up to now.
async fn replay_snapshot(
    State(state): State<Arc<AppState>>,
    Path(replay_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<Json<SnapshotResponse>, (StatusCode, Json<serde_json::Value>)> {
    let id = parse_uuid(&replay_id)?;
    let session = get_session(&state, &id).await?;

    let events = if let Some(seq) = query.after_seq {
        session.events_after_seq(seq).to_vec()
    } else {
        session.events().to_vec()
    };

    let current_seq = events.last().map(|e| e.seq).unwrap_or(0);

    Ok(Json(SnapshotResponse {
        events,
        current_seq,
    }))
}

/// `POST /api/v1/dashboard/replays/{replay_id}/pause`
async fn pause_replay(
    State(state): State<Arc<AppState>>,
    Path(replay_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let id = parse_uuid(&replay_id)?;
    let session = get_session(&state, &id).await?;
    session.controls().pause();
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/dashboard/replays/{replay_id}/resume`
async fn resume_replay(
    State(state): State<Arc<AppState>>,
    Path(replay_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let id = parse_uuid(&replay_id)?;
    let session = get_session(&state, &id).await?;
    session.controls().resume();
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/dashboard/replays/{replay_id}/speed`
async fn set_speed(
    State(state): State<Arc<AppState>>,
    Path(replay_id): Path<String>,
    Json(req): Json<SpeedRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let id = parse_uuid(&replay_id)?;
    let session = get_session(&state, &id).await?;
    session.controls().set_speed(req.speed);
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/dashboard/replays/{replay_id}`
async fn delete_replay(
    State(state): State<Arc<AppState>>,
    Path(replay_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let id = parse_uuid(&replay_id)?;
    state.replay_registry.remove(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── Helpers ──────────────────────────────────────────────────

fn parse_uuid(s: &str) -> Result<Uuid, (StatusCode, Json<serde_json::Value>)> {
    Uuid::parse_str(s).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid UUID: {s}") })),
        )
    })
}

async fn get_session(
    state: &AppState,
    id: &Uuid,
) -> Result<Arc<oco_orchestrator_core::ReplaySession>, (StatusCode, Json<serde_json::Value>)> {
    state.replay_registry.get(id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "replay not found" })),
        )
    })
}

/// Extract the event type name for the SSE `event:` field.
fn event_type_name(kind: &oco_shared_types::dashboard::DashboardEventKind) -> &'static str {
    use oco_shared_types::dashboard::DashboardEventKind;
    match kind {
        DashboardEventKind::RunStarted { .. } => "run_started",
        DashboardEventKind::RunStopped { .. } => "run_stopped",
        DashboardEventKind::PlanExploration { .. } => "plan_exploration",
        DashboardEventKind::PlanGenerated { .. } => "plan_generated",
        DashboardEventKind::StepStarted { .. } => "step_started",
        DashboardEventKind::StepCompleted { .. } => "step_completed",
        DashboardEventKind::FlatStepCompleted { .. } => "flat_step_completed",
        DashboardEventKind::Progress { .. } => "progress",
        DashboardEventKind::VerifyGateResult { .. } => "verify_gate_result",
        DashboardEventKind::ReplanTriggered { .. } => "replan_triggered",
        DashboardEventKind::BudgetWarning { .. } => "budget_warning",
        DashboardEventKind::BudgetSnapshot(_) => "budget_snapshot",
        DashboardEventKind::IndexProgress { .. } => "index_progress",
        DashboardEventKind::Heartbeat => "heartbeat",
    }
}
