mod support;
use serde_json::json;
use support::server::{call, client};

#[tokio::test]
async fn aberration_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBab0000001"})).await;
    assert_eq!(s["type"], "aberration");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "Df(2R)03072");
    assert_eq!(sec["breakpoints"]["computed"], json!(["51A5;51C1"]));
    assert!(
        sec["breakpoints"]["attributed"]
            .as_array()
            .unwrap()
            .contains(&json!("51A5;51C1"))
    );
    assert_eq!(sec["cytology"]["deleted_segments"], json!(["51A5--51C1"]));
    assert_eq!(sec["class"][0]["term"], "chromosomal_deletion");
}

#[tokio::test]
async fn aberration_optional_sections() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBab0000001", "sections": ["genes_affected", "stocks", "phenotypes"], "limit_per_section": 4}),
    )
    .await;
    let genes = s["sections"]["genes_affected"]["items"].as_array().unwrap();
    assert_eq!(genes.len(), 4);
    assert_eq!(genes[0]["relationship"], "deletes");
    assert_eq!(s["sections"]["genes_affected"]["has_more"], true);
    let stocks = s["sections"]["stocks"]["items"].as_array().unwrap();
    assert!(stocks.iter().any(|st| st["id"] == "FBst0005422"));
    let ph = s["sections"]["phenotypes"]["items"].as_array().unwrap();
    assert!(
        ph.iter()
            .any(|p| p["genotype"].as_str().unwrap().contains("Df(2R)03072")
                && p["reference"].as_str().unwrap().starts_with("FBrf"))
    );
}

#[tokio::test]
async fn balancer_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBba0000003"})).await;
    assert_eq!(s["type"], "balancer");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "FM6");
    let comps = sec["components"].as_array().unwrap();
    assert!(
        comps
            .iter()
            .any(|x| x["id"] == "FBal0000817" && x["relationship"] == "carries")
    );
    assert!(
        comps
            .iter()
            .any(|x| x["id"] == "FBab0003928" && x["relationship"] == "variant_of")
    );
    if support::full_db() {
        assert_eq!(sec["stock_count"], 283);
    } else {
        // The fixture exports FBba0000003's derived_stock_* props; it must carry enough for the paging test.
        assert!(sec["stock_count"].as_i64().unwrap() > 10);
    }
}

#[tokio::test]
async fn balancer_stocks_paged() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBba0000003", "sections": ["stocks"], "limit_per_section": 10}),
    )
    .await;
    assert_eq!(
        s["sections"]["stocks"]["items"].as_array().unwrap().len(),
        10
    );
    assert_eq!(s["sections"]["stocks"]["has_more"], true);
}

#[tokio::test]
async fn balancer_list_options() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBba0000003", "list_options": true}),
    )
    .await;
    let all = s.to_string();
    for name in ["components", "stock_count", "phenotypes"] {
        assert!(all.contains(name), "{name} missing from {all}");
    }
}
