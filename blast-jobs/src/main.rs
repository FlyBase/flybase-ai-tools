//! `blast-jobs <config.json>`: loads and checks the config and the catalogue. The job API and the
//! workers are wired in later tasks; until then this is a configuration check that exits 0 on success.
use anyhow::Context;
use blast_jobs::{catalogue::Catalogue, config::Config};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let path = std::env::args()
        .nth(1)
        .context("usage: blast-jobs <config.json>")?;
    let cfg = Config::load(&path)?;
    let catalogue = Catalogue::load(&cfg.catalogue, &cfg.database_root)?;
    tracing::info!(
        databases = catalogue.len(),
        database_release = %cfg.database_release,
        concurrency = cfg.concurrency,
        "configuration and catalogue are valid"
    );
    Ok(())
}
