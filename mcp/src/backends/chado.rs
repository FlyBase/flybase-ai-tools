use crate::config::Config;
use crate::error::{ToolError, ToolResult};
use crate::limits::POOL_WAIT;
use deadpool_postgres::{
    Hook, HookError, Manager, ManagerConfig, Object, Pool, PoolConfig, RecyclingMethod, Runtime,
    Status, Timeouts, Transaction,
};
use tokio_postgres::{NoTls, Row, types::ToSql};

/// Number of relations listed in `sql/queries/startup/tables.sql`.
pub const STARTUP_RELATIONS: usize = 95;

/// Pooled read-only chado access.
///
/// A tool call must hold at most one checked-out connection at a time: the pool is sized for one
/// connection per in-flight call, and a handler that checks out a second one while holding the first can
/// deadlock the pool under load. A handler that needs several queries for one call checks out one
/// `Object` and passes it to `query_on`; concurrent queries on one `Object` are pipelined by tokio-postgres
/// (`futures::try_join!`), so fan-out never needs a second connection.
#[derive(Clone)]
pub struct Chado {
    pool: Pool,
}

const SESSION_SETUP: &str = "set default_transaction_read_only = on; set statement_timeout = '10s'; \
  set search_path = mcp, public, flybase, gene, gene_group, humanhealth, dataclass, dataclass_relationship";

impl Chado {
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let mut pg: tokio_postgres::Config = cfg.chado_url.parse()?;
        pg.connect_timeout(POOL_WAIT);
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
                    create: Some(POOL_WAIT),
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

    /// Run a named query on a connection this call checked out already (see `get`). Several `query_on`
    /// futures on the same connection may be joined; they share the one session and its timeout.
    pub async fn query_on(
        conn: &Object,
        tool: &'static str,
        name: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> ToolResult<Vec<Row>> {
        conn.query(crate::backends::sql::get(name), params)
            .await
            .map_err(|e| ToolError::db(tool, &e))
    }

    /// Run a named query on a freshly checked-out connection with the light (10 s) timeout.
    /// For a call that issues more than one query, check out once and use `query_on`.
    pub async fn query(
        &self,
        tool: &'static str,
        name: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> ToolResult<Vec<Row>> {
        let c = self.get(tool).await?;
        Self::query_on(&c, tool, name, params).await
    }

    /// Run `f` inside a read-only transaction with the heavy (45 s) statement timeout on one pooled connection.
    /// `SET LOCAL` scopes the timeout to the transaction, which is rolled back afterwards (nothing is written); if the
    /// future is cancelled, dropping the transaction sends the rollback, so the session returns to the pool with its
    /// 10 s default. Callers hold `limits.heavy` while in here.
    pub async fn heavy<T>(
        &self,
        tool: &'static str,
        f: impl AsyncFnOnce(&Transaction<'_>) -> ToolResult<T>,
    ) -> ToolResult<T> {
        let mut conn = self.get(tool).await?;
        let tx = conn
            .transaction()
            .await
            .map_err(|e| ToolError::db(tool, &e))?;
        tx.batch_execute("set local statement_timeout = '45s'")
            .await
            .map_err(|e| ToolError::db(tool, &e))?;
        let out = f(&tx).await;
        if let Err(e) = tx.rollback().await {
            tracing::warn!(tool, error = %e, "rollback after heavy transaction");
        }
        out
    }

    /// Run a named query inside a `heavy` transaction.
    pub async fn query_tx(
        tx: &Transaction<'_>,
        tool: &'static str,
        name: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> ToolResult<Vec<Row>> {
        tx.query(crate::backends::sql::get(name), params)
            .await
            .map_err(|e| ToolError::db(tool, &e))
    }

    /// Verify privileges on every relation and helper function the server uses and that all expected migrations are logged.
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
            rows.len() == STARTUP_RELATIONS,
            "expected {STARTUP_RELATIONS} relations, found {}; run `flybase-mcp migrate` if mcp tables are missing",
            rows.len()
        );
        let fns = self
            .query("startup", "startup/functions", &[])
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let not_exec: Vec<String> = fns
            .iter()
            .filter(|r| !r.get::<_, bool>("ok"))
            .map(|r| r.get::<_, String>("fn"))
            .collect();
        anyhow::ensure!(
            not_exec.is_empty(),
            "role cannot execute: {}; run migration 007_grants",
            not_exec.join(", ")
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
