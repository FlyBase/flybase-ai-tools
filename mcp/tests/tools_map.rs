mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn map_region_finds_notch_and_insertions() {
    let c = client().await;
    let s = call(&c, "map_features", json!({"region": {"chromosome": "X", "start": 3134870, "end": 3172221}, "feature_types": ["gene", "insertion"], "limit": 100})).await;
    assert_eq!(s["region"]["chromosome"], "X");
    let f = s["features"].as_array().unwrap();
    let notch = f
        .iter()
        .find(|x| x["id"] == "FBgn0004647")
        .expect("Notch overlaps its own span");
    assert_eq!(notch["overlap"], "within");
    assert_eq!(notch["start"], 3134870);
    if support::full_db() {
        assert!(
            f.iter()
                .any(|x| x["type"] == "transposable_element_insertion_site")
        ); // insertions in the span are not fixture seeds
    }
    assert!(
        f.iter()
            .all(|x| x["type"] == "gene" || x["type"] == "transposable_element_insertion_site")
    );
}

#[tokio::test]
async fn map_by_id_with_padding_and_paging() {
    let c = client().await;
    let a = call(
        &c,
        "map_features",
        json!({"id": "FBgn0004647", "padding": 10000, "feature_types": ["gene"], "limit": 2}),
    )
    .await;
    assert_eq!(a["region"]["start"], 3124870);
    if support::full_db() {
        assert_eq!(a["features"].as_array().unwrap().len(), 2);
        assert_eq!(a["has_more"], true);
        let b = call(&c, "map_features", json!({"id": "FBgn0004647", "padding": 10000, "feature_types": ["gene"], "limit": 2, "offset": 2})).await;
        assert_ne!(a["features"][0]["id"], b["features"][0]["id"]);
    } else {
        assert!(
            a["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["id"] == "FBgn0004647")
        );
    }
}

#[tokio::test]
async fn map_rejects_bad_input_and_lists_types() {
    let c = client().await;
    let r = call_raw(
        &c,
        "map_features",
        json!({"region": {"chromosome": "X", "start": 1, "end": 600000}}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
    let r = call_raw(
        &c,
        "map_features",
        json!({"id": "FBgn0004647", "region": {"chromosome": "X", "start": 1, "end": 10}}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "map_features", json!({"id": "FBrf0227604"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let o = call(&c, "map_features", json!({"list_options": true})).await;
    assert!(
        o["feature_types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "insertion"
                && t["so_type"] == "transposable_element_insertion_site")
    );
}

#[tokio::test]
async fn cytosearch_band_and_range() {
    let c = client().await;
    let s = call(&c, "cytosearch", json!({"location": "91A1"})).await;
    assert_eq!(s["resolved"]["chromosome"], "3R");
    assert_eq!(s["resolved"]["start"], 18268611);
    assert_eq!(s["resolved"]["end"], 18297020);
    assert_eq!(s["resolved"]["bands"][0]["level"], "band");
    let r = call(
        &c,
        "cytosearch",
        json!({"location": "91A1-91B2", "feature_types": ["gene"], "limit": 100}),
    )
    .await;
    assert_eq!(r["resolved"]["end"], 18510198);
    assert_eq!(r["resolved"]["bands"].as_array().unwrap().len(), 2);
    let g = call(
        &c,
        "cytosearch",
        json!({"location": "3R:18268611..18297020", "feature_types": ["gene"]}),
    )
    .await;
    assert_eq!(g["resolved"]["start"], 18268611);
    if support::full_db() {
        // The fixture has featureloc rows for seed features only, none of them in 91A.
        assert!(!s["features"].as_array().unwrap().is_empty());
        assert_eq!(r["aberrations_has_more"], true);
        assert_eq!(g["features"][0]["type"], "gene");
    } else {
        assert_eq!(g["has_more"], false); // no seed feature lies in 91A1
    }
}

#[tokio::test]
async fn cytosearch_partial_filter_and_division() {
    let c = client().await;
    // 3C7 (X:3102328..3143194) overlaps the start of Notch (X:3134870..3172221), so Notch is a partial hit.
    let with = call(
        &c,
        "cytosearch",
        json!({"location": "3C7", "feature_types": ["gene"], "limit": 100}),
    )
    .await;
    assert!(
        with["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == "FBgn0004647" && f["overlap"] == "partial")
    );
    let without = call(&c, "cytosearch", json!({"location": "3C7", "feature_types": ["gene"], "include_partial": false, "limit": 100})).await;
    assert!(
        without["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["id"] != "FBgn0004647")
    );
    let div = call(
        &c,
        "cytosearch",
        json!({"location": "91", "feature_types": ["gene"], "include_aberrations": false}),
    )
    .await;
    assert_eq!(div["resolved"]["bands"][0]["level"], "division");
    if support::full_db() {
        assert_eq!(div["has_more"], true);
    }
}

/// Review focus 4.
#[tokio::test]
async fn band_range_must_share_an_arm() {
    let c = client().await;
    for bad in [
        "91A1-3C7",
        "91Z",
        "91A1-",
        "3R:5..1",
        "",
        "chr3R:1..2",
        "3R:1..99999999999",
    ] {
        let r = call_raw(&c, "cytosearch", json!({"location": bad}))
            .await
            .unwrap();
        assert_eq!(r.is_error, Some(true), "{bad}");
        let s = r.structured_content.unwrap();
        assert_eq!(s["error"], "invalid_input", "{bad}");
        assert!(s["message"].as_str().unwrap().contains("location"), "{bad}");
    }
    let r = call_raw(&c, "cytosearch", json!({"location": "999A1"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
}

/// Insertion sites are zero-length in chado (fmin == fmax); they are reported as points, never start > end.
#[tokio::test]
async fn point_insertions_have_start_equal_end() {
    let c = client().await;
    let s = call(&c, "map_features", json!({"region": {"chromosome": "3L", "start": 3337600, "end": 3337700}, "feature_types": ["insertion"], "limit": 100})).await;
    let f = s["features"].as_array().unwrap();
    let p = f
        .iter()
        .find(|x| x["id"] == "FBti0000488")
        .expect("FBti0000488 is at 3L:3337664 in both databases");
    assert_eq!(p["start"], 3337664);
    assert_eq!(p["end"], 3337664);
    assert!(
        f.iter()
            .all(|x| x["start"].as_i64().unwrap() <= x["end"].as_i64().unwrap())
    );
}

/// An arm that is not a golden_path feature is rejected up front, not answered with an empty success.
#[tokio::test]
async fn map_rejects_unknown_chromosome() {
    let c = client().await;
    for chrom in ["chrX", "x", "3"] {
        let r = call_raw(
            &c,
            "map_features",
            json!({"region": {"chromosome": chrom, "start": 1, "end": 10}}),
        )
        .await
        .unwrap();
        assert_eq!(r.is_error, Some(true), "{chrom}");
        let s = r.structured_content.unwrap();
        assert_eq!(s["error"], "invalid_input", "{chrom}");
        let msg = s["message"].as_str().unwrap();
        assert!(msg.contains("region.chromosome"), "{chrom}: {msg}");
        assert!(msg.contains(&format!("'{chrom}'")), "{chrom}: {msg}");
    }
}
