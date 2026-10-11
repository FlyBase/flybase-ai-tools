use crate::config::Config;
use crate::error::{ToolError, ToolResult};
use crate::limits::POOL_WAIT;
use deadpool_postgres::{
    Hook, HookError, Manager, ManagerConfig, Object, Pool, PoolConfig, RecyclingMethod, Runtime,
    Status, Timeouts,
};
use tokio_postgres::{NoTls, Row, types::ToSql};

#[derive(Clone)]
pub struct Chado {
    pool: Pool,
}

const SESSION_SETUP: &str = "set default_transaction_read_only = on; set statement_timeout = '10s'; \
  set search_path = mcp, public, flybase, gene, gene_group, humanhealth, dataclass, dataclass_relationship";

impl Chado {
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let pg: tokio_postgres::Config = cfg.chado_url.parse()?;
        let mgr = Manager::from_config(
            pg,
            NoTls,
            ManagerConfig {
                recycling_method: RecyclingMethod::Fast,
            },
        );
        let pool = Pool::builder(mgr)
            .config(PoolConfig {
                max_size: cfg.pool_size,
                timeouts: Timeouts {
                    wait: Some(POOL_WAIT),
                    ..Timeouts::default()
                },
                ..PoolConfig::default()
            })
            .runtime(Runtime::Tokio1)
            .post_create(Hook::async_fn(|client, _| {
                Box::pin(async move {
                    client
                        .batch_execute(SESSION_SETUP)
                        .await
                        .map_err(HookError::Backend)
                })
            }))
            .build()?;
        Ok(Self { pool })
    }

    pub async fn get(&self, tool: &'static str) -> ToolResult<Object> {
        self.pool.get().await.map_err(|e| {
            tracing::warn!(tool, error = %e, "pool checkout failed");
            ToolError::BackendUnavailable {
                tool,
                message: "database busy; try again shortly".into(),
            }
        })
    }

    /// Run a named query on a pooled connection with the light (10 s) timeout.
    pub async fn query(
        &self,
        tool: &'static str,
        name: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> ToolResult<Vec<Row>> {
        let c = self.get(tool).await?;
        c.query(crate::backends::sql::get(name), params)
            .await
            .map_err(|e| ToolError::db(tool, e))
    }

    /// Verify privileges on every relation the server reads and that all expected migrations are logged.
    pub async fn startup_check(&self, expected_migrations: &[&str]) -> anyhow::Result<()> {
        let rows = self
            .query("startup", "startup/tables", &[])
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let missing: Vec<String> = rows
            .iter()
            .filter(|r| !r.get::<_, bool>("ok"))
            .map(|r| r.get::<_, String>("relation"))
            .collect();
        anyhow::ensure!(
            missing.is_empty(),
            "role lacks SELECT on: {}",
            missing.join(", ")
        );
        anyhow::ensure!(
            rows.len() == 37,
            "expected 37 relations, found {}",
            rows.len()
        );
        let logged: Vec<String> = match self.query("startup", "startup/migrations", &[]).await {
            Ok(rows) => rows.iter().map(|r| r.get::<_, String>("name")).collect(),
            Err(_) => Vec::new(),
        };
        let unapplied: Vec<&str> = expected_migrations
            .iter()
            .copied()
            .filter(|m| !logged.iter().any(|l| l == m))
            .collect();
        anyhow::ensure!(
            unapplied.is_empty(),
            "migrations not applied: {}; run `flybase-mcp migrate`",
            unapplied.join(", ")
        );
        Ok(())
    }

    pub fn status(&self) -> Status {
        self.pool.status()
    }
}
