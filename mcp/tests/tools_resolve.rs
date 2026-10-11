mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn symbol_resolves_to_notch_first() {
    let c = client().await;
    let s = call(&c, "resolve", json!({"query": "N", "species": "Dmel"})).await;
    let hits = s["matches"].as_array().unwrap();
    assert_eq!(hits[0]["id"], "FBgn0004647");
    assert_eq!(hits[0]["status"], "current");
    assert_eq!(hits[0]["data_class"], "gene");
    let ids: Vec<&str> = hits.iter().map(|h| h["id"].as_str().unwrap()).collect();
    let mut dedup = ids.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(ids.len(), dedup.len(), "no duplicate IDs in matches");
}

#[tokio::test]
async fn secondary_id_resolves_to_current() {
    let c = client().await;
    let s = call(&c, "resolve", json!({"query": "FBgn0000311"})).await;
    let hit = &s["matches"][0];
    assert_eq!(hit["status"], "updated");
    assert_eq!(hit["replaced_by"], json!(["FBgn0004647"]));
    let report = s["links"]["report"].as_str().unwrap();
    assert!(report.ends_with("/FBgn0004647"), "report link {report}");
    let summary = r_summary(&c, json!({"query": "FBgn0000311"})).await;
    assert!(summary.contains("FBgn0004647"), "summary {summary}");
}

/// Text summary of a successful resolve call.
async fn r_summary(
    c: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    args: serde_json::Value,
) -> String {
    let r = call_raw(c, "resolve", args).await.unwrap();
    assert_ne!(r.is_error, Some(true));
    r.content
        .iter()
        .filter_map(|x| x.as_text().map(|t| t.text.clone()))
        .collect()
}

#[tokio::test]
async fn cell_line_and_strain_ids_are_current() {
    let c = client().await;
    for (id, class) in [("FBtc0000001", "cell_line"), ("FBsn0000001", "strain")] {
        let s = call(&c, "resolve", json!({"query": id})).await;
        let hit = &s["matches"][0];
        assert_eq!(hit["id"], id);
        assert_eq!(hit["status"], "current", "{id}");
        assert_eq!(hit["data_class"], class, "{id}");
    }
}

#[tokio::test]
async fn like_wildcards_are_literal() {
    let c = client().await;
    for q in ["%", "_", "\\", "%%%"] {
        let started = std::time::Instant::now();
        let r = call_raw(&c, "resolve", json!({"query": q})).await.unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "query {q:?} was slow"
        );
        if r.is_error == Some(true) {
            assert_eq!(
                r.structured_content.unwrap()["error"],
                "not_found",
                "query {q:?}"
            );
        } else {
            let s = r.structured_content.unwrap();
            for m in s["matches"].as_array().unwrap() {
                let matched = m["matched"].as_str().unwrap();
                assert!(
                    matched.contains(q),
                    "query {q:?} matched {matched:?} as a wildcard"
                );
            }
        }
    }
}

#[tokio::test]
async fn unknown_id_is_not_found() {
    let c = client().await;
    let r = call_raw(&c, "resolve", json!({"query": "FBgn9999999"}))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
}

#[tokio::test]
async fn full_name_case_insensitive() {
    let c = client().await;
    let s = call(
        &c,
        "resolve",
        json!({"query": "notch", "data_class": "gene"}),
    )
    .await;
    assert_eq!(s["matches"][0]["id"], "FBgn0004647");
    // The current full name outranks the stale symbol "Notch".
    assert_eq!(s["matches"][0]["matched"], "Notch");
    assert_eq!(s["matches"][0]["match_kind"], "fullname");
}

#[tokio::test]
async fn non_ascii_id_is_invalid_input() {
    let c = client().await;
    for q in ["é", "", "   "] {
        let r = call_raw(&c, "resolve", json!({"query": q})).await.unwrap();
        assert_eq!(r.is_error, Some(true), "query {q:?}");
        let code = r.structured_content.unwrap()["error"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            code == "invalid_input" || code == "not_found",
            "query {q:?} gave {code}"
        );
    }
}
