mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn region_plus_strand_length_and_coords() {
    let c = client().await;
    let s = call(
        &c,
        "get_sequence",
        json!({"region": {"chromosome": "X", "start": 3134870, "end": 3134969}}),
    )
    .await;
    assert_eq!(s["length"], 100);
    assert_eq!(s["coordinates"], "X:3134870..3134969(+)");
    assert!(
        s["fasta"]
            .as_str()
            .unwrap()
            .starts_with(">X:3134870..3134969")
    );
    assert_eq!(s["sequence"].as_str().unwrap().len(), 100);
}

#[tokio::test]
async fn region_minus_strand_is_reverse_complement() {
    let c = client().await;
    let plus = call(
        &c,
        "get_sequence",
        json!({"region": {"chromosome": "X", "start": 3134870, "end": 3134969}}),
    )
    .await;
    let minus = call(
        &c,
        "get_sequence",
        json!({"region": {"chromosome": "X", "start": 3134870, "end": 3134969, "strand": -1}}),
    )
    .await;
    let p = plus["sequence"].as_str().unwrap();
    let m = minus["sequence"].as_str().unwrap();
    assert_eq!(m.len(), 100);
    assert_eq!(flybase_mcp::backends::seq::reverse_complement(p), m);
    assert_eq!(minus["coordinates"], "X:3134870..3134969(-)");
}

#[tokio::test]
async fn region_over_cap_is_limit_exceeded() {
    let c = client().await;
    let r = call_raw(
        &c,
        "get_sequence",
        json!({"region": {"chromosome": "X", "start": 1, "end": 600000}}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
}

#[tokio::test]
async fn large_region_omits_inline_sequence() {
    let c = client().await;
    let s = call(
        &c,
        "get_sequence",
        json!({"region": {"chromosome": "X", "start": 1, "end": 200000}}),
    )
    .await;
    assert_eq!(s["length"], 200000);
    assert!(s.get("sequence").is_none());
    assert!(s["fasta"].as_str().unwrap().len() > 200000);
}

#[tokio::test]
async fn transcript_and_protein_by_id() {
    let c = client().await;
    let t = call(&c, "get_sequence", json!({"id": "FBtr0070507"})).await;
    assert_eq!(t["length"], 10150);
    let p = call(&c, "get_sequence", json!({"id": "FBpp0071769"})).await;
    assert!(p["sequence"].as_str().unwrap().starts_with('M'));
}

#[tokio::test]
async fn gene_cds_per_isoform() {
    let c = client().await;
    let s = call(
        &c,
        "get_sequence",
        json!({"id": "FBgn0004647", "kind": "cds"}),
    )
    .await;
    let recs = s["records"].as_array().unwrap();
    assert!(recs.len() >= 2);
    for r in recs {
        let seq = r["sequence"].as_str().unwrap();
        assert_eq!(seq.len() % 3, 0, "CDS length must be a multiple of 3");
        assert!(seq.starts_with("ATG"));
        assert_eq!(r["kind"], "cds");
    }
}

#[tokio::test]
async fn fbte_is_invalid_input_and_upstream_needs_padding() {
    let c = client().await;
    let r = call_raw(&c, "get_sequence", json!({"id": "FBte0000001"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(
        &c,
        "get_sequence",
        json!({"id": "FBgn0004647", "kind": "upstream"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn unsequenced_scaffold_is_not_found_not_a_crash() {
    let c = client().await;
    // 3_groupMISC is a Dpse golden_path feature with NULL seqlen and NULL residues.
    let r = call_raw(
        &c,
        "get_sequence",
        json!({"region": {"chromosome": "3_groupMISC", "start": 1, "end": 10}, "species": "Dpse"}),
    )
    .await
    .unwrap();
    assert_eq!(r.is_error, Some(true));
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "not_found");
    assert!(
        s["message"]
            .as_str()
            .unwrap()
            .contains("3_groupMISC has no stored sequence"),
        "{s}"
    );
}
