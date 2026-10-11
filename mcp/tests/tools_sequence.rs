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

#[tokio::test]
async fn unstranded_feature_returns_forward_strand() {
    let c = client().await;
    for id in ["FBsf0000428056", "FBti0067132"] {
        let s = call(&c, "get_sequence", json!({"id": id})).await;
        assert_eq!(s["strand"], 0, "{s}");
        assert!(s["coordinates"].as_str().unwrap().ends_with("(.)"), "{s}");
        assert!(s["length"].as_u64().unwrap() > 0);
        assert_eq!(s["id"], id);
    }
    let r = call_raw(
        &c,
        "get_sequence",
        json!({"id": "FBsf0000428056", "kind": "upstream", "padding": 100}),
    )
    .await
    .unwrap();
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "invalid_input");
    assert!(
        s["message"].as_str().unwrap().contains("invalid kind"),
        "{s}"
    );
    let plus = call(&c, "get_sequence", json!({"id": "FBgn0004647"})).await;
    assert_eq!(plus["strand"], 1);
}

#[tokio::test]
async fn oversized_gene_kind_is_limit_exceeded_naming_transcript_id() {
    let c = client().await;
    // dpy has 16 mRNAs totalling about 919 kb of transcript sequence.
    let r = call_raw(
        &c,
        "get_sequence",
        json!({"id": "FBgn0053196", "kind": "transcript"}),
    )
    .await
    .unwrap();
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "limit_exceeded", "{s}");
    let msg = s["message"].as_str().unwrap();
    assert!(msg.contains("transcript_id"), "{msg}");
    assert!(msg.contains("16 isoforms"), "{msg}");
    assert!(msg.contains("FBtr"), "{msg}");
    // Selecting one isoform fits; above INLINE_SEQ_MAX only FASTA is returned.
    let one = call(
        &c,
        "get_sequence",
        json!({"id": "FBgn0053196", "kind": "transcript", "transcript_id": "FBtr0332345"}),
    )
    .await;
    let recs = one["records"].as_array().unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(one["truncated"], false);
    assert!(
        recs[0]["fasta"].as_str().unwrap().len() > 60_000,
        "{}",
        recs[0]["length"]
    );
}

fn translate(cds: &str) -> String {
    const BASES: &str = "TCAG";
    const AA: &str = "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG";
    cds.as_bytes()
        .chunks(3)
        .filter(|c| c.len() == 3)
        .map(|c| {
            let idx = c
                .iter()
                .map(|b| BASES.find(*b as char).unwrap())
                .fold(0, |acc, i| acc * 4 + i);
            AA.as_bytes()[idx] as char
        })
        .collect()
}

#[tokio::test]
async fn minus_strand_cds_translates_to_stored_protein() {
    let c = client().await;
    let s = call(
        &c,
        "get_sequence",
        json!({"id": "FBgn0003996", "kind": "cds", "transcript_id": "FBtr0070490"}),
    )
    .await;
    let recs = s["records"].as_array().unwrap();
    assert_eq!(recs.len(), 1);
    let cds = recs[0]["sequence"].as_str().unwrap();
    assert_eq!(recs[0]["length"], 2064);
    assert_eq!(cds.len(), 2064);
    assert!(cds.starts_with("ATG"));
    assert!(cds.ends_with("TAG"));
    assert_eq!(cds.len() % 3, 0);
    let p = call(
        &c,
        "get_sequence",
        json!({"id": "FBgn0003996", "kind": "protein", "transcript_id": "FBtr0070490"}),
    )
    .await;
    let protein = p["records"][0]["sequence"].as_str().unwrap();
    let translated = translate(cds);
    assert!(translated.ends_with('*'));
    assert_eq!(translated.trim_end_matches('*'), protein);
}
