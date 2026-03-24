//! MCP (Model Context Protocol) server for OCO.
//!
//! Exposes orchestrator capabilities as MCP tools and resources,
//! allowing IDE extensions to interact with OCO through the standard MCP protocol.

pub mod handlers;
pub mod protocol;
pub mod router;
pub mod server;
pub mod session_manager;

pub use server::McpServer;
pub use session_manager::SessionManager;
