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

/// Handle tools/call method.
pub fn handle_tool_call(
    id: serde_json::Value,
    tool_name: &str,
    _arguments: &serde_json::Value,
) -> JsonRpcResponse {
    match tool_name {
        "oco_orchestrate" => JsonRpcResponse::success(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": "Orchestration session started. Use oco_status to check progress."
                }]
            }),
        ),
        "oco_status" => JsonRpcResponse::success(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": "{\"status\": \"idle\", \"sessions\": 0}"
                }]
            }),
        ),
        "oco_trace" => JsonRpcResponse::success(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": "[]"
                }]
            }),
        ),
        "oco_search" => JsonRpcResponse::success(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": "[]"
                }]
            }),
        ),
        _ => JsonRpcResponse::error(id, -32601, format!("Unknown tool: {tool_name}")),
    }
}
