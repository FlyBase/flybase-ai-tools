use crate::{backends::chado::Chado, config::Config, limits::Limits};
use axum::{Json, Router, routing::get};
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerConfig},
    tool_handler,
    transport::streamable_http_server::{
        session::never::NeverSessionManager,
        tower::{StreamableHttpServerConfig, StreamableHttpService},
    },
};
use serde_json::json;
use std::sync::{Arc, OnceLock};

static PROM: OnceLock<Option<metrics_exporter_prometheus::PrometheusHandle>> = OnceLock::new();

/// Install the Prometheus recorder once per process; later `app()` calls reuse the handle.
fn prometheus() -> Option<metrics_exporter_prometheus::PrometheusHandle> {
    PROM.get_or_init(|| {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .install_recorder()
            .ok()
    })
    .clone()
}

pub const INSTRUCTIONS: &str = "FlyBase MCP server. Start with `resolve` to turn a gene, allele, or other symbol into a FlyBase ID (FBgn..., FBal...). \
Use `get_report` with that ID and request only the sections you need. Use `search` for free text. IDs in results link to https://flybase.org/reports/<id>; cite them. \
Species are FlyBase abbreviations such as Dmel. When a tool has a `list_options` argument, call it with list_options=true before guessing field, section, or dataset names.";

#[derive(Clone)]
pub struct FlyBaseServer {
    pub chado: Chado,
    pub limits: Limits,
    pub release: String,
    tool_router: ToolRouter<Self>,
}

impl FlyBaseServer {
    pub fn all_tool_routers() -> ToolRouter<Self> {
        Self::tool_router_species() + Self::tool_router_resolve()
    }

    pub fn new(chado: Chado, limits: Limits, release: String) -> Self {
        Self {
            chado,
            limits,
            release,
            tool_router: Self::all_tool_routers(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FlyBaseServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
    }
}

pub fn app(cfg: &Config, chado: Chado) -> Router {
    let limits = Limits::new();
    let release = cfg.release.clone();
    let health_chado = chado.clone();
    let health_release = release.clone();
    let factory = move || {
        Ok(FlyBaseServer::new(
            chado.clone(),
            limits.clone(),
            release.clone(),
        ))
    };
    let mcp_cfg = StreamableHttpServerConfig::default()
        .with_allowed_hosts(cfg.allowed_hosts())
        .with_allowed_origins(cfg.allowed_origins())
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_max_request_body_bytes(1024 * 1024);
    let mcp =
        StreamableHttpService::new(factory, Arc::new(NeverSessionManager::default()), mcp_cfg);
    let prom = prometheus();
    Router::new()
        .nest_service("/mcp", mcp)
        .route(
            "/health",
            get(move || {
                let chado = health_chado.clone();
                let release = health_release.clone();
                async move {
                    let db = match chado.query("health", "startup/ping", &[]).await {
                        Ok(_) => "ok",
                        Err(_) => "error",
                    };
                    let st = chado.status();
                    Json(json!({
                        "status": if db == "ok" { "ok" } else { "degraded" },
                        "release": release, "database": db,
                        "pool": { "size": st.size, "available": st.available },
                        "blast": "unconfigured"
                    }))
                }
            }),
        )
        .route(
            "/metrics",
            get(move || {
                let prom = prom.clone();
                async move { prom.map(|h| h.render()).unwrap_or_default() }
            }),
        )
}

pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let chado = Chado::connect(&cfg).await?;
    chado.startup_check(&crate::migrate::names()).await?;
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    tracing::info!(bind = %cfg.bind, release = %cfg.release, "flybase-mcp listening");
    axum::serve(listener, app(&cfg, chado))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("flybase-mcp stopped");
    Ok(())
}

/// Resolve on Ctrl-C or SIGTERM (the binary is PID 1 in a container).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl-C, shutting down"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}
