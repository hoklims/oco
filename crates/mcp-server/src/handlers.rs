//! MCP method handlers.

use serde_json::json;

use crate::protocol::{
    JsonRpcResponse, McpCapabilities, McpResource, McpResourceCapability, McpTool,
    McpToolCapability,
};

/// Handle the initialize method.
pub fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
    let capabilities = McpCapabilities {
        tools: Some(McpToolCapability {
            list_changed: false,
        }),
        resources: Some(McpResourceCapability {
            subscribe: false,
            list_changed: false,
        }),
    };

    JsonRpcResponse::success(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "oco-mcp-server",
                "version": "0.1.0"
            },
            "capabilities": capabilities
        }),
    )
}

/// Handle tools/list method.
pub fn handle_tools_list(id: serde_json::Value) -> JsonRpcResponse {
    let tools = vec![
        McpTool {
            name: "oco_orchestrate".into(),
            description: "Start an orchestration session for a coding task".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request": {
                        "type": "string",
                        "description": "The user's coding request"
                    },
                    "workspace_root": {
                        "type": "string",
                        "description": "Path to the workspace root"
                    }
                },
                "required": ["request"]
            }),
        },
        McpTool {
            name: "oco_status".into(),
            description: "Get the status of the current orchestration session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        McpTool {
            name: "oco_trace".into(),
            description: "Get the decision trace for a session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session ID to get trace for"
                    }
                }
            }),
        },
        McpTool {
            name: "oco_search".into(),
            description: "Search the indexed codebase".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        },
        McpTool {
            name: "oco_routes".into(),
            description: "Trace the call chain (callers and callees) for a symbol. Returns transitive callers and callees up to a configurable depth.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "The function/method name to trace"
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["callers", "callees", "both"],
                        "description": "Direction of traversal",
                        "default": "both"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum traversal depth",
                        "default": 5
                    }
                },
                "required": ["symbol"]
            }),
        },
        McpTool {
            name: "oco_impact".into(),
            description: "Analyze the impact of changing a symbol. Returns all transitive callers (what breaks if this changes) and callees (what this depends on).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "The function/method name to analyze impact for"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum traversal depth",
                        "default": 5
                    }
                },
                "required": ["symbol"]
            }),
        },
    ];

    JsonRpcResponse::success(id, json!({ "tools": tools }))
}

/// Handle resources/list method.
pub fn handle_resources_list(id: serde_json::Value) -> JsonRpcResponse {
    let resources = vec![
        McpResource {
            uri: "oco://session/current".into(),
            name: "Current Session".into(),
            description: "The current orchestration session state".into(),
            mime_type: "application/json".into(),
        },
        McpResource {
            uri: "oco://traces/latest".into(),
            name: "Latest Traces".into(),
            description: "Decision traces from the latest session".into(),
            mime_type: "application/json".into(),
        },
    ];

    JsonRpcResponse::success(id, json!({ "resources": resources }))
}

/// Helper: wrap a structured response into MCP content format.
/// All tools return: { summary, evidence, risks, next_step, confidence }.
fn structured_response(id: serde_json::Value, payload: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "content": [{
                "type": "text",
                "text": payload.to_string()
            }]
        }),
    )
}

/// Handle tools/call method (stdio fallback — no AppState available).
///
/// For the full HTTP server with real session management, see `router.rs`.
/// This handler returns structured responses indicating that the client
/// should use the HTTP server for full functionality.
pub fn handle_tool_call(
    id: serde_json::Value,
    tool_name: &str,
    _arguments: &serde_json::Value,
) -> JsonRpcResponse {
    match tool_name {
        "oco_orchestrate" => structured_response(
            id,
            json!({
                "summary": "Stdio transport does not support session orchestration",
                "evidence": [],
                "risks": ["Orchestration requires the HTTP server (`oco serve`)"],
                "next_step": "Start the HTTP server with `oco serve --port 3000`, then connect via HTTP",
                "confidence": 0.0
            }),
        ),
        "oco_status" => structured_response(
            id,
            json!({
                "summary": "Server running in stdio mode — no active sessions",
                "evidence": [{"status": "idle", "sessions": 0, "transport": "stdio"}],
                "risks": [],
                "next_step": "Use `oco serve` for full session management",
                "confidence": 1.0
            }),
        ),
        "oco_trace" => structured_response(
            id,
            json!({
                "summary": "No traces available in stdio mode",
                "evidence": [],
                "risks": ["Session traces require the HTTP server"],
                "next_step": "Start `oco serve` and run a session first",
                "confidence": 0.0
            }),
        ),
        "oco_search" => {
            // Search can work without full AppState — attempt CLI-based search.
            let query = _arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if query.is_empty() {
                return structured_response(
                    id,
                    json!({
                        "summary": "Empty search query",
                        "evidence": [],
                        "risks": ["No query provided"],
                        "next_step": "Provide a non-empty search query",
                        "confidence": 0.0
                    }),
                );
            }
            structured_response(
                id,
                json!({
                    "summary": format!("Search for \"{query}\" requires indexed workspace"),
                    "evidence": [],
                    "risks": ["Workspace may not be indexed — run `oco index .` first"],
                    "next_step": format!("Run `oco search \"{query}\" --workspace .` via CLI, or use `oco serve` for HTTP search"),
                    "confidence": 0.1
                }),
            )
        }
        "oco_routes" => {
            let symbol = _arguments
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if symbol.is_empty() {
                return structured_response(
                    id,
                    json!({
                        "summary": "Empty symbol name",
                        "evidence": [],
                        "risks": ["No symbol provided"],
                        "next_step": "Provide a function/method name to trace",
                        "confidence": 0.0
                    }),
                );
            }
            structured_response(
                id,
                json!({
                    "summary": format!("Route analysis for \"{symbol}\" requires indexed workspace"),
                    "evidence": [],
                    "risks": ["Workspace may not be indexed — run `oco index .` first"],
                    "next_step": format!("Index the workspace first, then query routes for \"{symbol}\""),
                    "confidence": 0.1
                }),
            )
        }
        "oco_impact" => {
            let symbol = _arguments
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if symbol.is_empty() {
                return structured_response(
                    id,
                    json!({
                        "summary": "Empty symbol name",
                        "evidence": [],
                        "risks": ["No symbol provided"],
                        "next_step": "Provide a function/method name to analyze impact for",
                        "confidence": 0.0
                    }),
                );
            }
            structured_response(
                id,
                json!({
                    "summary": format!("Impact analysis for \"{symbol}\" requires indexed workspace"),
                    "evidence": [],
                    "risks": ["Workspace may not be indexed — run `oco index .` first"],
                    "next_step": format!("Index the workspace first, then run impact analysis for \"{symbol}\""),
                    "confidence": 0.1
                }),
            )
        }
        _ => JsonRpcResponse::error(id, -32601, format!("Unknown tool: {tool_name}")),
    }
}
