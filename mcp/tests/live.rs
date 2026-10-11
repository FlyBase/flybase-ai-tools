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
