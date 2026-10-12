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

#[tokio::test]
async fn similar_genes_to_notch() {
    let c = client().await;
    let s = call(
        &c,
        "rnaseq_similar_genes",
        json!({"id": "FBgn0004647", "dataset": "FlyAtlas2", "limit": 5}),
    )
    .await;
    assert_eq!(s["query"]["id"], "FBgn0004647");
    assert_eq!(s["query"]["symbol"], "N");
    assert_eq!(s["query"]["n_samples"], 43);
    let sim = s["similar"].as_array().unwrap();
    assert!(sim.len() <= 5);
    assert!(sim.iter().all(|g| g["id"] != "FBgn0004647"));
    let rs: Vec<f64> = sim
        .iter()
        .map(|g| g["correlation"].as_f64().unwrap())
        .collect();
    assert!(
        rs.windows(2).all(|w| w[0] >= w[1]),
        "sorted descending: {rs:?}"
    );
    assert!(rs.iter().all(|r| (-1.0..=1.0).contains(r)));
    assert_eq!(
        s["links"]["report"],
        "https://flybase.org/reports/FBgn0004647"
    );
    if support::full_db() {
        let compared = s["query"]["genes_compared"].as_u64().unwrap();
        assert!(
            (16_590..=16_610).contains(&compared),
            "genes_compared {compared}"
        );
        assert_eq!(sim[0]["id"], "FBgn0010768");
        assert_eq!(sim[0]["symbol"], "sqz");
        assert!((rs[0] - 0.9308).abs() < 0.002, "sqz r = {}", rs[0]);
        let m = call(&c, "rnaseq_similar_genes", json!({"id": "FBgn0004647", "dataset": "FlyAtlas2", "limit": 100, "min_correlation": 0.88})).await;
        assert!(
            m["similar"]
                .as_array()
                .unwrap()
                .iter()
                .all(|g| g["correlation"].as_f64().unwrap() >= 0.88)
        );
        assert!(m["similar"].as_array().unwrap().len() >= 3);
    } else {
        // The fixture carries 589 FlyAtlas2 genes; Notch is compared against the rest of them.
        assert!(!sim.is_empty());
        assert!(s["query"]["genes_compared"].as_u64().unwrap() > 0);
    }
    // A second call is served from the cached matrix; /health reports it (Task 8 asserts the health field).
    let again = call(
        &c,
        "rnaseq_similar_genes",
        json!({"id": "FBgn0004647", "dataset": "FlyAtlas2", "limit": 5}),
    )
    .await;
    assert_eq!(again["similar"], s["similar"]);
}

/// Review focus 3: a profile with zero variance cannot be correlated.
#[tokio::test]
async fn flat_profile_is_rejected() {
    if !support::full_db() {
        return; // FBgn0000227 is not a fixture gene; the unit test flat_rows_are_excluded covers the fixture path.
    }
    let c = client().await;
    let r = call_raw(
        &c,
        "rnaseq_similar_genes",
        json!({"id": "FBgn0000227", "dataset": "FlyAtlas2"}),
    )
    .await
    .unwrap();
    let e = r.structured_content.unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(e["message"].as_str().unwrap().contains("no variation"));
}

#[tokio::test]
async fn similar_genes_validation() {
    let c = client().await;
    let r = call_raw(
        &c,
        "rnaseq_similar_genes",
        json!({"id": "FBgn9999999", "dataset": "FlyAtlas2"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
    let r = call_raw(
        &c,
        "rnaseq_similar_genes",
        json!({"id": "FBal0012701", "dataset": "FlyAtlas2"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(
        &c,
        "rnaseq_similar_genes",
        json!({"id": "FBgn0004647", "dataset": "Nope"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let o = call(&c, "rnaseq_similar_genes", json!({"list_options": true})).await;
    assert_eq!(o["datasets"].as_array().unwrap().len(), 5);
    let r = call_raw(&c, "rnaseq_similar_genes", json!({"dataset": "FlyAtlas2"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}
