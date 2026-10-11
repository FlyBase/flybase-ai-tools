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
    assert_eq!(o[0]["score"], 11);
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
    for q in ["NOTCH1", "HGNC:7881", "notch1"] {
        let s = call(&c, "get_orthologs", json!({"id": q})).await;
        assert_eq!(s["query"]["id"], "FBog0000245780", "{q}");
        assert_eq!(s["query"]["species"], "Hsap");
        let n = s["orthologs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == "FBgn0004647")
            .expect("Notch");
        assert_eq!(n["score"], 11);
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
}
