mod support;
use flybase_mcp::backends::chado::Chado;

#[tokio::test]
async fn pool_connects_and_lists_species() {
    let chado = Chado::connect(&support::test_config()).await.unwrap();
    let rows = chado.query("test", "species/list", &[]).await.unwrap();
    assert!(rows.len() > 10);
    let ping = chado.query("test", "startup/ping", &[]).await.unwrap();
    assert_eq!(ping[0].get::<_, i32>("ok"), 1);
}

#[tokio::test]
async fn startup_check_passes_with_no_expected_migrations() {
    let chado = Chado::connect(&support::test_config()).await.unwrap();
    chado.startup_check(&[]).await.unwrap();
    let rows = chado.query("test", "startup/tables", &[]).await.unwrap();
    assert_eq!(rows.len(), flybase_mcp::backends::chado::STARTUP_RELATIONS);
    let names: Vec<String> = rows.iter().map(|r| r.get("relation")).collect();
    for rel in [
        "public.grp",
        "public.cell_line",
        "public.strain",
        "mcp.name_index",
    ] {
        assert!(names.iter().any(|n| n == rel), "missing {rel}");
    }
}

#[tokio::test]
async fn statement_timeout_maps_to_limit_exceeded() {
    let chado = Chado::connect(&support::test_config()).await.unwrap();
    let mut c = chado.get("test").await.unwrap();
    let tx = c.transaction().await.unwrap();
    tx.batch_execute("set local statement_timeout = '1ms'")
        .await
        .unwrap();
    let e = tx.query("select pg_sleep(1)", &[]).await.unwrap_err();
    let mapped = flybase_mcp::error::ToolError::db("test", &e);
    assert_eq!(mapped.code(), "limit_exceeded");
    assert!(!mapped.to_string().contains("pg_sleep"));
}

/// Review focus 2: the 45 s timeout is local to the heavy transaction; the pooled session keeps 10 s.
#[tokio::test]
async fn heavy_timeout_is_local_to_the_transaction() {
    use flybase_mcp::config::Config;
    let base = support::test_config();
    let cfg = Config::from_lookup(|k| match k {
        "CHADO_POOL_SIZE" => Some("1".into()),
        "FLYBASE_MCP_BIND" => Some("127.0.0.1:0".into()),
        "FLYBASE_MCP_PUBLIC_URL" => Some("http://127.0.0.1".into()),
        "FLYBASE_RELEASE" => Some("FB_TEST".into()),
        "CHADO_URL" => Some(base.chado_url.clone()),
        _ => None,
    })
    .unwrap();
    let chado = Chado::connect(&cfg).await.unwrap();
    let inside: String = chado
        .heavy("test", async |tx| {
            let r = Chado::query_tx(tx, "test", "startup/timeout_setting", &[]).await?;
            Ok(r[0].get::<_, String>("setting"))
        })
        .await
        .unwrap();
    assert_eq!(inside, "45s");
    let after = chado
        .query("test", "startup/timeout_setting", &[])
        .await
        .unwrap();
    assert_eq!(
        after[0].get::<_, String>("setting"),
        "10s",
        "pool size 1, so this is the same session"
    );
}
