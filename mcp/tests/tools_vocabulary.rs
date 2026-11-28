mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn cv_term_report_via_get_report() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "GO:0007219"})).await;
    assert_eq!(s["type"], "cv_term");
    let sec = &s["sections"];
    assert_eq!(sec["definition"]["name"], "Notch signaling pathway");
    assert_eq!(sec["definition"]["vocabulary"], "biological_process");
    assert!(
        sec["synonyms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "N signaling pathway")
    );
    assert_eq!(sec["parents"][0]["term_id"], "GO:0007166");
    assert!(
        sec["children"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["term_id"] == "GO:0045747")
    );
    assert_eq!(
        s["links"]["report"],
        "https://flybase.org/cgi-bin/cvreport.pl?id=GO:0007219"
    );
}

#[tokio::test]
async fn get_term_returns_all_sections_with_counts() {
    let c = client().await;
    let s = call(&c, "get_term", json!({"term_id": "GO:0007219"})).await;
    assert_eq!(s["id"], "GO:0007219");
    assert_eq!(
        s["sections"]["definition"]["name"],
        "Notch signaling pathway"
    );
    assert_eq!(
        s["sections"]["definition"]["vocabulary"],
        "biological_process"
    );
    let ac = &s["sections"]["annotation_counts"];
    assert_eq!(ac["descendants"], 5);
    assert_eq!(ac["descendants_included"], true);
    let genes = ac["counts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["class"] == "feature:gene")
        .unwrap();
    // The fixture carries feature_cvterm rows for seed features only (Notch and Delta are seeds).
    assert!(genes["count"].as_u64().unwrap() >= if support::full_db() { 20 } else { 1 });
}

#[tokio::test]
async fn broad_term_counts_direct_only() {
    let c = client().await;
    let s = call(&c, "get_term", json!({"term_id": "GO:0008150"})).await;
    let ac = &s["sections"]["annotation_counts"];
    // Over MAX_DESCENDANTS (5,000) is what turns the closure off; the exact size drifts with each release.
    assert!(ac["descendants"].as_u64().unwrap() > 5000);
    if support::full_db() {
        assert!(ac["descendants"].as_u64().unwrap() > 20000);
    }
    assert_eq!(ac["descendants_included"], false);
}

#[tokio::test]
async fn anatomy_and_disease_terms() {
    let c = client().await;
    let wing = call(&c, "get_term", json!({"term_id": "FBbt:00004729"})).await;
    assert_eq!(wing["sections"]["definition"]["name"], "wing");
    let alleles = wing["sections"]["annotation_counts"]["counts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["class"] == "allele:phenotype")
        .unwrap();
    // N[55e11] has wing phenotypes in the fixture.
    assert!(alleles["count"].as_u64().unwrap() > if support::full_db() { 5000 } else { 0 });
    let doid = call(&c, "get_term", json!({"term_id": "DOID:1289"})).await;
    assert_eq!(
        doid["sections"]["definition"]["name"],
        "neurodegenerative disease"
    );
    let alias = call(&c, "get_term", json!({"term_id": "DO:1289"})).await;
    assert_eq!(alias["id"], "DOID:1289");
}

#[tokio::test]
async fn unknown_or_malformed_term_id() {
    let c = client().await;
    let r = call_raw(&c, "get_term", json!({"term_id": "GO:9999999"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
    let r = call_raw(&c, "get_term", json!({"term_id": "notch"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn search_vocabulary_all_and_filtered() {
    let c = client().await;
    let s = call(
        &c,
        "search_vocabulary",
        json!({"query": "notch", "limit": 5}),
    )
    .await;
    let terms = s["terms"].as_array().unwrap();
    let t = terms
        .iter()
        .find(|t| t["id"] == "GO:0007219")
        .expect("Notch signaling pathway");
    assert!(
        t["synonyms"]
            .as_str()
            .unwrap()
            .contains("N signaling pathway")
    );
    let w = call(
        &c,
        "search_vocabulary",
        json!({"query": "wing", "vocabularies": ["anatomy"], "limit": 3}),
    )
    .await;
    assert_eq!(w["terms"][0]["id"], "FBbt:00004729");
    assert_eq!(w["terms"][0]["match_kind"], "exact");
    assert!(
        w["terms"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["vocabulary"] == "FlyBase anatomy CV")
    );
}

#[tokio::test]
async fn search_vocabulary_guards() {
    let c = client().await;
    let opts = call(
        &c,
        "search_vocabulary",
        json!({"query": "x", "list_options": true}),
    )
    .await;
    assert!(
        opts["vocabularies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "anatomy")
    );
    let none = call(&c, "search_vocabulary", json!({"query": "%%"})).await;
    assert_eq!(none["terms"].as_array().unwrap().len(), 0);
    let r = call_raw(&c, "search_vocabulary", json!({"query": "w"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

/// get_term children page with offset: offset 1 starts at the second child of offset 0.
#[tokio::test]
async fn term_children_page_with_offset() {
    let c = client().await;
    let p0 = call(&c, "get_term", json!({"term_id": "GO:0007219"})).await;
    let p1 = call(
        &c,
        "get_term",
        json!({"term_id": "GO:0007219", "offset": 1}),
    )
    .await;
    let c0 = p0["sections"]["children"]["items"].as_array().unwrap();
    assert!(c0.len() >= 2);
    assert_eq!(p1["offset"], 1);
    assert_eq!(p1["sections"]["children"]["items"][0], c0[1]);
    let r = call_raw(
        &c,
        "get_term",
        json!({"term_id": "GO:0007219", "offset": 9901}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn stock_vocabulary_search_and_reports() {
    let c = client().await;
    let hits = call(
        &c,
        "search_vocabulary",
        json!({"query": "stock state", "vocabularies": ["stock"]}),
    )
    .await;
    assert_eq!(hits["terms"][0]["id"], "FBsv:0000001");
    assert_eq!(hits["terms"][0]["match_kind"], "exact");

    let s = call(&c, "get_report", json!({"id": "fbsv:0000000"})).await;
    assert_eq!(s["id"], "FBsv:0000000");
    assert_eq!(s["type"], "cv_term");
    assert_eq!(s["sections"]["definition"]["name"], "stock descriptor");
    assert_eq!(
        s["sections"]["definition"]["definition"],
        "An aspect of a stock relevant to its use for research."
    );
    assert!(
        s["sections"]["children"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["relationship"] == "is_a" && t["term_id"] == "FBsv:0000001")
    );

    let living = call(&c, "get_term", json!({"term_id": "FBsv:0000002"})).await;
    assert_eq!(living["sections"]["definition"]["name"], "living stock");
    assert_eq!(living["sections"]["parents"][0]["term_id"], "FBsv:0000001");
    let count = &living["sections"]["annotation_counts"]["counts"][0];
    assert_eq!(count["class"], "stock");
    assert!(count["count"].as_u64().unwrap() > 0);

    let provider = call(&c, "get_term", json!({"term_id": "FBsv:0000103"})).await;
    let count = &provider["sections"]["annotation_counts"]["counts"][0];
    assert_eq!(count["class"], "stock");
    assert!(count["count"].as_u64().unwrap() > 0);

    let balancer = call(&c, "get_term", json!({"term_id": "FBsv:0001006"})).await;
    assert_eq!(
        balancer["sections"]["definition"]["name"],
        "chromosome 1 balancer"
    );
    let count = &balancer["sections"]["annotation_counts"]["counts"][0];
    assert_eq!(count["class"], "stock");
    assert!(count["count"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn released_ontology_terms_have_canonical_reports_and_hierarchy() {
    let mut cfg = support::test_config();
    cfg.release_terms_path = Some(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/release_terms.json"),
    );
    let c = support::server::client_at(&support::server::start(cfg)).await;
    let imaging = call(&c, "get_report", json!({"id": "fbbi:00000009"})).await;
    assert_eq!(imaging["type"], "cv_term");
    assert_eq!(imaging["id"], "FBbi:00000009");
    assert_eq!(
        imaging["sections"]["definition"]["vocabulary"],
        "Imaging Methods"
    );
    assert_eq!(
        imaging["sections"]["synonyms"],
        json!([{"name":"EGS","scope":null}])
    );
    assert_eq!(
        imaging["sections"]["parents"][0]["term_id"],
        "FBbi:00000008"
    );

    let alias = call(&c, "get_term", json!({"term_id": "chebi:23648"})).await;
    assert_eq!(alias["id"], "CHEBI:27780");
    assert_eq!(alias["resolved_from"], "CHEBI:23648");
    assert_eq!(alias["sections"]["definition"]["id"], "CHEBI:27780");
    assert_eq!(alias["sections"]["definition"]["name"], "detergent");
    assert_eq!(
        alias["sections"]["definition"]["vocabulary"],
        "Application and Biological Role"
    );
    assert_eq!(alias["sections"]["synonyms"][0]["scope"], "exact");
    assert_eq!(alias["sections"]["parents"][0]["name"], "surfactant");
    assert_eq!(
        alias["links"]["report"],
        "https://flybase.org/cgi-bin/cvreport.pl?id=CHEBI:27780"
    );
    assert!(alias["sections"].get("annotation_counts").is_none());
    let report_alias = call(&c, "get_report", json!({"id": "chebi:23648"})).await;
    assert_eq!(alias["sections"], report_alias["sections"]);
    assert_eq!(alias["resolved_from"], report_alias["resolved_from"]);

    let page = call(
        &c,
        "get_term",
        json!({"term_id": "FBbi:00000008", "offset": 1}),
    )
    .await;
    assert_eq!(
        page["sections"]["children"]["items"][0]["term_id"],
        "FBbi:00000010"
    );
    assert_eq!(page["sections"]["children"]["count_shown"], 1);
    let limited = call(
        &c,
        "get_report",
        json!({"id": "FBbi:00000008", "limit_per_section": 1}),
    )
    .await;
    assert_eq!(
        limited["sections"]["children"]["items"][0]["term_id"],
        "FBbi:00000009"
    );
    assert_eq!(limited["sections"]["children"]["has_more"], true);

    let missing = call_raw(&c, "get_term", json!({"term_id": "CHEBI:15377"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(missing["error"], "not_found");
}

#[tokio::test]
async fn released_ontology_discovery_and_unsupported_operations() {
    let c = client().await;
    let opts = call(
        &c,
        "get_report",
        json!({"id": "CHEBI:27780", "list_options": true}),
    )
    .await;
    assert_eq!(
        opts["default_sections"],
        json!(["definition", "synonyms", "parents", "children"])
    );
    assert_eq!(opts["optional_sections"], json!([]));

    for (tool, args, error) in [
        (
            "get_term",
            json!({"term_id": "FBbi:00000009"}),
            "backend_unavailable",
        ),
        (
            "get_report",
            json!({"id": "CHEBI:27780", "sections": ["annotation_counts"]}),
            "invalid_input",
        ),
        (
            "entities_for_term",
            json!({"term_id": "CHEBI:27780", "data_class": "allele"}),
            "invalid_input",
        ),
    ] {
        let body = call_raw(&c, tool, args)
            .await
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(body["error"], error);
    }
}
