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

use oco_orchestrator_core::runtime::OrchestratorRuntime;

use crate::server::AppState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StartSessionRequest {
    pub user_request: String,
    pub workspace_root: Option<String>,
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
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/sessions", post(start_session).get(list_sessions))
        .route("/api/v1/sessions/{session_id}", get(get_session))
        .route("/api/v1/sessions/{session_id}/stop", post(stop_session))
        .route("/api/v1/sessions/{session_id}/trace", get(get_trace))
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/index", post(index_workspace))
        .route("/api/v1/search", post(search_workspace))
        .route("/api/v1/mcp", post(mcp_handler))
        .with_state(state)
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

    match state
        .session_manager
        .create_session(&req.user_request, req.workspace_root.as_deref())
    {
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

            match state.session_manager.create_session(request, workspace) {
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
        _ => crate::protocol::JsonRpcResponse::error(
            id,
            -32601,
            format!("Unknown tool: {tool_name}"),
        ),
    }
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

    /// Build a minimal `Arc<AppState>` suitable for testing.
    fn test_state() -> Arc<AppState> {
        let config = oco_orchestrator_core::OrchestratorConfig::default();
        let session_manager = Arc::new(SessionManager::new(config.clone(), None));
        Arc::new(AppState {
            config,
            session_manager,
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
    }

    // -- 5. POST /api/v1/mcp — unknown method → -32601 -----------------------

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
}
