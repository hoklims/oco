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
    /// Path to the dashboard static files (built Svelte app).
    pub dashboard_dir: Option<std::path::PathBuf>,
    /// Optional shared secret for authenticating hook requests.
    /// If `None`, hook auth is skipped (dev mode).
    pub hook_secret: Option<String>,
}

/// The MCP server that wraps Axum.
pub struct McpServer {
    config: oco_orchestrator_core::OrchestratorConfig,
    llm: Option<Arc<dyn LlmProvider>>,
    dashboard_dir: Option<std::path::PathBuf>,
}

impl McpServer {
    pub fn new(config: oco_orchestrator_core::OrchestratorConfig) -> Self {
        Self {
            config,
            llm: None,
            dashboard_dir: None,
        }
    }

    /// Set the LLM provider to use for orchestration sessions.
    /// If not called, a `StubLlmProvider` is used.
    #[must_use]
    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Set the dashboard static files directory.
    #[must_use]
    pub fn with_dashboard_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.dashboard_dir = Some(dir);
        self
    }

    /// Bind, build the app, and return the listener + app without serving.
    /// The caller can inspect the bound address before starting.
    pub async fn bind(self) -> Result<(tokio::net::TcpListener, axum::Router)> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);

        let session_manager = Arc::new(SessionManager::new(self.config.clone(), self.llm));
        let hook_secret = std::env::var("OCO_HOOK_SECRET").ok();

        let state = Arc::new(AppState {
            config: self.config,
            session_manager,
            replay_registry: ReplayRegistry::new(),
            dashboard_dir: self.dashboard_dir,
            hook_secret,
        });

        let app = create_router(state)
            .layer(CorsLayer::permissive())
            .layer(TraceLayer::new_for_http());

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        Ok((listener, app))
    }

    /// Start the server and listen for connections.
    pub async fn run(self) -> Result<()> {
        let has_dashboard = self.dashboard_dir.is_some();
        let (listener, app) = self.bind().await?;
        let local_addr = listener.local_addr()?;

        tracing::info!("OCO server listening on http://{local_addr}");
        if has_dashboard {
            tracing::info!("Dashboard: http://{local_addr}/dashboard");
        }

        axum::serve(listener, app).await?;
        Ok(())
    }
}
