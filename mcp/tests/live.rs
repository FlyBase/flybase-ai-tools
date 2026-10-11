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
