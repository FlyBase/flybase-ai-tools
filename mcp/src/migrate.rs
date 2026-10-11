use crate::config::Config;
use tokio_postgres::NoTls;

pub const MIGRATIONS: &[(&str, &str)] = &[
    (
        "000_schema",
        include_str!("../sql/migrations/000_schema.sql"),
    ),
    (
        "001_extensions",
        include_str!("../sql/migrations/001_extensions.sql"),
    ),
    (
        "002_name_index",
        include_str!("../sql/migrations/002_name_index.sql"),
    ),
    (
        "003_cvterm_closure",
        include_str!("../sql/migrations/003_cvterm_closure.sql"),
    ),
    (
        "004_band_coordinates",
        include_str!("../sql/migrations/004_band_coordinates.sql"),
    ),
    ("006_diopt", include_str!("../sql/migrations/006_diopt.sql")),
    (
        "007_grants",
        include_str!("../sql/migrations/007_grants.sql"),
    ),
];

/// The database's own explanation of a failure (SQLSTATE and message) when there is one, else the client error.
fn describe(e: &tokio_postgres::Error) -> String {
    match e.as_db_error() {
        Some(d) => format!("{} {}", d.code().code(), d.message()),
        None => format!("{e:?}"),
    }
}

pub fn names() -> Vec<&'static str> {
    MIGRATIONS.iter().map(|(n, _)| *n).collect()
}

pub async fn run(cfg: &Config, diopt: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let (mut client, conn) = tokio_postgres::connect(&cfg.chado_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::error!(%e, "migration connection");
        }
    });
    client.batch_execute("set search_path = mcp, public, flybase, gene, gene_group, humanhealth, dataclass, dataclass_relationship").await?;
    for (name, sql) in MIGRATIONS {
        tracing::info!(migration = name, "applying");
        let started = std::time::Instant::now();
        client
            .batch_execute(sql)
            .await
            .map_err(|e| anyhow::anyhow!("migration {name} failed: {}", describe(&e)))?;
        client
            .execute(
                "insert into mcp.migration_log (name, crate_version, release) values ($1, $2, $3) \
                 on conflict (name) do update set applied_at = now(), crate_version = excluded.crate_version, release = excluded.release",
                &[name, &env!("CARGO_PKG_VERSION"), &cfg.release],
            )
            .await?;
        tracing::info!(
            migration = name,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "applied"
        );
    }
    if let Some(path) = diopt {
        let n = crate::backends::diopt::load(&mut client, &path).await?;
        tracing::info!(rows = n, path = %path.display(), "DIOPT orthologs loaded");
    }
    tracing::info!("migrations complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;
    /// Grants must run after every table rebuild, so they stay last.
    #[test]
    fn grants_are_the_last_migration() {
        assert_eq!(MIGRATIONS.last().unwrap().0, "007_grants");
        let mut names: Vec<&str> = MIGRATIONS.iter().map(|m| m.0).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            super::names(),
            "migrations are applied in numeric order"
        );
    }
}
