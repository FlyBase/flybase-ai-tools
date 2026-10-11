mod support;
use serde_json::json;
use support::server::{call, client};

#[tokio::test]
async fn allele_report() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBal0012701"})).await;
    assert_eq!(s["type"], "allele");
    assert_eq!(s["sections"]["gene"]["id"], "FBgn0004647");
    assert_eq!(s["sections"]["identity"]["symbol"], "N[55e11]");
    assert!(
        !s["sections"]["phenotype_summary"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(s["sections"]["stock_count"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn reference_report() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBrf0227604"})).await;
    assert_eq!(s["type"], "reference");
    assert_eq!(s["sections"]["citation"]["pubmed_id"], "25637572");
    assert_eq!(s["sections"]["citation"]["year"], "2015");
    assert!(!s["sections"]["authors"].as_array().unwrap().is_empty());
    assert!(s["sections"]["authors"][0]["surname"].is_string());
}

#[tokio::test]
async fn stock_report() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBst0028813"})).await;
    assert_eq!(s["type"], "stock");
    assert!(s["sections"]["identity"]["genotype"].is_string());
    assert_eq!(
        s["sections"]["identity"]["provider"],
        "Bloomington Drosophila Stock Center"
    );
    assert!(
        s["sections"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == "FBal0012701")
    );
}

#[tokio::test]
async fn optional_sections() {
    let c = client().await;
    let a = call(
        &c,
        "get_report",
        json!({"id": "FBal0012701", "sections": ["phenotypes", "transgenic_product_class", "insertions", "constructs", "stocks", "references", "synonyms"], "limit_per_section": 3}),
    )
    .await;
    let sec = &a["sections"];
    assert_eq!(sec["phenotypes"]["items"].as_array().unwrap().len(), 3);
    assert_eq!(sec["phenotypes"]["has_more"], true);
    assert_eq!(sec["phenotypes"]["items"][0]["genotype"], "N[55e11]");
    assert_eq!(sec["stocks"]["items"][0]["id"], "FBst0028813");
    assert_eq!(sec["insertions"]["items"][0]["id"], "FBti0014164");
    assert!(sec["transgenic_product_class"].is_array());
    assert!(sec["constructs"]["items"].is_array());
    assert!(
        sec["references"]["items"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("FBrf")
    );
    assert!(
        sec["synonyms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "N[55e11]")
    );

    let r = call(
        &c,
        "get_report",
        json!({"id": "FBrf0227604", "sections": ["genes", "alleles", "data_types"]}),
    )
    .await;
    let sec = &r["sections"];
    assert!(
        sec["genes"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "FBgn0000463")
    );
    assert!(
        sec["alleles"]["items"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("FBal")
    );
    assert!(
        sec["data_types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["type"] == "gene")
    );
}
