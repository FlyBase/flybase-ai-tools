mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

/// Review focus 5.
#[tokio::test]
async fn human_orthologs_carry_scores() {
    let c = client().await;
    let s = call(
        &c,
        "get_orthologs",
        json!({"id": "FBgn0004647", "species": ["Hsap"], "min_score": 9}),
    )
    .await;
    assert_eq!(s["query"]["symbol"], "N");
    let o = s["orthologs"].as_array().unwrap();
    let syms: Vec<&str> = o.iter().map(|x| x["symbol"].as_str().unwrap()).collect();
    assert_eq!(syms, vec!["Hsap\\NOTCH1", "Hsap\\NOTCH2", "Hsap\\NOTCH3"]);
    // Sorted by score descending and filtered by min_score; the exact DIOPT score drifts with releases.
    assert!(o[0]["score"].as_u64().unwrap() >= 9);
    assert!(
        o.windows(2)
            .all(|w| w[0]["score"].as_u64() >= w[1]["score"].as_u64())
    );
    if support::full_db() {
        assert_eq!(o[0]["score"], 11); // FB2026_02
    }
    assert_eq!(o[0]["best_score"], true);
    assert_eq!(o[0]["best_score_rev"], true);
    assert_eq!(o[0]["hgnc_id"], "HGNC:7881");
    assert!(
        o[0]["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "OrthoDB")
    );
    assert_eq!(o[0]["link"], "https://flybase.org/reports/FBog0000245780");
    assert!(s["diopt_file_loaded"].is_boolean());
}

#[tokio::test]
async fn all_species_and_best_only() {
    let c = client().await;
    // Sorted by score descending, so the score-less Drosophila pairs come last: ask for the full list.
    let s = call(
        &c,
        "get_orthologs",
        json!({"id": "FBgn0004647", "limit": 1000}),
    )
    .await;
    assert_eq!(s["count"], s["orthologs"].as_array().unwrap().len());
    assert_eq!(s["capped"], false);
    let o = s["orthologs"].as_array().unwrap();
    assert!(o.iter().any(|x| x["species"] == "Mmus"));
    assert!(
        o.iter()
            .any(|x| x["species"] == "Dsim" && x["score"].is_null())
    );
    let b = call(
        &c,
        "get_orthologs",
        json!({"id": "FBgn0004647", "best_only": true, "limit": 100}),
    )
    .await;
    assert!(
        b["orthologs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["best_score"] == true || x["best_score_rev"] == true)
    );
    assert!(b["count"].as_u64().unwrap() < s["count"].as_u64().unwrap());
}

#[tokio::test]
async fn human_gene_query_by_symbol_and_hgnc() {
    let c = client().await;
    for q in ["NOTCH1", "HGNC:7881", "notch1", "hgnc:7881"] {
        let s = call(&c, "get_orthologs", json!({"id": q})).await;
        assert_eq!(s["query"]["id"], "FBog0000245780", "{q}");
        assert_eq!(s["query"]["species"], "Hsap");
        let n = s["orthologs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == "FBgn0004647")
            .expect("Notch");
        assert!(n["score"].as_u64().unwrap() >= 1, "{q}");
        if support::full_db() {
            assert_eq!(n["score"], 11, "{q}"); // FB2026_02
        }
    }
}

#[tokio::test]
async fn bad_species_and_unknown_gene() {
    let c = client().await;
    let r = call_raw(
        &c,
        "get_orthologs",
        json!({"id": "FBgn0004647", "species": ["Martian"]}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "get_orthologs", json!({"id": "NO_SUCH_HUMAN_GENE_XYZ"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
    // Non-ASCII input must give a structured error, never a byte-index panic.
    for q in ["HGNCé1", "é", "hgnc:é", "Hsap\\é"] {
        let r = call_raw(&c, "get_orthologs", json!({"id": q}))
            .await
            .unwrap();
        let e = r.structured_content.unwrap()["error"].clone();
        assert!(e == "invalid_input" || e == "not_found", "{q}: {e}");
    }
}

/// TkR86C has 1,496 orthologs, over the 1,000-row cap: the cap must apply after scoring and ordering, so no
/// species is silently dropped, and `count` must describe what came back.
#[tokio::test]
async fn capped_gene_keeps_every_species() {
    if !support::full_db() {
        return;
    }
    let c = client().await;
    let s = call(
        &c,
        "get_orthologs",
        json!({"id": "FBgn0004841", "limit": 1000}),
    )
    .await;
    let o = s["orthologs"].as_array().unwrap();
    assert_eq!(s["count"], o.len());
    assert_eq!(o.len(), 1000);
    assert_eq!(s["capped"], true);
    for sp in ["Xtro", "Rnor", "Hsap", "Cele"] {
        assert!(o.iter().any(|x| x["species"] == sp), "{sp} present");
    }
    let scores: Vec<i64> = o.iter().filter_map(|x| x["score"].as_i64()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "ordered by score");
    let x = call(
        &c,
        "get_orthologs",
        json!({"id": "FBgn0004841", "species": ["Xtro"], "limit": 1000}),
    )
    .await;
    assert_eq!(x["capped"], false);
    assert_eq!(x["count"], 201);
}
