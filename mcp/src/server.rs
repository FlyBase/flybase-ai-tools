use crate::{
    backends::{blast::BlastClient, cache::Caches, chado::Chado, wiki::WikiClient},
    config::Config,
    limits::Limits,
    tools::wiki::Wiki,
};
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
use std::time::Duration;

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

pub const INSTRUCTIONS: &str = "FlyBase MCP server. Start with `resolve` to turn a gene, allele, or other symbol into a FlyBase ID (FBgn..., FBal...); use `validate_ids` for a batch of exact IDs or symbols and `search` for free text. \
Use `get_report` with an ID and request only the sections you need; `list_options=true` lists them. `batch_download` returns a table for many IDs. \
Genome: `get_sequence` (FASTA), `map_features` (what lies in a range), `cytosearch` (cytological bands), `convert_coordinates` (release 5 to release 6 positions). \
Vocabularies: `search_vocabulary`, `get_term`, `entities_for_term` with term IDs such as GO:0007219, FBbt:00004729, DOID:162. \
Homepage data: `get_orthologs`, `get_gene_groups_and_pathways`, `get_disease_models`, `get_stocks`, `get_interactions`. \
Expression: `rnaseq_profile_search` (genes meeting RPKM conditions in named samples) and `rnaseq_similar_genes` (co-expressed genes); call either with `list_options=true` to see datasets and samples. \
BLAST: `blast_submit` returns a job_id and job_token; poll `blast_status`, then `blast_results`. \
Documentation: `search_wiki` finds FlyBase wiki help pages (what report fields mean, how tools and downloads work); `get_wiki_page` reads one. \
IDs in results link to https://flybase.org/reports/<id>; cite them. Species are FlyBase abbreviations such as Dmel; call `list_species` for the list. Paged results carry `has_more`; pass `offset` to continue. When a tool has a `list_options` argument, call it with list_options=true before guessing names.";

/// What `/health` reads; built in `app` before the MCP factory takes its clones.
#[derive(Clone)]
struct HealthState {
    chado: Chado,
    release: String,
    caches: Arc<Caches>,
    blast: Option<Arc<BlastClient>>,
    wiki: Option<Arc<Wiki>>,
}

/// Database and migration checks (3 s, together) run concurrently with the BLAST and wiki probes (2 s each),
/// so the route answers within about 3 s even when a backend hangs. `status` depends only on the database
/// and migrations.
async fn health(state: HealthState) -> Json<serde_json::Value> {
    let checks = tokio::time::timeout(Duration::from_secs(3), async {
        let db = state
            .chado
            .query("health", "startup/ping", &[])
            .await
            .is_ok();
        let migrations = match state.chado.query("health", "startup/migrations", &[]).await {
            Ok(rows) => {
                let logged: Vec<String> = rows.iter().map(|r| r.get::<_, String>("name")).collect();
                let missing: Vec<&str> = crate::migrate::names()
                    .into_iter()
                    .filter(|m| !logged.iter().any(|l| l == m))
                    .collect();
                if missing.is_empty() {
                    "ok".to_string()
                } else {
                    format!("missing: {}", missing.join(", "))
                }
            }
            Err(_) => "error".to_string(),
        };
        (db, migrations)
    });
    let blast_probe = async {
        match &state.blast {
            None => "unconfigured",
            Some(c) => {
                if c.healthy().await {
                    "ok"
                } else {
                    "unreachable"
                }
            }
        }
    };
    let wiki_probe = async {
        match &state.wiki {
            None => "unconfigured",
            Some(w) => {
                if w.client.healthy().await {
                    "ok"
                } else {
                    "unreachable"
                }
            }
        }
    };
    let (checks, blast, wiki) = tokio::join!(checks, blast_probe, wiki_probe);
    let (db, migrations) = checks.unwrap_or((false, "timeout".to_string()));
    let (entries, bytes) = state.caches.response.stats().await;
    let matrices: Vec<serde_json::Value> = state
        .caches
        .matrix
        .loaded()
        .into_iter()
        .map(|(d, g, b)| json!({"dataset": d, "genes": g, "bytes": b}))
        .collect();
    let st = state.chado.status();
    let ok = db && migrations == "ok";
    Json(json!({
        "status": if ok { "ok" } else { "degraded" },
        "release": state.release,
        "database": if db { "ok" } else { "error" },
        "migrations": migrations,
        "pool": {"size": st.size, "available": st.available},
        "blast": blast,
        "wiki": wiki,
        "caches": {
            "response": {"entries": entries, "bytes": bytes},
            "rnaseq_matrix": matrices,
            "blast_jobs": state.caches.blast_tokens.len(),
        },
    }))
}

#[derive(Clone)]
pub struct FlyBaseServer {
    pub chado: Chado,
    pub limits: Limits,
    pub release: String,
    pub caches: Arc<Caches>,
    /// None when BLAST_BASE_URL is unset; the BLAST tools then answer backend_unavailable.
    pub blast: Option<Arc<BlastClient>>,
    /// None when WIKI_BASE_URL is unset; the wiki tools then answer backend_unavailable.
    pub wiki: Option<Arc<Wiki>>,
    tool_router: ToolRouter<Self>,
}

impl FlyBaseServer {
    pub fn all_tool_routers() -> ToolRouter<Self> {
        Self::tool_router_species()
            + Self::tool_router_resolve()
            + Self::tool_router_search()
            + Self::tool_router_sequence()
            + Self::tool_router_report()
            + Self::tool_router_vocabulary()
            + Self::tool_router_validate()
            + Self::tool_router_batch()
            + Self::tool_router_map()
            + Self::tool_router_cyto()
            + Self::tool_router_entities()
            + Self::tool_router_interactions()
            + Self::tool_router_orthologs()
            + Self::tool_router_groups()
            + Self::tool_router_disease()
            + Self::tool_router_stocks()
            + Self::tool_router_coordinates()
            + Self::tool_router_rnaseq()
            + Self::tool_router_blast()
            + Self::tool_router_wiki()
    }

    pub fn new(
        chado: Chado,
        limits: Limits,
        release: String,
        caches: Arc<Caches>,
        blast: Option<Arc<BlastClient>>,
        wiki: Option<Arc<Wiki>>,
    ) -> Self {
        Self {
            chado,
            limits,
            release,
            caches,
            blast,
            wiki,
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
    let caches = Arc::new(Caches::new(cfg.response_cache_bytes));
    let blast = match &cfg.blast_base_url {
        Some(u) => {
            if cfg.blast_token.is_none() {
                tracing::warn!(
                    "BLAST_BASE_URL set without BLAST_SERVICE_TOKEN; requests go unauthenticated"
                );
            }
            match BlastClient::new(u.clone(), cfg.blast_token.clone()) {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    tracing::error!(error = %e, "BLAST client not built; BLAST tools unavailable");
                    None
                }
            }
        }
        None => None,
    };
    let wiki = match &cfg.wiki_base_url {
        Some(u) => match WikiClient::new(u.clone(), cfg.wiki_host_header.clone()) {
            Ok(c) => Some(Arc::new(Wiki::new(c))),
            Err(e) => {
                tracing::error!(error = %e, "wiki client not built; wiki tools unavailable");
                None
            }
        },
        None => None,
    };
    let health_state = HealthState {
        chado: chado.clone(),
        release: release.clone(),
        caches: caches.clone(),
        blast: blast.clone(),
        wiki: wiki.clone(),
    };
    let factory = move || {
        Ok(FlyBaseServer::new(
            chado.clone(),
            limits.clone(),
            release.clone(),
            caches.clone(),
            blast.clone(),
            wiki.clone(),
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
        .route("/health", get(move || health(health_state.clone())))
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
