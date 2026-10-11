mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn gene_and_allele_fields_in_json() {
    let c = client().await;
    let s = call(&c, "batch_download", json!({"ids": ["FBgn0004647", "FBgn0003996", "FBal0012701", "FBgn0000311"], "fields": ["symbol", "name", "location", "alleles", "human_orthologs", "allele_gene", "allele_classes", "stock_count"]})).await;
    let rows = s["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let notch = rows.iter().find(|r| r["id"] == "FBgn0004647").unwrap();
    assert_eq!(notch["symbol"], "N");
    assert_eq!(notch["name"], "Notch");
    assert_eq!(notch["location"], "X:3134870..3172221(+)");
    assert!(
        notch["alleles"]
            .as_str()
            .unwrap()
            .starts_with("FBal0012694|")
    );
    assert!(notch["alleles"].as_str().unwrap().contains("(+"));
    assert!(
        notch["human_orthologs"]
            .as_str()
            .unwrap()
            .starts_with("NOTCH1 (HGNC:7881, DIOPT 11)")
    );
    // 164 distinct stocks on FB2026_02; the exact figure moves between releases, so only the full DB pins it.
    if support::full_db() {
        assert_eq!(notch["stock_count"], "164");
    } else {
        assert!(
            notch["stock_count"]
                .as_str()
                .unwrap()
                .parse::<u32>()
                .unwrap()
                > 0
        );
    }
    assert!(notch["allele_gene"].is_null());
    let al = rows.iter().find(|r| r["id"] == "FBal0012701").unwrap();
    assert_eq!(al["allele_gene"], "FBgn0004647 N");
    assert!(
        al["allele_classes"]
            .as_str()
            .unwrap()
            .contains("loss of function allele")
    );
    let merged = rows.iter().find(|r| r["input"] == "FBgn0000311").unwrap();
    assert_eq!(merged["id"], "FBgn0004647");
    assert_eq!(merged["symbol"], "N");
}

#[tokio::test]
async fn tsv_format_and_list_options() {
    let c = client().await;
    let s = call(&c, "batch_download", json!({"ids": ["FBgn0004647"], "fields": ["symbol", "annotation_id", "go_biological_process"], "format": "tsv"})).await;
    let tsv = s["tsv"].as_str().unwrap();
    assert!(tsv.starts_with("input\tid\tsymbol\tannotation_id\tgo_biological_process\n"));
    assert!(tsv.contains("\tN\tCG3936\t"));
    assert!(
        s.get("rows").is_none(),
        "tsv mode returns only the text block"
    );
    assert_eq!(s["row_count"], 1);
    let o = call(
        &c,
        "batch_download",
        json!({"ids": [], "fields": [], "list_options": true}),
    )
    .await;
    assert!(
        o["fields"]["gene"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "human_orthologs")
    );
    assert!(
        o["fields"]["any"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "symbol")
    );
}

#[tokio::test]
async fn unknown_field_and_unknown_id() {
    let c = client().await;
    let r = call_raw(
        &c,
        "batch_download",
        json!({"ids": ["FBgn0004647"], "fields": ["nope"]}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let s = call(
        &c,
        "batch_download",
        json!({"ids": ["FBgn9999999"], "fields": ["symbol"]}),
    )
    .await;
    assert_eq!(s["rows"][0]["status"], "unknown");
    assert!(s["rows"][0]["symbol"].is_null());
}

/// Review focus 2: 500 IDs with eight fields, two of them multi-valued, complete inside the heavy transaction.
/// Measured on FB2026_02 (chado-mcp-dev, 500 genes from FBgn0004600): this set is about 183 KB; adding `references`
/// instead of `annotation_id` is 344 KB and is refused with limit_exceeded.
#[tokio::test]
async fn five_hundred_ids() {
    let c = client().await;
    let ids: Vec<String> = (4600..5100).map(|i| format!("FBgn{i:07}")).collect();
    let s = call(&c, "batch_download", json!({"ids": ids, "fields": ["symbol", "name", "species", "type", "location", "synonyms", "alleles", "annotation_id"]})).await;
    assert_eq!(
        s["rows"].as_array().unwrap().len(),
        500,
        "one row per input, known or not"
    );
    assert_eq!(s["truncated"], false);
    assert!(
        s["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "FBgn0004647" && r["symbol"] == "N")
    );
    if support::full_db() {
        // 365 of these 500 IDs are current on FB2026_02; the rest are secondary or unknown.
        assert!(
            s["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["status"] == "current")
                .count()
                > 300
        );
    }
}

/// Review focus 2: all 27 fields for 500 genes is about 1044 KB on the full database and is refused, never halved.
/// The fixture refuses too: 500 rows with 27 keys is about 410 KB even when most cells are null, so this call can
/// never fit and the error must tell the caller to ask for fewer fields.
#[tokio::test]
async fn over_cap_is_limit_exceeded() {
    let c = client().await;
    let ids: Vec<String> = (4600..5100).map(|i| format!("FBgn{i:07}")).collect();
    let fields: Vec<&str> = flybase_mcp::tools::batch::FIELDS
        .iter()
        .map(|f| f.0)
        .collect();
    let r = call_raw(&c, "batch_download", json!({"ids": ids, "fields": fields}))
        .await
        .unwrap();
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "limit_exceeded");
    assert!(s["message"].as_str().unwrap().contains("fewer fields"));
    assert!(
        s.get("rows").is_none(),
        "rows are refused whole, never halved"
    );
}
