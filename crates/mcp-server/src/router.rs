//! HTTP/REST API router for the OCO server.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use oco_code_intel::{CodeParser, CompositeParser, language_from_path};
use oco_orchestrator_core::runtime::OrchestratorRuntime;
use oco_retrieval::{CallGraphIndex, StoredCallEdge};

use crate::server::AppState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StartSessionRequest {
    pub user_request: String,
    pub workspace_root: Option<String>,
    /// External session ID for correlation (e.g. Claude Code session).
    pub external_session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub status: String,
    pub steps: u32,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub active_sessions: u32,
    pub max_sessions: u32,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct IndexRequest {
    pub workspace_root: String,
}

#[derive(Debug, Serialize)]
pub struct IndexResponse {
    pub status: String,
    pub workspace: String,
    pub files_indexed: u32,
    pub symbols_indexed: u32,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub workspace_root: String,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    10
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub path: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn create_router(state: Arc<AppState>) -> Router {
    // Claude Code HTTP hooks (v2.1.63+) — isolated sub-router with auth + body limit.
    let hooks = crate::hooks::hook_router(Arc::clone(&state));

    let mut router = Router::new()
        .route("/health", get(health))
        .route("/api/v1/sessions", post(start_session).get(list_sessions))
        .route("/api/v1/sessions/{session_id}", get(get_session))
        .route("/api/v1/sessions/{session_id}/stop", post(stop_session))
        .route("/api/v1/sessions/{session_id}/trace", get(get_trace))
        .route(
            "/api/v1/sessions/{session_id}/hooks",
            get(get_session_hooks),
        )
        .route(
            "/api/v1/sessions/{session_id}/summary",
            get(get_session_summary),
        )
        .route(
            "/api/v1/sessions/{session_id}/snapshot",
            get(get_session_snapshot),
        )
        .route(
            "/api/v1/sessions/{session_id}/mission",
            get(get_session_mission),
        )
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/index", post(index_workspace))
        .route("/api/v1/search", post(search_workspace))
        .route("/api/v1/mcp", post(mcp_handler))
        .nest("/api/v1/hooks", hooks)
        .nest("/api/v1/dashboard", crate::dashboard::dashboard_router());

    // Serve the dashboard static files if the dist directory exists.
    if let Some(ref dir) = state.dashboard_dir.as_ref().filter(|d| d.exists()) {
        tracing::info!(path = %dir.display(), "serving dashboard at /dashboard");
        router = router.nest_service(
            "/dashboard",
            tower_http::services::ServeDir::new(dir)
                .fallback(tower_http::services::ServeFile::new(dir.join("index.html"))),
        );
    }

    router.with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "service": "oco-core"
    }))
}

/// `POST /api/v1/sessions` — create a new orchestration session.
async fn start_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartSessionRequest>,
) -> impl IntoResponse {
    tracing::info!(
        request = %req.user_request,
        workspace = ?req.workspace_root,
        "Creating new session"
    );

    match state.session_manager.create_session(
        &req.user_request,
        req.workspace_root.as_deref(),
        req.external_session_id.as_deref(),
    ) {
        Ok(session_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": session_id,
                "status": "active",
                "steps": 0,
            })),
        ),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": e.to_string(),
            })),
        ),
    }
}

/// `GET /api/v1/sessions` — list all sessions.
async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let sessions = state.session_manager.list_sessions().await;
    Json(serde_json::json!({ "sessions": sessions }))
}

/// `GET /api/v1/sessions/{id}` — get session info.
async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.session_manager.get_session(&session_id).await {
        Some(info) => (
            StatusCode::OK,
            Json(serde_json::to_value(info).expect("SessionInfo serialization")),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("session not found: {session_id}") })),
        ),
    }
}

/// `POST /api/v1/sessions/{id}/stop` — cancel a session.
async fn stop_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.session_manager.stop_session(&session_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "cancelled", "id": session_id })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// `GET /api/v1/sessions/{id}/trace` — get decision traces.
async fn get_trace(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.session_manager.get_trace(&session_id).await {
        Ok(traces) => (
            StatusCode::OK,
            Json(serde_json::json!({ "traces": traces })),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// `GET /api/v1/sessions/{id}/hooks` — get hook events received during this session.
async fn get_session_hooks(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.session_manager.get_hook_events(&session_id).await {
        Ok(events) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "session_id": session_id,
                "count": events.len(),
                "events": events,
            })),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// `GET /api/v1/sessions/{id}/summary` — get a high-level run summary.
async fn get_session_summary(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.session_manager.get_run_summary(&session_id).await {
        Some(summary) => (
            StatusCode::OK,
            Json(serde_json::to_value(summary).unwrap_or_default()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("session not found: {session_id}") })),
        ),
    }
}

/// `GET /api/v1/sessions/{id}/snapshot` — get the latest compact snapshot.
async fn get_session_snapshot(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state
        .session_manager
        .get_typed_compact_snapshot(&session_id)
        .await
    {
        Some(snapshot) => (
            StatusCode::OK,
            Json(serde_json::to_value(snapshot).unwrap_or_default()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no snapshot for session: {session_id}") })),
        ),
    }
}

/// `GET /api/v1/sessions/{id}/mission` — get mission memory for handoff/resume.
async fn get_session_mission(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    // Resolve session ID (supports both OCO UUID and external Claude Code session ID).
    let resolved_id = match state.session_manager.resolve_session_id(&session_id).await {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("session not found: {session_id}") })),
            );
        }
    };

    match state.session_manager.get_mission_memory(&resolved_id).await {
        Some(mm) => (
            StatusCode::OK,
            Json(serde_json::to_value(mm).unwrap_or_default()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": format!("no mission memory for session: {session_id}") }),
            ),
        ),
    }
}

/// `GET /api/v1/status` — overall server status.
async fn get_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let active = state.session_manager.active_count().await;
    Json(StatusResponse {
        status: if active > 0 {
            "busy".into()
        } else {
            "idle".into()
        },
        active_sessions: active,
        max_sessions: state.config.max_concurrent_sessions,
        version: "0.1.0".into(),
    })
}

/// `POST /api/v1/index` — index a workspace using `OrchestratorRuntime`.
async fn index_workspace(Json(req): Json<IndexRequest>) -> impl IntoResponse {
    tracing::info!(workspace = %req.workspace_root, "Indexing workspace");

    // Blocking I/O: offload to the blocking thread pool.
    let workspace = req.workspace_root.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut rt = OrchestratorRuntime::new(PathBuf::from(&workspace));
        rt.index_workspace()
    })
    .await;

    match result {
        Ok(Ok(idx)) => (
            StatusCode::OK,
            Json(
                serde_json::to_value(IndexResponse {
                    status: "indexed".into(),
                    workspace: req.workspace_root,
                    files_indexed: idx.file_count,
                    symbols_indexed: idx.symbol_count,
                })
                .expect("IndexResponse serialization"),
            ),
        ),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("indexing failed: {e}") })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("task panicked: {e}") })),
        ),
    }
}

/// `POST /api/v1/search` — search an indexed workspace.
async fn search_workspace(Json(req): Json<SearchRequest>) -> impl IntoResponse {
    let workspace = req.workspace_root.clone();
    let query = req.query.clone();
    let limit = req.limit;

    let result = tokio::task::spawn_blocking(move || {
        let mut rt = OrchestratorRuntime::new(PathBuf::from(&workspace));
        // Must index first to be able to search.
        rt.index_workspace()?;
        rt.search(&query, limit)
    })
    .await;

    match result {
        Ok(Ok(hits)) => {
            let results: Vec<SearchHit> = hits
                .into_iter()
                .map(|h| SearchHit {
                    path: h.path,
                    snippet: h.snippet,
                    score: h.score,
                })
                .collect();
            (
                StatusCode::OK,
                Json(
                    serde_json::to_value(SearchResponse { results })
                        .expect("SearchResponse serialization"),
                ),
            )
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("search failed: {e}") })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("task panicked: {e}") })),
        ),
    }
}

/// `POST /api/v1/mcp` — JSON-RPC MCP handler.
///
/// Tool calls are resolved against live `AppState` so they return real data
/// instead of hardcoded stubs.
async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<crate::protocol::JsonRpcRequest>,
) -> Json<crate::protocol::JsonRpcResponse> {
    let response = match request.method.as_str() {
        "initialize" => crate::handlers::handle_initialize(request.id),
        "tools/list" => crate::handlers::handle_tools_list(request.id),
        "resources/list" => crate::handlers::handle_resources_list(request.id),
        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            handle_mcp_tool_call(request.id, tool_name, &arguments, &state).await
        }
        _ => crate::protocol::JsonRpcResponse::error(
            request.id,
            -32601,
            format!("Method not found: {}", request.method),
        ),
    };
    Json(response)
}

/// Resolve MCP tool calls against live application state.
async fn handle_mcp_tool_call(
    id: serde_json::Value,
    tool_name: &str,
    arguments: &serde_json::Value,
    state: &AppState,
) -> crate::protocol::JsonRpcResponse {
    match tool_name {
        "oco_orchestrate" => {
            let request = arguments
                .get("request")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let workspace = arguments.get("workspace_root").and_then(|v| v.as_str());
            let ext_sid = arguments
                .get("external_session_id")
                .and_then(|v| v.as_str());

            match state
                .session_manager
                .create_session(request, workspace, ext_sid)
            {
                Ok(session_id) => crate::protocol::JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Orchestration session created: {session_id}. Use oco_status to check progress.")
                        }]
                    }),
                ),
                Err(e) => crate::protocol::JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Failed to create session: {e}")
                        }],
                        "isError": true
                    }),
                ),
            }
        }
        "oco_status" => {
            let active = state.session_manager.active_count().await;
            let sessions = state.session_manager.list_sessions().await;
            let status_json = serde_json::json!({
                "status": if active > 0 { "busy" } else { "idle" },
                "active_sessions": active,
                "max_sessions": state.config.max_concurrent_sessions,
                "sessions": sessions,
            });
            crate::protocol::JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": status_json.to_string()
                    }]
                }),
            )
        }
        "oco_trace" => {
            let session_id = arguments
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match state.session_manager.get_trace(session_id).await {
                Ok(traces) => {
                    let traces_json =
                        serde_json::to_string(&traces).unwrap_or_else(|_| "[]".into());
                    crate::protocol::JsonRpcResponse::success(
                        id,
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": traces_json
                            }]
                        }),
                    )
                }
                Err(e) => crate::protocol::JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error: {e}")
                        }],
                        "isError": true
                    }),
                ),
            }
        }
        "oco_search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as u32;

            // Search requires indexing, which is blocking I/O.
            let result = tokio::task::spawn_blocking(move || {
                let mut rt = OrchestratorRuntime::new(PathBuf::from("."));
                rt.index_workspace()?;
                rt.search(&query, limit)
            })
            .await;

            let text = match result {
                Ok(Ok(hits)) => serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into()),
                Ok(Err(e)) => format!("Search error: {e}"),
                Err(e) => format!("Task error: {e}"),
            };

            crate::protocol::JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": text
                    }]
                }),
            )
        }
        "oco_routes" => {
            let symbol = arguments
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let direction = arguments
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("both")
                .to_string();
            let max_depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as u32;

            if symbol.is_empty() {
                return crate::protocol::JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": "Error: symbol is required" }],
                        "isError": true
                    }),
                );
            }

            let result = tokio::task::spawn_blocking(move || {
                let graph = build_call_graph_for_workspace(&PathBuf::from("."))?;
                let mut response = serde_json::Map::new();
                response.insert("symbol".into(), serde_json::json!(symbol));

                if direction == "callers" || direction == "both" {
                    let callers = graph.routes_callers(&symbol, max_depth)?;
                    response.insert(
                        "callers".into(),
                        serde_json::to_value(&callers).unwrap_or_default(),
                    );
                }
                if direction == "callees" || direction == "both" {
                    let callees = graph.routes_callees(&symbol, max_depth)?;
                    response.insert(
                        "callees".into(),
                        serde_json::to_value(&callees).unwrap_or_default(),
                    );
                }

                Ok::<_, anyhow::Error>(serde_json::Value::Object(response))
            })
            .await;

            let text = match result {
                Ok(Ok(val)) => serde_json::to_string_pretty(&val).unwrap_or_else(|_| "{}".into()),
                Ok(Err(e)) => format!("Error: {e}"),
                Err(e) => format!("Task error: {e}"),
            };

            crate::protocol::JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{ "type": "text", "text": text }]
                }),
            )
        }
        "oco_impact" => {
            let symbol = arguments
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let max_depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as u32;

            if symbol.is_empty() {
                return crate::protocol::JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": "Error: symbol is required" }],
                        "isError": true
                    }),
                );
            }

            let result = tokio::task::spawn_blocking(move || {
                let graph = build_call_graph_for_workspace(&PathBuf::from("."))?;
                let impact = graph.impact(&symbol, max_depth)?;
                serde_json::to_string_pretty(&impact).map_err(|e| anyhow::anyhow!(e))
            })
            .await;

            let text = match result {
                Ok(Ok(json)) => json,
                Ok(Err(e)) => format!("Error: {e}"),
                Err(e) => format!("Task error: {e}"),
            };

            crate::protocol::JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{ "type": "text", "text": text }]
                }),
            )
        }
        _ => crate::protocol::JsonRpcResponse::error(
            id,
            -32601,
            format!("Unknown tool: {tool_name}"),
        ),
    }
}

/// Build or refresh a call graph index with incremental invalidation.
///
/// Uses `.oco/call_graph.db` for persistence. Only re-parses files whose
/// modification timestamp has changed since last indexing.
fn build_call_graph_for_workspace(workspace: &std::path::Path) -> anyhow::Result<CallGraphIndex> {
    // Ensure .oco directory exists
    let oco_dir = workspace.join(".oco");
    std::fs::create_dir_all(&oco_dir)?;

    let db_path = oco_dir.join("call_graph.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let graph = CallGraphIndex::new(&db_path_str)?;
    let parser = CompositeParser::new();

    index_directory_incremental(workspace, &parser, &graph)?;

    Ok(graph)
}

/// Recursively walk a directory and incrementally index call edges.
///
/// Only re-parses files whose mtime is newer than the stored timestamp.
fn index_directory_incremental(
    dir: &std::path::Path,
    parser: &CompositeParser,
    graph: &CallGraphIndex,
) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('.')
                || matches!(
                    dir_name,
                    "node_modules" | "target" | "__pycache__" | "vendor" | "dist" | "build"
                )
            {
                continue;
            }
            index_directory_incremental(&path, parser, graph)?;
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        let Some(language) = language_from_path(&path_str) else {
            continue;
        };

        // Check file modification time for incremental indexing
        let mtime = path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Skip if file hasn't changed since last indexing
        if !graph.needs_reindex(&path_str, mtime).unwrap_or(true) {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        let Ok(parsed) = parser.parse(&content, language) else {
            continue;
        };

        let edges: Vec<StoredCallEdge> = parsed
            .calls
            .iter()
            .map(|call| StoredCallEdge {
                file: path_str.clone(),
                caller: call.caller.clone(),
                callee: call.callee.clone(),
                line: call.line,
                col: call.col,
                edge_type: call.kind.to_string(),
                confidence: call.confidence,
            })
            .collect();

        let edge_count = edges.len();
        graph.index_file_calls(&path_str, &edges)?;
        graph.record_file_meta(&path_str, mtime, edge_count)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::JsonRpcResponse;
    use crate::session_manager::SessionManager;

    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Build a minimal `Arc<AppState>` suitable for testing (no auth).
    fn test_state() -> Arc<AppState> {
        let config = oco_orchestrator_core::OrchestratorConfig::default();
        let session_manager = Arc::new(SessionManager::new(config.clone(), None));
        Arc::new(AppState {
            config,
            session_manager,
            replay_registry: oco_orchestrator_core::replay::ReplayRegistry::new(),
            dashboard_dir: None,
            hook_secret: None,
            claude_capabilities: Arc::new(oco_claude_adapter::ClaudeCapabilities::none()),
        })
    }

    /// Build an `Arc<AppState>` with hook auth enabled.
    fn test_state_with_secret(secret: &str) -> Arc<AppState> {
        let config = oco_orchestrator_core::OrchestratorConfig::default();
        let session_manager = Arc::new(SessionManager::new(config.clone(), None));
        Arc::new(AppState {
            config,
            session_manager,
            replay_registry: oco_orchestrator_core::replay::ReplayRegistry::new(),
            dashboard_dir: None,
            hook_secret: Some(secret.to_string()),
            claude_capabilities: Arc::new(oco_claude_adapter::ClaudeCapabilities::none()),
        })
    }

    /// Helper: collect the response body bytes.
    async fn body_bytes(body: Body) -> Vec<u8> {
        body.collect().await.unwrap().to_bytes().to_vec()
    }

    // -- 1. GET /health -------------------------------------------------------

    #[tokio::test]
    async fn health_returns_200_with_status_ok() {
        let app = create_router(test_state());

        let req = axum::http::Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["version"].is_string());
        assert!(!json["version"].as_str().unwrap().is_empty());
    }

    // -- 2. GET /api/v1/status ------------------------------------------------

    #[tokio::test]
    async fn status_returns_200_with_expected_fields() {
        let app = create_router(test_state());

        let req = axum::http::Request::builder()
            .uri("/api/v1/status")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(json["status"].is_string());
        assert_eq!(json["active_sessions"], 0);
        assert_eq!(json["max_sessions"], 5);
        assert!(json["version"].is_string());
    }

    // -- 3. POST /api/v1/mcp — initialize ------------------------------------

    #[tokio::test]
    async fn mcp_initialize_returns_well_formed_response() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/mcp")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let rpc: JsonRpcResponse = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(rpc.jsonrpc, "2.0");
        assert_eq!(rpc.id, serde_json::json!(1));
        assert!(rpc.error.is_none());

        let result = rpc.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "oco-mcp-server");
        assert!(result["capabilities"]["tools"].is_object());
    }

    // -- 4. POST /api/v1/mcp — tools/list ------------------------------------

    #[tokio::test]
    async fn mcp_tools_list_returns_known_tools() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/mcp")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let rpc: JsonRpcResponse = serde_json::from_slice(&bytes).unwrap();

        assert!(rpc.error.is_none());

        let tools = rpc.result.unwrap()["tools"].clone();
        let tool_names: Vec<String> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();

        assert!(tool_names.contains(&"oco_orchestrate".to_string()));
        assert!(tool_names.contains(&"oco_status".to_string()));
        assert!(tool_names.contains(&"oco_trace".to_string()));
        assert!(tool_names.contains(&"oco_search".to_string()));
        assert!(tool_names.contains(&"oco_routes".to_string()));
        assert!(tool_names.contains(&"oco_impact".to_string()));
    }

    // -- 5. POST /api/v1/hooks/post-tool — hook endpoint ----------------------

    #[tokio::test]
    async fn hook_post_tool_returns_200_ok() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "event": "PostToolUse",
            "session_id": "test-session-1",
            "data": {
                "tool_name": "Edit",
                "success": true,
                "duration_ms": 42
            }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/post-tool")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn hook_file_changed_returns_message() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "event": "FileChanged",
            "data": {
                "paths": ["src/main.rs", "src/lib.rs"],
                "change_type": "modified"
            }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/file-changed")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], true);
        assert!(json["message"].as_str().unwrap().contains("2 file change"));
    }

    #[tokio::test]
    async fn hook_stop_returns_200() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "event": "Stop",
            "session_id": "test-session-2",
            "data": {
                "reason": "user_cancelled"
            }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/stop")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn hook_catchall_returns_200_for_unknown_event() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "event": "SomeNewEvent",
            "data": {}
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/some-new-event")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn hook_post_compact_returns_200() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "event": "PostCompact",
            "session_id": "test-session-3",
            "data": {}
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/post-compact")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], true);
    }

    // -- 6. Hook auth: rejected without token when secret is set ---------------

    #[tokio::test]
    async fn hook_auth_rejects_without_token() {
        let app = create_router(test_state_with_secret("s3cret"));

        let body = serde_json::json!({
            "event": "PostToolUse",
            "data": { "tool_name": "Edit", "success": true }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/post-tool")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn hook_auth_accepts_valid_token() {
        let app = create_router(test_state_with_secret("s3cret"));

        let body = serde_json::json!({
            "event": "PostToolUse",
            "data": { "tool_name": "Edit", "success": true }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/post-tool")
            .header("content-type", "application/json")
            .header("authorization", "Bearer s3cret")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -- 7. Event validation — mismatch returns 400 ----------------------------

    #[tokio::test]
    async fn hook_event_mismatch_returns_400() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "event": "WrongEvent",
            "data": { "tool_name": "Edit", "success": true }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/post-tool")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], false);
        assert!(json["message"].as_str().unwrap().contains("event mismatch"));
    }

    // -- 8. Missing required field returns 400 ---------------------------------

    #[tokio::test]
    async fn hook_missing_required_field_returns_400() {
        let app = create_router(test_state());

        // tool_name is required (no #[serde(default)])
        let body = serde_json::json!({
            "event": "PostToolUse",
            "data": { "success": true }
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/post-tool")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let bytes = body_bytes(resp.into_body()).await;
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Should return generic message, not raw serde error
        assert_eq!(json["message"], "invalid payload");
    }

    // -- 9. POST /api/v1/mcp — unknown method → -32601 -----------------------

    #[tokio::test]
    async fn mcp_unknown_method_returns_error_32601() {
        let app = create_router(test_state());

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "nonexistent/method",
            "params": {}
        });

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/mcp")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let rpc: JsonRpcResponse = serde_json::from_slice(&bytes).unwrap();

        assert!(rpc.result.is_none());

        let err = rpc.error.unwrap();
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("nonexistent/method"));
    }

    // -- Session correlation ---------------------------------------------------

    #[tokio::test]
    async fn session_created_with_external_id() {
        let state = test_state();
        let sid = state
            .session_manager
            .create_session("test request", None, Some("claude-abc-123"))
            .unwrap();

        let info = state.session_manager.get_session(&sid).await.unwrap();
        assert_eq!(info.external_session_id.as_deref(), Some("claude-abc-123"));
    }

    #[tokio::test]
    async fn session_created_without_external_id() {
        let state = test_state();
        let sid = state
            .session_manager
            .create_session("test request", None, None)
            .unwrap();

        let info = state.session_manager.get_session(&sid).await.unwrap();
        assert!(info.external_session_id.is_none());

        // SessionInfo JSON should not contain external_session_id when None
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("external_session_id"));
    }

    #[tokio::test]
    async fn session_info_serializes_external_id() {
        let state = test_state();
        let sid = state
            .session_manager
            .create_session("test", None, Some("ext-42"))
            .unwrap();

        let info = state.session_manager.get_session(&sid).await.unwrap();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"external_session_id\":\"ext-42\""));
    }

    // -- Hook payload hygiene -------------------------------------------------

    #[tokio::test]
    async fn record_hook_event_returns_false_for_unknown_session() {
        let state = test_state();
        let recorded = state
            .session_manager
            .record_hook_event("nonexistent-session", "PostToolUse", "test_tool")
            .await;
        assert!(!recorded);
    }

    #[tokio::test]
    async fn record_hook_event_returns_true_for_active_session() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        let recorded = state
            .session_manager
            .record_hook_event(&sid, "PostToolUse", "test_tool")
            .await;
        assert!(recorded);
    }

    #[test]
    fn rate_limiter_allows_within_limit() {
        use crate::hooks::RateLimiter;
        let limiter = RateLimiter::new(10);
        for _ in 0..10 {
            assert!(limiter.check());
        }
        // 11th request should be rejected
        assert!(!limiter.check());
    }

    #[test]
    fn rate_limiter_resets_after_window() {
        use crate::hooks::RateLimiter;
        let limiter = RateLimiter::new(5);
        // Exhaust the limit
        for _ in 0..5 {
            assert!(limiter.check());
        }
        assert!(!limiter.check());

        // Manually advance the window
        limiter
            .window_start
            .store(0, std::sync::atomic::Ordering::Relaxed);
        // Should allow again
        assert!(limiter.check());
    }

    // -- PostCompact re-injection ---------------------------------------------

    #[tokio::test]
    async fn compact_snapshot_returns_none_for_unknown_session() {
        let state = test_state();
        let snap = state
            .session_manager
            .get_compact_snapshot("nonexistent")
            .await;
        assert!(snap.is_none());
    }

    #[tokio::test]
    async fn compact_snapshot_returns_none_for_session_without_state() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        let snap = state.session_manager.get_compact_snapshot(&sid).await;
        assert!(snap.is_none());
    }

    #[tokio::test]
    async fn compact_snapshot_returns_none_for_empty_memory() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        // Inject a state with default (empty) WorkingMemory
        let session = oco_shared_types::Session::new("test".into(), None);
        let orch_state = oco_orchestrator_core::state::OrchestrationState::new(session);
        state
            .session_manager
            .inject_state(&sid, orch_state)
            .await
            .unwrap();

        let snap = state.session_manager.get_compact_snapshot(&sid).await;
        assert!(snap.is_none(), "empty memory should produce no snapshot");
    }

    #[tokio::test]
    async fn compact_snapshot_returns_content_when_memory_has_data() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        // Inject state with populated WorkingMemory
        let session = oco_shared_types::Session::new("test".into(), None);
        let mut orch_state = oco_orchestrator_core::state::OrchestrationState::new(session);
        orch_state
            .memory
            .add_finding(oco_shared_types::MemoryEntry::new(
                "auth bug found".into(),
                0.8,
            ));
        let fact = oco_shared_types::MemoryEntry::new("token expired".into(), 1.0);
        let fact_id = fact.id;
        orch_state.memory.add_finding(fact);
        orch_state.memory.promote_to_fact(fact_id);
        state
            .session_manager
            .inject_state(&sid, orch_state)
            .await
            .unwrap();

        let snap = state.session_manager.get_compact_snapshot(&sid).await;
        assert!(snap.is_some(), "populated memory should produce a snapshot");

        let snap = snap.unwrap();
        let facts = snap["verified_facts"].as_array().unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].as_str().unwrap(), "token expired");
    }

    // -- Hook event persistence -----------------------------------------------

    #[tokio::test]
    async fn hook_events_stored_for_active_session() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        state
            .session_manager
            .record_hook_event(&sid, "PostToolUse", "Read")
            .await;
        state
            .session_manager
            .record_hook_event(&sid, "PostToolUse", "Edit")
            .await;

        let events = state.session_manager.get_hook_events(&sid).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].hook_name, "PostToolUse");
        assert_eq!(events[0].detail, "Read");
        assert!(events[0].recorded);
        assert_eq!(events[1].detail, "Edit");
    }

    #[tokio::test]
    async fn hook_events_empty_for_new_session() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        let events = state.session_manager.get_hook_events(&sid).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn hook_events_not_found_for_unknown_session() {
        let state = test_state();
        let result = state.session_manager.get_hook_events("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn hook_events_count_in_session_info() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        state
            .session_manager
            .record_hook_event(&sid, "PostToolUse", "Read")
            .await;
        state
            .session_manager
            .record_hook_event(&sid, "TaskCompleted", "task-1")
            .await;

        let info = state.session_manager.get_session(&sid).await.unwrap();
        assert_eq!(info.hook_events_count, 2);
    }

    #[tokio::test]
    async fn hook_events_serialization() {
        use crate::session_manager::HookEvent;

        let event = HookEvent {
            timestamp: chrono::Utc::now(),
            hook_name: "PostToolUse".into(),
            detail: "Read".into(),
            session_id: Some("claude-abc".into()),
            recorded: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"hook_name\":\"PostToolUse\""));
        assert!(json.contains("\"detail\":\"Read\""));
        assert!(json.contains("\"session_id\":\"claude-abc\""));
        assert!(json.contains("\"recorded\":true"));
    }

    #[tokio::test]
    async fn hook_events_endpoint_returns_detail() {
        let state = test_state();
        let sid = state.session_manager.create_test_session("test", None);

        state
            .session_manager
            .record_hook_event(&sid, "PostToolUse", "Bash")
            .await;

        let app = create_router(state);
        let req = axum::http::Request::builder()
            .uri(format!("/api/v1/sessions/{sid}/hooks"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = body_bytes(resp.into_body()).await;
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["count"], 1);
        assert_eq!(body["events"][0]["hook_name"], "PostToolUse");
        assert_eq!(body["events"][0]["detail"], "Bash");
    }

    // -- Session ID resolution (cross-ID correlation) -------------------------
    //
    // These tests simulate the real-world flow: Claude Code sends its own
    // session_id in hook payloads, which differs from the OCO internal UUID.
    // Before the review fix, all these lookups silently returned None.

    #[tokio::test]
    async fn resolve_session_id_by_oco_uuid() {
        let state = test_state();
        let oco_sid = state.session_manager.create_test_session("test", None);

        let resolved = state.session_manager.resolve_session_id(&oco_sid).await;
        assert_eq!(resolved.as_deref(), Some(oco_sid.as_str()));
    }

    #[tokio::test]
    async fn resolve_session_id_by_external_id() {
        let state = test_state();
        let oco_sid = state
            .session_manager
            .create_test_session("test", Some("claude-sess-xyz"));

        // Lookup by Claude Code session_id → should find the OCO session
        let resolved = state
            .session_manager
            .resolve_session_id("claude-sess-xyz")
            .await;
        assert_eq!(resolved.as_deref(), Some(oco_sid.as_str()));
    }

    #[tokio::test]
    async fn resolve_session_id_unknown_returns_none() {
        let state = test_state();
        let _ = state
            .session_manager
            .create_test_session("test", Some("claude-abc"));

        let resolved = state
            .session_manager
            .resolve_session_id("totally-unknown")
            .await;
        assert!(resolved.is_none());
    }

    #[tokio::test]
    async fn hook_events_recorded_via_external_session_id() {
        // Simulates the real flow: create_session with external_id,
        // then record_hook_event using the external_id (as Claude Code would).
        let state = test_state();
        let oco_sid = state
            .session_manager
            .create_test_session("test", Some("claude-sess-42"));

        // Resolve external → OCO, then record
        let resolved = state
            .session_manager
            .resolve_session_id("claude-sess-42")
            .await
            .unwrap();
        let recorded = state
            .session_manager
            .record_hook_event(&resolved, "PostToolUse", "Read")
            .await;
        assert!(recorded);

        // Verify the event is stored in the OCO session
        let events = state
            .session_manager
            .get_hook_events(&oco_sid)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].detail, "Read");
    }

    #[tokio::test]
    async fn compact_snapshot_via_external_session_id() {
        let state = test_state();
        let oco_sid = state
            .session_manager
            .create_test_session("test", Some("claude-compact-1"));

        // Inject state with data
        let session = oco_shared_types::Session::new("test".into(), None);
        let mut orch_state = oco_orchestrator_core::state::OrchestrationState::new(session);
        let fact = oco_shared_types::MemoryEntry::new("important fact".into(), 1.0);
        let fact_id = fact.id;
        orch_state.memory.add_finding(fact);
        orch_state.memory.promote_to_fact(fact_id);
        state
            .session_manager
            .inject_state(&oco_sid, orch_state)
            .await
            .unwrap();

        // Resolve via external_id (as hook_post_compact would)
        let resolved = state
            .session_manager
            .resolve_session_id("claude-compact-1")
            .await
            .unwrap();
        let snap = state.session_manager.get_compact_snapshot(&resolved).await;
        assert!(snap.is_some());
        assert_eq!(snap.unwrap()["verified_facts"][0], "important fact");
    }

    // -- Rate limiter CAS behavior --------------------------------------------

    #[test]
    fn rate_limiter_concurrent_window_reset_is_bounded() {
        use crate::hooks::RateLimiter;
        // Simulate the scenario: limiter at window boundary
        let limiter = RateLimiter::new(5);

        // Exhaust current window
        for _ in 0..5 {
            assert!(limiter.check());
        }
        assert!(!limiter.check());

        // Force window_start to 0 to simulate a new second
        limiter
            .window_start
            .store(0, std::sync::atomic::Ordering::Relaxed);

        // First check wins the CAS and resets
        assert!(limiter.check());
        // Subsequent checks in the same window use the normal counter
        for _ in 1..5 {
            assert!(limiter.check());
        }
        // 6th should be rejected
        assert!(!limiter.check());
    }
}
