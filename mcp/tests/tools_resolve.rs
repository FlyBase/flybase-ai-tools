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
            started.elapsed() < std::time::Duration::from_secs(3),
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

#[tokio::test]
async fn species_is_case_insensitive_and_validated() {
    let c = client().await;
    let upper = call(&c, "resolve", json!({"query": "N", "species": "Dmel"})).await;
    let lower = call(&c, "resolve", json!({"query": "N", "species": "dmel"})).await;
    assert_eq!(lower["matches"][0]["id"], "FBgn0004647");
    // Only the top hit is compared: the candidate query's 200-row cutoff is not order-stable on ties.
    assert_eq!(lower["matches"][0], upper["matches"][0]);
    assert!(
        lower["matches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["species"] == "Dmel")
    );
    let r = call_raw(
        &c,
        "resolve",
        json!({"query": "N", "species": "Dmelanogaster"}),
    )
    .await
    .unwrap();
    assert_eq!(r.is_error, Some(true));
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "invalid_input");
    assert!(
        s["message"].as_str().unwrap().contains("list_species"),
        "{}",
        s["message"]
    );
}

#[tokio::test]
async fn lowercase_fb_prefix_resolves_as_id() {
    let c = client().await;
    for q in ["fbgn0004647", "FBGN0004647"] {
        let s = call(&c, "resolve", json!({"query": q})).await;
        assert_eq!(s["matches"][0]["id"], "FBgn0004647", "{q}");
        assert_eq!(s["matches"][0]["status"], "current", "{q}");
    }
}

#[tokio::test]
async fn stock_number_resolves_to_stock_id() {
    let c = client().await;
    let s = call(
        &c,
        "resolve",
        json!({"query": "28813", "data_class": "stock"}),
    )
    .await;
    assert_eq!(s["matches"][0]["id"], "FBst0028813");
    assert_eq!(s["matches"][0]["match_kind"], "stock_number");
}

#[tokio::test]
async fn cell_line_name_resolves_via_index() {
    let c = client().await;
    // Kc167 is the current symbol of FBtc0000001 in cell_line_synonym.
    let s = call(
        &c,
        "resolve",
        json!({"query": "Kc167", "data_class": "cell_line"}),
    )
    .await;
    assert_eq!(s["matches"][0]["id"], "FBtc0000001");
    assert_eq!(s["matches"][0]["data_class"], "cell_line");
    assert_eq!(s["matches"][0]["species"], "Dmel");
    // Strains and libraries are indexed the same way.
    let st = call(
        &c,
        "resolve",
        json!({"query": "Oregon-R-modENCODE", "data_class": "strain"}),
    )
    .await;
    assert_eq!(st["matches"][0]["id"], "FBsn0000001");
}

#[tokio::test]
async fn text_content_repeats_structured_body() {
    let c = client().await;
    let r = call_raw(&c, "resolve", json!({"query": "FBgn0004647"}))
        .await
        .unwrap();
    let text = r.content[0].as_text().unwrap().text.clone();
    let (_, json) = text.split_once("\n\n").expect("summary, blank line, JSON");
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(parsed, r.structured_content.unwrap());
}

#[tokio::test]
async fn name_without_species_does_not_error() {
    // FBtc0000033's names are indexed with a NULL species.
    let c = client().await;
    let s = call(&c, "resolve", json!({"query": "ML83-63"})).await;
    assert_eq!(s["matches"][0]["id"], "FBtc0000033");
    assert_eq!(s["matches"][0]["data_class"], "cell_line");
}

#[tokio::test]
async fn notch_name_ranks_gene_before_alleles() {
    let c = client().await;
    for q in ["Notch", "notch"] {
        let s = call(&c, "resolve", json!({"query": q, "species": "Dmel"})).await;
        let hits = s["matches"].as_array().unwrap();
        let ids: Vec<&str> = hits.iter().map(|h| h["id"].as_str().unwrap()).collect();
        assert_eq!(ids[0], "FBgn0004647", "query {q:?}: {ids:?}");
        assert_eq!(hits[0]["data_class"], "gene");
        if support::full_db() {
            // Two alleles carry "notch"/"Notch" as synonyms and tie with the gene's full name on score;
            // the class tie-break puts the gene ahead of them.
            assert_eq!(ids[1..3], ["FBal0002094", "FBal0017897"], "query {q:?}");
        } else {
            // The fixture carries no tying allele; the gene still leads and the insertion follows.
            assert_eq!(ids[1], "FBti0024374", "query {q:?}");
        }
    }
}
