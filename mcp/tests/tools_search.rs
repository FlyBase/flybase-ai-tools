mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn substring_finds_notch_gene() {
    let c = client().await;
    let s = call(
        &c,
        "search",
        json!({"query": "otch", "data_class": "gene", "species": "Dmel"}),
    )
    .await;
    assert!(
        s["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"] == "FBgn0004647")
    );
    assert!(s["has_more"].is_boolean());
}

#[tokio::test]
async fn two_char_query_is_prefix_only() {
    let c = client().await;
    let s = call(&c, "search", json!({"query": "No", "data_class": "gene"})).await;
    for h in s["hits"].as_array().unwrap() {
        assert!(
            h["name"].as_str().unwrap().to_lowercase().starts_with("no"),
            "{h}"
        );
    }
}

#[tokio::test]
async fn reference_by_author_and_year() {
    let c = client().await;
    let s = call(
        &c,
        "search",
        json!({"query": "Notch", "data_class": "reference", "year": 2015, "limit": 5}),
    )
    .await;
    let hits = s["hits"].as_array().unwrap();
    assert!(!hits.is_empty() && hits.len() <= 5);
    for h in hits {
        assert_eq!(h["data_class"], "reference");
        assert_eq!(h["year"], "2015");
    }
}

#[tokio::test]
async fn short_query_rejected() {
    let c = client().await;
    let r = call_raw(&c, "search", json!({"query": "N"})).await.unwrap();
    assert_eq!(r.is_error, Some(true));
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn pagination_offset_shifts_results() {
    let c = client().await;
    let a = call(
        &c,
        "search",
        json!({"query": "wing", "data_class": "cv_term", "limit": 3}),
    )
    .await;
    let b = call(
        &c,
        "search",
        json!({"query": "wing", "data_class": "cv_term", "limit": 3, "offset": 3}),
    )
    .await;
    assert_ne!(a["hits"][0]["id"], b["hits"][0]["id"]);
}

#[tokio::test]
async fn like_wildcards_match_literally() {
    let c = client().await;
    // Unescaped, '%' would be a wildcard and this would match Notch; escaped, nothing contains "%otch".
    let s = call(
        &c,
        "search",
        json!({"query": "%otch", "data_class": "gene", "species": "Dmel"}),
    )
    .await;
    assert_eq!(s["hits"], json!([]), "{s}");
    let r = call(
        &c,
        "search",
        json!({"query": "N_tch", "data_class": "reference"}),
    )
    .await;
    assert_eq!(r["hits"], json!([]), "{r}");
}

#[tokio::test]
async fn query_without_letters_or_digits_returns_no_hits() {
    let c = client().await;
    for class in ["gene", "reference", "cv_term"] {
        let s = call(&c, "search", json!({"query": "-- ", "data_class": class})).await;
        assert_eq!(s["hits"], json!([]), "{class}: {s}");
        assert_eq!(s["has_more"], false);
    }
}

#[tokio::test]
async fn reference_author_filter_matches_surname() {
    let c = client().await;
    let s = call(&c, "search", json!({"query": "Notch", "data_class": "reference", "author": "artavanis-tsakonas", "limit": 5})).await;
    let hits = s["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 5);
    assert_eq!(s["has_more"], true);
    for h in hits {
        assert!(
            h["authors"]
                .as_str()
                .unwrap()
                .contains("Artavanis-Tsakonas"),
            "{h}"
        );
    }
}

#[tokio::test]
async fn species_is_case_insensitive_and_validated() {
    let c = client().await;
    let s = call(
        &c,
        "search",
        json!({"query": "otch", "data_class": "gene", "species": "dmel"}),
    )
    .await;
    assert!(
        s["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"] == "FBgn0004647")
    );
    assert!(
        s["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["species"] == "Dmel")
    );
    let r = call_raw(
        &c,
        "search",
        json!({"query": "otch", "species": "Dmelanogaster"}),
    )
    .await
    .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn deep_offset_is_rejected() {
    let c = client().await;
    let r = call_raw(
        &c,
        "search",
        json!({"query": "wing", "limit": 100, "offset": 9_950}),
    )
    .await
    .unwrap();
    assert_eq!(r.is_error, Some(true));
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "invalid_input");
    assert!(s["message"].as_str().unwrap().contains("offset"));
    let ok = call(
        &c,
        "search",
        json!({"query": "wing", "limit": 100, "offset": 9_900}),
    )
    .await;
    assert!(ok["hits"].is_array());
}

#[tokio::test]
async fn cv_term_hits_link_to_cv_report() {
    let c = client().await;
    let s = call(
        &c,
        "search",
        json!({"query": "wing", "data_class": "cv_term", "limit": 3}),
    )
    .await;
    let hits = s["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    for h in hits {
        let link = h["link"].as_str().unwrap();
        assert!(
            link.starts_with("https://flybase.org/cgi-bin/cvreport.pl?id="),
            "{link}"
        );
        assert!(link.ends_with(h["id"].as_str().unwrap()), "{link}");
    }
    assert_eq!(s["links"]["first_report"], hits[0]["link"]);
    let g = call(
        &c,
        "search",
        json!({"query": "otch", "data_class": "gene", "species": "Dmel"}),
    )
    .await;
    let first = &g["hits"][0];
    assert_eq!(
        first["link"],
        format!(
            "https://flybase.org/reports/{}",
            first["id"].as_str().unwrap()
        )
    );
    assert_eq!(g["links"]["first_report"], first["link"]);
}

#[tokio::test]
async fn cell_line_class_is_searchable() {
    let c = client().await;
    let s = call(
        &c,
        "search",
        json!({"query": "Kc1", "data_class": "cell_line"}),
    )
    .await;
    assert!(
        s["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"] == "FBtc0000001"),
        "{s}"
    );
}
