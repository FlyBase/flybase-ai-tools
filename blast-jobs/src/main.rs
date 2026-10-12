//! `blast-jobs serve <config.json>`: runs the internal job API, the workers and the retention sweep.
//! `blast-jobs check <config.json>`: loads the config and the catalogue, probes the BLAST+ binaries,
//! and exits 0 when all is well.
//!
//! The bearer token comes from `BLAST_JOBS_TOKEN` (never logged). Logs are JSON lines on stdout;
//! `RUST_LOG` sets the filter (default `info`). SIGTERM or Ctrl-C stops accepting connections and
//! lets in-flight requests finish; jobs still queued or running are marked failed at the next start.
use anyhow::{Context, bail};
use blast_jobs::auth::Auth;
use blast_jobs::catalogue::Catalogue;
use blast_jobs::config::Config;
use blast_jobs::http::{self, AppState};
use blast_jobs::runner;
use blast_jobs::store::{SWEEP_INTERVAL, Store};
use std::sync::Arc;
use std::time::Duration;

const USAGE: &str = "usage: blast-jobs serve <config.json> | blast-jobs check <config.json>";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stdout)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, path) = match args.as_slice() {
        [cmd, path] => (cmd.as_str(), path.as_str()),
        _ => bail!(USAGE),
    };
    let cfg = Config::load(path)?;
    let catalogue = Catalogue::load(&cfg.catalogue, &cfg.database_root)?;
    match cmd {
        "check" => {
            http::probe_binaries(&cfg.blast_bin_dir).map_err(|e| {
                anyhow::anyhow!("BLAST binaries in {}: {e}", cfg.blast_bin_dir.display())
            })?;
            tracing::info!(
                databases = catalogue.len(),
                database_release = %cfg.database_release,
                "configuration, catalogue and BLAST binaries are valid"
            );
            Ok(())
        }
        "serve" => {
            let auth = Auth::from_env()?;
            tokio::runtime::Runtime::new()?.block_on(serve(cfg, catalogue, auth))
        }
        _ => bail!(USAGE),
    }
}

async fn serve(cfg: Config, catalogue: Catalogue, auth: Auth) -> anyhow::Result<()> {
    if let Err(e) = http::probe_binaries(&cfg.blast_bin_dir) {
        // Not fatal: health reports it, so the container stays up and says why.
        tracing::error!(dir = %cfg.blast_bin_dir.display(), error = %e, "BLAST binaries unavailable");
    }
    let cfg = Arc::new(cfg);
    let store = Store::open(&cfg.state_root, cfg.max_queued)?;
    let workers = runner::spawn_workers(cfg.clone(), store.clone());
    let _sweeper = store.spawn_sweeper(SWEEP_INTERVAL, Duration::from_secs(cfg.retention_seconds));
    let listener = tokio::net::TcpListener::bind(cfg.bind)
        .await
        .with_context(|| format!("binding {}", cfg.bind))?;
    tracing::info!(
        bind = %cfg.bind,
        databases = catalogue.len(),
        database_release = %cfg.database_release,
        concurrency = cfg.concurrency,
        auth = matches!(auth, Auth::Token(_)),
        "blast-jobs listening"
    );
    let state = AppState::new(cfg.clone(), Arc::new(catalogue), store, auth, workers);
    axum::serve(listener, http::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    tracing::info!("blast-jobs stopped");
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
    tracing::info!("shutdown signal received");
}
