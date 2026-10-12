mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn insertion_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBti0000488"})).await;
    assert_eq!(s["type"], "insertion");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "P{PZ}kst[01318]");
    assert_eq!(
        sec["identity"]["type"],
        "transposable_element_insertion_site"
    );
    assert_eq!(sec["identity"]["cytology"], "63D2");
    assert_eq!(sec["location"]["chromosome"], "3L");
    // Zero-length insertion site (fmin == fmax): reported as the point start == end == fmax.
    assert_eq!(sec["location"]["start"], 3337664);
    assert_eq!(sec["location"]["end"], 3337664);
    assert!(
        sec["genes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "FBgn0004167")
    );
    assert_eq!(sec["construct"]["id"], "FBtp0000210");
    assert_eq!(sec["stock_count"], 1);
    assert!(s["links"]["jbrowse"].as_str().unwrap().contains("loc=3L:"));
}

#[tokio::test]
async fn insertion_alleles_and_stocks() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBti0000488", "sections": ["alleles", "stocks"]}),
    )
    .await;
    let alleles = s["sections"]["alleles"]["items"].as_array().unwrap();
    assert!(alleles.iter().any(|a| a["id"] == "FBal0009407"));
    assert_eq!(
        s["sections"]["stocks"]["items"][0]["provider"],
        "Bloomington"
    );
}

#[tokio::test]
async fn construct_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBtp0000743"})).await;
    assert_eq!(s["type"], "construct");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "P{GAL4-elav.L}");
    assert!(
        sec["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == "FBmc0000803" && x["relationship"] == "in_vitro_descendant_of")
    );
    assert_eq!(sec["regulatory_regions"][0]["id"], "FBgn0260400");
    assert_eq!(sec["encoded_products"][0]["id"], "FBgn0014445");
}

#[tokio::test]
async fn construct_optional_sections() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBtp0000743", "sections": ["alleles", "insertions", "stocks"], "limit_per_section": 2}),
    )
    .await;
    assert_eq!(s["sections"]["alleles"]["items"][0]["id"], "FBal0042579");
    assert_eq!(
        s["sections"]["insertions"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(s["sections"]["insertions"]["has_more"], true);
    assert_eq!(
        s["sections"]["stocks"]["items"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn lowercase_prefix_is_normalized() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "fbti0000488", "sections": ["identity"]}),
    )
    .await;
    assert_eq!(s["id"], "FBti0000488");
}

/// `__timeout_probe` exists only when FLYBASE_MCP_TEST_SECTIONS=1 (the test server sets it); its query sleeps past the light timeout.
#[tokio::test]
async fn section_error_does_not_abort_report() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBti0000488", "sections": ["identity", "__timeout_probe"]}),
    )
    .await;
    assert_eq!(s["sections"]["identity"]["symbol"], "P{PZ}kst[01318]");
    assert_eq!(s["sections"]["__timeout_probe"]["error"], "limit_exceeded");
    assert_eq!(s["section_errors"], json!(["__timeout_probe"]));
}

#[tokio::test]
async fn unknown_prefix_is_invalid_input() {
    let c = client().await;
    let r = call_raw(&c, "get_report", json!({"id": "FBxx0000001"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}
