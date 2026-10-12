mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn list_options_describes_datasets() {
    let c = client().await;
    let o = call(&c, "rnaseq_profile_search", json!({"list_options": true})).await;
    let ds = o["datasets"].as_array().unwrap();
    assert_eq!(ds.len(), 5);
    let fa2 = ds.iter().find(|d| d["dataset"] == "FlyAtlas2").unwrap();
    assert_eq!(fa2["project_id"], "FBlc0003498");
    assert_eq!(fa2["samples"].as_array().unwrap().len(), 43);
    assert!(
        fa2["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "Adult_Male_Testis")
    );
    assert_eq!(fa2["value_type"], "RPKM");
    assert_eq!(fa2["link"], "https://flybase.org/reports/FBlc0003498");
    assert!(
        ds.iter().any(|d| d["dataset"] == "modENCODE_development"
            && d["samples"].as_array().unwrap().len() == 30)
    );
}

#[tokio::test]
async fn testis_high_ovary_low() {
    let c = client().await;
    let s = call(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": [
        {"dataset": "FlyAtlas2", "sample": "Adult_Male_Testis", "operator": "gte", "value": 500},
        {"dataset": "FlyAtlas2", "sample": "Adult_Female_Ovary", "operator": "lte", "value": 1}
    ], "limit": 3}),
    )
    .await;
    assert_eq!(s["dataset"], "FlyAtlas2");
    let genes = s["genes"].as_array().unwrap();
    assert!(genes.len() <= 3);
    for g in genes {
        assert!(g["values"]["Adult_Male_Testis"].as_f64().unwrap() >= 500.0);
        assert!(g["values"]["Adult_Female_Ovary"].as_f64().unwrap() <= 1.0);
        assert!(
            g["link"]
                .as_str()
                .unwrap()
                .starts_with("https://flybase.org/reports/FBgn")
        );
    }
    if support::full_db() {
        assert_eq!(s["total"], 194);
        assert_eq!(s["has_more"], true);
        assert_eq!(genes[0]["id"], "FBgn0028416");
        assert_eq!(genes[0]["symbol"], "Met75Ca");
        assert_eq!(genes[0]["values"]["Adult_Male_Testis"], 4105.0);
    }
}

#[tokio::test]
async fn notch_is_found_by_its_own_values() {
    // Notch is in the fixture: FlyAtlas2 Adult_Female_Ovary = 64, L3_CNS = 57.
    let c = client().await;
    let s = call(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": [
        {"dataset": "FlyAtlas2", "sample": "Adult_Female_Ovary", "operator": "eq", "value": 64},
        {"dataset": "FlyAtlas2", "sample": "L3_CNS", "operator": "gt", "value": 56}
    ], "limit": 100}),
    )
    .await;
    assert!(
        s["genes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "FBgn0004647")
    );
}

#[tokio::test]
async fn condition_validation() {
    let c = client().await;
    let r = call_raw(&c, "rnaseq_profile_search", json!({"conditions": [{"dataset": "FlyAtlas2", "sample": "Nope", "operator": "gt", "value": 1}]})).await.unwrap();
    let e = r.structured_content.unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(e["message"].as_str().unwrap().contains("list_options"));
    let r = call_raw(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": [{"dataset": "Nope", "sample": "x", "operator": "gt", "value": 1}]}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": [
        {"dataset": "FlyAtlas2", "sample": "Adult_Male_Testis", "operator": "gt", "value": 1},
        {"dataset": "modENCODE_tissues", "sample": "L3_CNS", "operator": "gt", "value": 1}]}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let many: Vec<_> = (0..11)
        .map(|_| json!({"dataset": "FlyAtlas2", "sample": "L3_CNS", "operator": "gt", "value": 1}))
        .collect();
    let r = call_raw(&c, "rnaseq_profile_search", json!({"conditions": many}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
    let r = call_raw(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": [], "species": "Hsap"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "rnaseq_profile_search", json!({}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn page_past_the_end_keeps_the_total() {
    let c = client().await;
    let conds = json!([
        {"dataset": "FlyAtlas2", "sample": "Adult_Male_Testis", "operator": "gte", "value": 500},
        {"dataset": "FlyAtlas2", "sample": "Adult_Female_Ovary", "operator": "lte", "value": 1}
    ]);
    let (offset, total) = if support::full_db() {
        (194, 194)
    } else {
        (5, 2)
    };
    let r = call_raw(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": conds, "limit": 3, "offset": offset}),
    )
    .await
    .unwrap();
    let text = r.content[0].as_text().unwrap().text.clone();
    assert!(text.starts_with(&format!("{total} FlyAtlas2 gene(s) meet all 2 condition(s); this page is empty (offset {offset})")), "{text}");
    let s = r.structured_content.unwrap();
    assert_eq!(s["total"], total);
    assert_eq!(s["genes"].as_array().unwrap().len(), 0);
    assert_eq!(s["has_more"], false);
    assert_eq!(s["offset"], offset);
}

#[tokio::test]
async fn padded_names_match_and_values_echo_as_given() {
    let c = client().await;
    let s = call(
        &c,
        "rnaseq_profile_search",
        json!({"conditions": [
        {"dataset": " FlyAtlas2 ", "sample": " L3_CNS ", "operator": "gt", "value": 4.1},
        {"dataset": "flyatlas2 ", "sample": "adult_female_ovary", "operator": "gte", "value": 0}
    ], "limit": 1}),
    )
    .await;
    assert_eq!(s["dataset"], "FlyAtlas2");
    assert_eq!(s["conditions"][0]["sample"], "L3_CNS");
    assert_eq!(s["conditions"][0]["value"], 4.1);
    assert_eq!(s["conditions"][1]["sample"], "Adult_Female_Ovary");
}
