mod support;
use tokio_postgres::NoTls;

/// The two `migrate::run` callers in this binary never overlap (each run drops and rebuilds the derived tables).
static MIGRATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn client() -> tokio_postgres::Client {
    let (c, conn) = tokio_postgres::connect(&support::test_config().chado_url, NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    c
}

#[tokio::test]
async fn migrations_apply_twice_and_populate() {
    let _serial = MIGRATE.lock().await;
    let cfg = support::test_config();
    flybase_mcp::migrate::run(&cfg, None).await.unwrap();
    flybase_mcp::migrate::run(&cfg, None).await.unwrap(); // idempotent
    let c = client().await;
    let n: i64 = c
        .query_one("select count(*) from mcp.migration_log", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 9);
    let located: i64 = c
        .query_one(
            "select count(*) from mcp.located_feature where species = 'Dmel' and arm = 'X' and type = 'gene'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(located > 0, "located_feature holds Dmel genes on X");
    let indexes: Vec<String> = c
        .query(
            "select indexname::text from pg_indexes where schemaname = 'mcp' and tablename = 'located_feature' order by 1",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(indexes, ["located_feature_arm", "located_feature_window"]);
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
    let ab: (String, i32, i32) = c
        .query_one(
            "select arm, fmin, fmax from mcp.aberration_cyto where fbab_id = 'FBab0000001'",
            &[],
        )
        .await
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .unwrap();
    assert_eq!(ab, ("2R".to_string(), 14389491, 14872152));
    if support::full_db() {
        let multi: i64 = c.query_one("select count(*) from (select fbab_id from mcp.aberration_cyto group by 1 having count(*) > 1) x", &[]).await.unwrap().get(0);
        assert!(multi > 1000, "translocations span several arms");
    }
    let ds: i64 = c
        .query_one("select count(*) from mcp.rnaseq_dataset", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(ds, 5, "five RNA-seq datasets");
    let fa2: Vec<String> = c
        .query_one(
            "select samples from mcp.rnaseq_dataset where dataset = 'FlyAtlas2'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(fa2.len(), 43);
    assert_eq!(fa2[0], "Adult_Female_Brain");
    let notch: (Vec<f32>, f32, f32) = {
        let r = c.query_one("select vector, mean, stddev from mcp.rnaseq_matrix where dataset = 'FlyAtlas2' and gene_id = 'FBgn0004647'", &[]).await.unwrap();
        (r.get(0), r.get(1), r.get(2))
    };
    assert_eq!(notch.0.len(), 43);
    assert!((notch.1 - 10.372).abs() < 0.01, "mean {}", notch.1);
    assert!((notch.2 - 14.967).abs() < 0.01, "stddev {}", notch.2);
    if support::full_db() {
        let n: i64 = c
            .query_one("select count(*) from mcp.rnaseq_expression", &[])
            .await
            .unwrap()
            .get(0);
        assert!(n > 2_600_000, "expression rows {n}");
        let m: i64 = c
            .query_one(
                "select count(*) from mcp.rnaseq_matrix where dataset = 'FlyAtlas2' and stddev > 0",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(m, 16608);
    }
    let chado = flybase_mcp::backends::chado::Chado::connect(&cfg)
        .await
        .unwrap();
    chado
        .startup_check(&flybase_mcp::migrate::names())
        .await
        .unwrap();
    assert!(chado.startup_check(&["999_not_applied"]).await.is_err());
}

#[tokio::test]
async fn diopt_loader_parses_the_release_file_format() {
    let _serial = MIGRATE.lock().await;
    let cfg = support::test_config();
    let dir = std::env::temp_dir().join(format!("diopt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dmel_human_orthologs_disease_fb_test.tsv");
    std::fs::write(&path, "## FlyBase D. melanogaster-Human Orthologs and associated diseases report\n\
##Dmel_gene_ID\tDmel_gene_symbol\tHuman_gene_HGNC_ID\tHuman_gene_OMIM_ID\tHuman_gene_symbol\tDIOPT_score\tOMIM_Phenotype_IDs\tOMIM_Phenotype_IDs[name]\n\
FBgn0004647\tN\tHGNC:7881\tMIM:190198\tNOTCH1\t11\t109730,616028\t109730[AORTIC VALVE DISEASE 1; AOVD1],616028[ADAMS-OLIVER SYNDROME 5; AOS5]\n\
FBgn0004647\tN\tHGNC:7884\tMIM:164951\tNOTCH4\t5\t\t\n\
## Finished report_human_orthologs\n").unwrap();
    let c = client().await;
    flybase_mcp::migrate::run(&cfg, Some(path.clone()))
        .await
        .unwrap();
    let n: i64 = c
        .query_one("select count(*) from mcp.diopt_orthologs", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 2, "loader replaces the table with the file's rows");
    let score: i32 = c
        .query_one(
            "select score from mcp.diopt_orthologs where hgnc_id = 'HGNC:7881'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(score, 11);
    let omim: Option<String> = c
        .query_one(
            "select omim_phenotypes from mcp.diopt_orthologs where hgnc_id = 'HGNC:7884'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(omim.is_none());
    // Never leave the two-row sample behind: reload the full file when it is present (local development keeps it at
    // fixture/data/diopt.tsv.gz), else empty the table, which is the state before any --diopt load.
    let full =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture/data/diopt.tsv.gz");
    if full.exists() {
        flybase_mcp::migrate::run(&cfg, Some(full)).await.unwrap();
    } else {
        c.batch_execute("truncate mcp.diopt_orthologs")
            .await
            .unwrap();
    }
    std::fs::remove_dir_all(&dir).ok();
}
