mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn mixed_batch_of_ids_symbols_and_external_ids() {
    let c = client().await;
    let s = call(&c, "validate_ids", json!({"ids": ["FBgn0004647", "FBgn0000311", "FBgn9999999", "N", "P07207", "318824", "HGNC:7881", "25637572", "xyzzy_no_such_thing"], "species": "Dmel"})).await;
    let by_input = |k: &str| {
        s["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["input"] == k)
            .unwrap()
            .clone()
    };
    assert_eq!(by_input("FBgn0004647")["status"], "current");
    assert_eq!(by_input("FBgn0004647")["symbol"], "N");
    assert_eq!(by_input("FBgn0000311")["status"], "updated");
    assert_eq!(
        by_input("FBgn0000311")["current_ids"],
        json!(["FBgn0004647"])
    );
    assert_eq!(by_input("FBgn9999999")["status"], "unknown");
    let n = by_input("N");
    assert_eq!(n["status"], "matched");
    assert_eq!(n["matched_by"], "symbol");
    assert_eq!(n["current_ids"][0], "FBgn0004647");
    let up = by_input("P07207");
    assert_eq!(up["matched_by"], "external_id");
    assert_eq!(up["status"], "ambiguous");
    assert!(
        up["current_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i == "FBgn0004647")
    );
    // 318824 is dpy's Entrez Gene ID and not a Bloomington stock number (Notch's 31293 is both, and stock numbers win).
    assert_eq!(by_input("318824")["current_ids"], json!(["FBgn0053196"]));
    assert_eq!(by_input("318824")["matched_by"], "external_id");
    assert_eq!(
        by_input("HGNC:7881")["current_ids"],
        json!(["FBog0000245780"])
    );
    assert_eq!(by_input("25637572")["current_ids"], json!(["FBrf0227604"]));
    assert_eq!(by_input("25637572")["matched_by"], "pubmed_id");
    assert_eq!(by_input("xyzzy_no_such_thing")["status"], "unknown");
    assert_eq!(s["counts"]["current"], 1);
}

#[tokio::test]
async fn one_result_per_input_in_order() {
    let c = client().await;
    let s = call(
        &c,
        "validate_ids",
        json!({"ids": ["28813", "N", "", "N", "FBgn0004647"]}),
    )
    .await;
    let r = s["results"].as_array().unwrap();
    assert_eq!(r.len(), 5);
    assert_eq!(r[0]["current_ids"], json!(["FBst0028813"]));
    assert_eq!(r[0]["matched_by"], "stock_number");
    assert_eq!(r[2]["status"], "invalid");
    assert_eq!(r[1]["input"], "N");
    assert_eq!(r[3]["input"], "N");
    assert_eq!(r[1]["current_ids"], r[3]["current_ids"]);
    assert_eq!(r[4]["status"], "current");
}

#[tokio::test]
async fn target_class_filters_matches() {
    let c = client().await;
    let s = call(
        &c,
        "validate_ids",
        json!({"ids": ["P07207"], "target_class": "gene"}),
    )
    .await;
    assert_eq!(s["results"][0]["current_ids"], json!(["FBgn0004647"]));
    assert_eq!(s["results"][0]["data_class"], "gene");
    // The symbol belongs to the surviving gene, not to the first polypeptide or GenBank protein row the query returned.
    assert_eq!(s["results"][0]["symbol"], "N");
    assert_eq!(s["results"][0]["status"], "matched");
}

#[tokio::test]
async fn ambiguous_symbol_lists_all_candidates() {
    let c = client().await;
    // "Notch" is the exact-case full name of the gene and of allele FBal0017897.
    let s = call(&c, "validate_ids", json!({"ids": ["Notch"]})).await;
    let r = &s["results"][0];
    assert_eq!(r["status"], "ambiguous");
    assert!(
        r["current_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i == "FBgn0004647")
    );
    assert!(r["current_ids"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn batch_limit_and_empty_list() {
    let c = client().await;
    let ids: Vec<String> = (0..501).map(|i| format!("FBgn{i:07}")).collect();
    let r = call_raw(&c, "validate_ids", json!({"ids": ids}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
    let r = call_raw(&c, "validate_ids", json!({"ids": []}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

/// The HGNC prefix is case-insensitive, as in get_orthologs; each input is answered as written, and a non-ASCII
/// accession after the prefix is simply unknown.
#[tokio::test]
async fn hgnc_prefix_is_case_insensitive() {
    let c = client().await;
    let s = call(
        &c,
        "validate_ids",
        json!({"ids": ["hgnc:7881", "HGNC:7881", "hgnc:é"]}),
    )
    .await;
    let r = s["results"].as_array().unwrap();
    assert_eq!(r[0]["input"], "hgnc:7881");
    assert_eq!(r[0]["matched_by"], "external_id");
    assert_eq!(r[0]["current_ids"], json!(["FBog0000245780"]));
    assert_eq!(r[0]["current_ids"], r[1]["current_ids"]);
    assert_eq!(r[2]["status"], "unknown");
}
