mod support;
use tokio_postgres::NoTls;

async fn client() -> tokio_postgres::Client {
    let (c, conn) = tokio_postgres::connect(&support::test_config().chado_url, NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    c
}

#[tokio::test]
async fn migrations_apply_twice_and_populate() {
    let cfg = support::test_config();
    flybase_mcp::migrate::run(&cfg, None).await.unwrap();
    flybase_mcp::migrate::run(&cfg, None).await.unwrap(); // idempotent
    let c = client().await;
    let n: i64 = c
        .query_one("select count(*) from mcp.migration_log", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 6);
    let notch: i64 = c
        .query_one(
            "select count(*) from mcp.name_index where entity_id = 'FBgn0004647'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        (2..=60).contains(&notch),
        "distinct names only, got {notch}"
    );
    let desc: i64 = c.query_one("select count(*) from mcp.cvterm_closure a join cvterm t on t.cvterm_id = a.ancestor_id where t.name = 'biological_process'", &[]).await.unwrap().get(0);
    assert!(desc > 1000);
    let band: Option<i32> = c
        .query_opt(
            "select fmin from mcp.band_coordinates where band = '3C7' and arm = 'X'",
            &[],
        )
        .await
        .unwrap()
        .map(|r| r.get(0));
    assert!(band.is_some());
    let trgm: bool = c
        .query_one(
            "select exists(select 1 from pg_extension where extname = 'pg_trgm')",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(trgm);
    let chado = flybase_mcp::backends::chado::Chado::connect(&cfg)
        .await
        .unwrap();
    chado
        .startup_check(&flybase_mcp::migrate::names())
        .await
        .unwrap();
    assert!(chado.startup_check(&["999_not_applied"]).await.is_err());
}
