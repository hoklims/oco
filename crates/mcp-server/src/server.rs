//! MCP + HTTP server startup.

use anyhow::Result;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use oco_orchestrator_core::llm::LlmProvider;
use oco_orchestrator_core::replay::ReplayRegistry;

use crate::router::create_router;
use crate::session_manager::SessionManager;

/// Shared application state.
pub struct AppState {
    pub config: oco_orchestrator_core::OrchestratorConfig,
    pub session_manager: Arc<SessionManager>,
    /// Manages active replay sessions for the dashboard.
    pub replay_registry: ReplayRegistry,
    /// Optional shared secret for authenticating hook requests.
    /// If `None`, hook auth is skipped (dev mode).
    pub hook_secret: Option<String>,
}

/// The MCP server that wraps Axum.
pub struct McpServer {
    config: oco_orchestrator_core::OrchestratorConfig,
    llm: Option<Arc<dyn LlmProvider>>,
}

impl McpServer {
    pub fn new(config: oco_orchestrator_core::OrchestratorConfig) -> Self {
        Self { config, llm: None }
    }

    /// Set the LLM provider to use for orchestration sessions.
    /// If not called, a `StubLlmProvider` is used.
    #[must_use]
    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Start the server and listen for connections.
    pub async fn run(self) -> Result<()> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);

        let session_manager = Arc::new(SessionManager::new(self.config.clone(), self.llm));

        let hook_secret = std::env::var("OCO_HOOK_SECRET").ok();

        let state = Arc::new(AppState {
            config: self.config,
            session_manager,
            replay_registry: ReplayRegistry::new(),
            hook_secret,
        });

        let app = create_router(state)
            .layer(CorsLayer::permissive())
            .layer(TraceLayer::new_for_http());

        tracing::info!("OCO server listening on {addr}");

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}
