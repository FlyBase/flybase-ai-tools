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
