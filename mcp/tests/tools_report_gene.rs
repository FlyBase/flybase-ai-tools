mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn gene_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBgn0004647"})).await;
    assert_eq!(s["type"], "gene");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "N");
    assert_eq!(sec["identity"]["name"], "Notch");
    assert_eq!(sec["identity"]["gene_type"], "protein_coding_gene");
    assert_eq!(sec["location"]["chromosome"], "X");
    assert_eq!(sec["location"]["strand"], 1);
    assert_eq!(sec["location"]["start"], 3134870);
    assert_eq!(sec["location"]["end"], 3172221);
    assert!(
        sec["go_summary"]["biological_process"]
            .as_array()
            .unwrap()
            .len()
            > 5
    );
    assert!(sec["stock_count"].as_u64().unwrap() > 0);
    assert!(
        sec["phenotype_summary"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["phenotype"] != "unspecified")
    );
    assert_eq!(
        s["links"]["report"],
        "https://flybase.org/reports/FBgn0004647"
    );
    assert!(s["links"]["jbrowse"].as_str().unwrap().contains("loc=X:"));
}

#[tokio::test]
async fn gene_snapshot_for_white() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgn0003996", "sections": ["snapshot"]}),
    )
    .await;
    assert!(s["sections"]["snapshot"]["text"].as_str().unwrap().len() > 50);
}

#[tokio::test]
async fn gene_optional_sections_and_limit() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgn0004647", "sections": ["alleles", "transcripts"], "limit_per_section": 3}),
    )
    .await;
    assert_eq!(
        s["sections"]["alleles"]["items"].as_array().unwrap().len(),
        3
    );
    assert_eq!(s["sections"]["alleles"]["has_more"], true);
    assert_eq!(s["sections"]["alleles"]["next_offset"], 3);
    assert!(s["sections"].get("identity").is_none());
    assert!(
        s["sections"]["transcripts"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "FBtr0070507")
    );
}

#[tokio::test]
async fn list_options_returns_sections_only() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgn0004647", "list_options": true}),
    )
    .await;
    assert!(
        s["default_sections"]
            .as_array()
            .unwrap()
            .contains(&json!("identity"))
    );
    assert!(
        s["optional_sections"]
            .as_array()
            .unwrap()
            .contains(&json!("alleles"))
    );
    assert!(s.get("sections").is_none());
}

#[tokio::test]
async fn unknown_section_is_invalid_input() {
    let c = client().await;
    let r = call_raw(
        &c,
        "get_report",
        json!({"id": "FBgn0004647", "sections": ["nope"]}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn secondary_id_redirects() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgn0000311", "sections": ["identity"]}),
    )
    .await;
    assert_eq!(s["id"], "FBgn0004647");
    assert_eq!(s["resolved_from"], "FBgn0000311");
}

#[tokio::test]
async fn lowercase_id_prefix_is_accepted() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "fbgn0004647", "sections": ["identity"]}),
    )
    .await;
    assert_eq!(s["type"], "gene");
    assert_eq!(s["sections"]["identity"]["symbol"], "N");
}

/// A stock carrying two alleles of the gene counts once (Plan 1 deferred minor): 164 distinct stocks against a
/// per-allele sum of 165 on the full database. The fixture copies gene.allele and gene.allele_stock whole from the
/// current public release, where Notch has one allele and one stock fewer, so there the report is checked against
/// the database's own distinct count and the sum must still exceed it by the one double-counted stock.
#[tokio::test]
async fn gene_stock_count_is_distinct_stocks() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgn0004647", "sections": ["stock_count"]}),
    )
    .await;
    let (db, conn) =
        tokio_postgres::connect(&support::test_config().chado_url, tokio_postgres::NoTls)
            .await
            .unwrap();
    tokio::spawn(conn);
    let sum: i64 = db
        .query_one("select coalesce(sum(a.stocks_count), 0)::bigint from gene.allele a join feature g on g.feature_id = a.gene_id where g.uniquename = 'FBgn0004647'", &[])
        .await
        .unwrap()
        .get(0);
    let distinct: i64 = db
        .query_one("select count(distinct s.fbst_id)::bigint from gene.allele a join feature g on g.feature_id = a.gene_id join gene.allele_stock s on s.allele_id = a.id where g.uniquename = 'FBgn0004647'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(s["sections"]["stock_count"], distinct);
    assert_eq!(
        sum,
        distinct + 1,
        "the Plan 1 per-allele sum double-counts one stock"
    );
    if support::full_db() {
        assert_eq!(distinct, 164);
        assert_eq!(sum, 165);
    }
}

/// Compare the concatenated reference pages with the unpaged bibliography from the
/// same fixture. This detects skipped, duplicated and out-of-order publications.
#[tokio::test]
async fn reference_pages_cover_the_database_list() {
    let c = client().await;
    let (db, connection) =
        tokio_postgres::connect(&support::test_config().chado_url, tokio_postgres::NoTls)
            .await
            .unwrap();
    tokio::spawn(connection);
    let expected: Vec<String> = db
        .query(
            "select p.uniquename from feature f cross join lateral mcp.feature_publications(f.feature_id,true) refs
         join pub p on p.pub_id=refs.pub_id where f.uniquename = $1
         order by p.pyear desc nulls last, p.uniquename",
            &[&"FBgn0004647"],
        )
        .await
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert!(expected.len() > 100);
    let mut ids = Vec::new();
    let mut args = json!({"id":"FBgn0004647","sections":["references"],"limit_per_section":100});
    loop {
        let response = call(&c, "get_report", args.clone()).await;
        let page = &response["sections"]["references"];
        let items = page["items"].as_array().unwrap();
        for item in items {
            if item["id"] == "FBrf0056686" {
                assert_eq!(item["is_obsolete"], true);
            } else if item["id"] == "FBrf0217757" {
                assert_eq!(item["is_obsolete"], false);
            }
        }
        ids.extend(
            items
                .iter()
                .map(|item| item["id"].as_str().unwrap().to_owned()),
        );
        if page["has_more"] == false {
            assert_eq!(page.get("next_offset"), Some(&serde_json::Value::Null));
            break;
        }
        assert_eq!(items.len(), 100);
        assert_eq!(page["next_offset"], ids.len());
        assert!(ids.len() < expected.len());
        args["offset"] = page["next_offset"].clone();
    }
    assert_eq!(ids, expected);
    assert!(ids.iter().all(|id| id.starts_with("FBrf")));
    let batch = call(
        &c,
        "batch_download",
        json!({"ids":["FBgn0004647"], "fields":["references", "pubmed_ids"]}),
    )
    .await;
    let mut sorted = expected.clone();
    sorted.sort();
    if !support::full_db() {
        // Frozen FB2026_03 fixture oracle. The full development restore uses an older release.
        // Independent FB2026_03 Notch report, one distinct publication ID per line.
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(format!("{}\n", sorted.join("\n")));
        assert_eq!(
            format!("{digest:x}"),
            "bb3941ba711206289fc90cfd345a225279cc4c1a3076752b7287f2cdc1d0f7c0"
        );
        assert!(!ids.iter().any(|id| id == "FBrf0213040"));
    }
    let pmids: Vec<String> = db
        .query(
            "select distinct d.accession from pub p join pub_dbxref pd using(pub_id)
         join dbxref d using(dbxref_id) join db using(db_id)
         where lower(db.name)='pubmed' and p.uniquename=any($1) order by d.accession",
            &[&sorted],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(
        batch["rows"][0]["pubmed_ids"],
        format!("{} (+{} more)", pmids[..50].join("|"), pmids.len() - 50)
    );
    let preview = format!("{} (+{} more)", sorted[..50].join("|"), sorted.len() - 50);
    assert_eq!(batch["rows"][0]["references"], preview);

    args["sections"] = json!(["references", "identity"]);
    let invalid = call_raw(&c, "get_report", args).await.unwrap();
    assert_eq!(
        invalid.structured_content.unwrap()["error"],
        "invalid_input"
    );
}

#[tokio::test]
async fn allele_pages_preserve_existing_order_and_finish() {
    let c = client().await;
    let (db, connection) =
        tokio_postgres::connect(&support::test_config().chado_url, tokio_postgres::NoTls)
            .await
            .unwrap();
    tokio::spawn(connection);
    let id = "FBgn0004647";
    let expected: Vec<(String, Option<bool>, Option<bool>)> = db
        .query(
            "select a.fbal_id, a.is_alleleof, a.gene_is_regulatory_region
             from gene.allele a join feature g on g.feature_id = a.gene_id
             where g.uniquename = $1 order by a.paper_count desc nulls last, a.fbal_id, a.id",
            &[&id],
        )
        .await
        .unwrap()
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    assert!(expected.len() > 100);

    for limit in [37, 100] {
        let mut args = json!({"id":id,"sections":["alleles"],"limit_per_section":limit});
        for (index, chunk) in expected.chunks(limit).enumerate() {
            let response = call(&c, "get_report", args.clone()).await;
            let page = &response["sections"]["alleles"];
            let items = page["items"].as_array().unwrap();
            let rows: Vec<_> = items
                .iter()
                .map(|item| {
                    (
                        item["id"].as_str().unwrap().to_owned(),
                        item["is_alleleof"].as_bool(),
                        item["gene_is_regulatory_region"].as_bool(),
                    )
                })
                .collect();
            assert_eq!(rows, chunk);
            assert_eq!(page["count_shown"], chunk.len());
            let next = index * limit + chunk.len();
            let has_more = next < expected.len();
            assert_eq!(page["has_more"], has_more);
            assert_eq!(page["next_offset"], json!(has_more.then_some(next)));
            args["offset"] = page["next_offset"].clone();
        }
    }

    if !support::full_db() {
        let allele = "FBal0033811";
        let index = expected.iter().position(|row| row.0 == allele).unwrap();
        assert_eq!(expected[index + 1].0, allele);
        let mut roles = Vec::new();
        for offset in [index, index + 1] {
            let response = call(
                &c,
                "get_report",
                json!({"id":id,"sections":["alleles"],"limit_per_section":1,"offset":offset}),
            )
            .await;
            let page = &response["sections"]["alleles"];
            let row = &page["items"][0];
            assert_eq!(row["id"], allele);
            assert_eq!(page["next_offset"], offset + 1);
            roles.push((
                row["is_alleleof"].as_bool().unwrap(),
                row["gene_is_regulatory_region"].as_bool().unwrap(),
            ));
        }
        roles.sort_unstable();
        assert_eq!(roles, vec![(false, true), (true, false)]);
    }

    let response = call(
        &c,
        "get_report",
        json!({"id":id,"sections":["alleles"],"offset":expected.len()}),
    )
    .await;
    assert_eq!(
        response["sections"]["alleles"],
        json!({"items":[],"count_shown":0,"has_more":false,"next_offset":null})
    );
}
