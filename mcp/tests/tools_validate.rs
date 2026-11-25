mod support;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use support::server::{call, call_raw, client};

#[tokio::test]
async fn mixed_batch_of_ids_symbols_and_external_ids() {
    let c = client().await;
    let s = call(&c, "validate_ids", json!({"ids": ["FBgn0004647", "FBgn0000311", "FBgn9999999", "N", "P07207", "318824", "HGNC:7881", "25637572", "xyzzy_no_such_thing"], "species": "Dmel"})).await;
    let by_input = |k: &str| {
        s["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["input"] == k)
            .unwrap()
            .clone()
    };
    assert_eq!(by_input("FBgn0004647")["status"], "current");
    assert_eq!(by_input("FBgn0004647")["symbol"], "N");
    assert_eq!(by_input("FBgn0000311")["status"], "updated");
    assert_eq!(
        by_input("FBgn0000311")["current_ids"],
        json!(["FBgn0004647"])
    );
    assert_eq!(by_input("FBgn9999999")["status"], "unknown");
    let n = by_input("N");
    assert_eq!(n["status"], "matched");
    assert_eq!(n["matched_by"], "symbol");
    assert_eq!(n["current_ids"][0], "FBgn0004647");
    let up = by_input("P07207");
    assert_eq!(up["matched_by"], "external_id");
    assert_eq!(up["status"], "ambiguous");
    assert!(
        up["current_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i == "FBgn0004647")
    );
    // 318824 is dpy's Entrez Gene ID and not a Bloomington stock number (Notch's 31293 is both, and stock numbers win).
    assert_eq!(by_input("318824")["current_ids"], json!(["FBgn0053196"]));
    assert_eq!(by_input("318824")["matched_by"], "external_id");
    assert_eq!(
        by_input("HGNC:7881")["current_ids"],
        json!(["FBog0000245780"])
    );
    assert_eq!(by_input("25637572")["current_ids"], json!(["FBrf0227604"]));
    assert_eq!(by_input("25637572")["matched_by"], "pubmed_id");
    assert_eq!(by_input("xyzzy_no_such_thing")["status"], "unknown");
    assert_eq!(s["counts"]["current"], 1);
}

#[tokio::test]
async fn one_result_per_input_in_order() {
    let c = client().await;
    let s = call(
        &c,
        "validate_ids",
        json!({"ids": ["28813", "N", "", "N", "FBgn0004647"]}),
    )
    .await;
    let r = s["results"].as_array().unwrap();
    assert_eq!(r.len(), 5);
    assert_eq!(r[0]["current_ids"], json!(["FBst0028813"]));
    assert_eq!(r[0]["matched_by"], "stock_number");
    assert_eq!(r[2]["status"], "invalid");
    assert_eq!(r[1]["input"], "N");
    assert_eq!(r[3]["input"], "N");
    assert_eq!(r[1]["current_ids"], r[3]["current_ids"]);
    assert_eq!(r[4]["status"], "current");
    assert!(s.get("offset").is_none());
    assert!(s.get("total").is_none());
    assert!(s.get("next_offset").is_none());
}

#[tokio::test]
async fn target_class_filters_matches() {
    let c = client().await;
    let s = call(
        &c,
        "validate_ids",
        json!({"ids": ["P07207"], "target_class": "gene"}),
    )
    .await;
    assert_eq!(s["results"][0]["current_ids"], json!(["FBgn0004647"]));
    assert_eq!(s["results"][0]["data_class"], "gene");
    // The symbol belongs to the surviving gene, not to the first polypeptide or GenBank protein row the query returned.
    assert_eq!(s["results"][0]["symbol"], "N");
    assert_eq!(s["results"][0]["status"], "matched");
}

#[tokio::test]
async fn ambiguous_symbol_lists_all_candidates() {
    let c = client().await;
    // "Notch" is the exact-case full name of the gene and of allele FBal0017897.
    let s = call(&c, "validate_ids", json!({"ids": ["Notch"]})).await;
    let r = &s["results"][0];
    assert_eq!(r["status"], "ambiguous");
    assert!(
        r["current_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i == "FBgn0004647")
    );
    assert!(r["current_ids"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn request_and_pagination_limits() {
    let c = client().await;
    let ids = vec!["FBgn0004647"; 50_001];
    let r = call_raw(&c, "validate_ids", json!({"ids": ids}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
    let r = call_raw(&c, "validate_ids", json!({"ids": []}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    for options in [
        json!({"offset": 1}),
        json!({"limit": 0}),
        json!({"limit": 1001}),
    ] {
        let mut args = options;
        args["ids"] = json!(["FBgn0004647"]);
        let r = call_raw(&c, "validate_ids", args).await.unwrap();
        assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    }
}

#[tokio::test]
async fn fifty_thousand_inputs_match_unpaged_results() {
    let c = client().await;
    let examples = [
        " FBgn0004647 ",
        "fbgn0004647",
        "FBgn0000311",
        "FBgn9999999",
        "Notch",
        "N",
        "P07207",
        "318824",
        "28813",
        "HGNC:7881",
        "hgnc:7881",
        "25637572",
        "xyzzy_no_such_thing",
        "",
    ];
    let small = call(&c, "validate_ids", json!({"ids": examples})).await;
    let expected: HashMap<_, _> = examples
        .into_iter()
        .zip(small["results"].as_array().unwrap())
        .collect();
    let mut ids: Vec<_> = examples.into_iter().cycle().take(50_000).collect();
    // Duplicate an ambiguous input across an exact page boundary.
    ids[999] = "P07207";
    ids[1000] = "P07207";
    let mut args = json!({"ids": ids});
    let mut seen = 0;
    loop {
        let page = call(&c, "validate_ids", args.clone()).await;
        assert_eq!(page["offset"], seen);
        assert_eq!(page["total"], ids.len());
        assert_eq!(page["truncated"], false);
        let rows = page["results"].as_array().unwrap();
        assert!(!rows.is_empty());
        assert!(rows.len() <= 1000);
        if seen == 0 {
            assert_eq!(rows.len(), 1000);
        }
        let mut counts = BTreeMap::new();
        for row in rows {
            assert_eq!(row, expected[ids[seen]], "input at offset {seen}");
            *counts.entry(row["status"].as_str().unwrap()).or_insert(0) += 1;
            seen += 1;
        }
        assert_eq!(page["counts"], json!(counts));
        if page["next_offset"].is_null() {
            break;
        }
        assert_eq!(page["next_offset"], seen);
        args["offset"] = page["next_offset"].clone();
    }
    assert_eq!(seen, ids.len());
}

#[tokio::test]
async fn dense_symbol_lookup_preserves_all_ambiguous_candidates() {
    let c = client().await;
    // Many libraries share this full name. Case variants remain distinct inputs after deduplication.
    let name = "d. melanogaster, s2-drsc cell line, source for chip.";
    let positions: Vec<_> = name
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| b.is_ascii_alphabetic().then_some(i))
        .take(8)
        .collect();
    let ids: Vec<_> = (0..256)
        .map(|mask| {
            let mut input = name.as_bytes().to_vec();
            for (bit, &pos) in positions.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    input[pos].make_ascii_uppercase();
                }
            }
            String::from_utf8(input).unwrap()
        })
        .collect();
    let mut args = json!({"ids": ids, "species": "Dmel"});
    let error = call_raw(&c, "validate_ids", args.clone())
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(error["error"], "limit_exceeded");
    assert!(error["message"].as_str().unwrap().contains("smaller limit"));

    let page_size = 64;
    let mut expected = Vec::new();
    for chunk in ids[..page_size].chunks(8) {
        let small = call(&c, "validate_ids", json!({"ids": chunk, "species": "Dmel"})).await;
        expected.extend(small["results"].as_array().unwrap().iter().cloned());
    }
    let candidate_count: usize = expected
        .iter()
        .map(|row| row["current_ids"].as_array().unwrap().len())
        .sum();
    assert!(
        candidate_count > 5_000,
        "fixture must exercise the old SQL cap"
    );

    args["limit"] = json!(page_size);
    let result = call(&c, "validate_ids", args).await;
    assert_eq!(result["results"], json!(expected));
    assert_eq!(result["counts"], json!({"ambiguous": page_size}));
    assert_eq!(result["next_offset"], page_size);
    assert_eq!(result["truncated"], false);
}

#[tokio::test]
async fn wide_results_resume_at_the_first_unreturned_input() {
    let c = client().await;
    // The pre-existing invalid row echoes the full input. Make 400 rows exceed the JSON envelope.
    let mut ids = vec!["x".repeat(1000); 400];
    ids[0] = "Notch".into();
    let mut args = json!({"ids": ids});
    let page = call(&c, "validate_ids", args.clone()).await;
    let rows = page["results"].as_array().unwrap();
    let count = rows.len();
    assert!(count > 1 && count < ids.len());
    assert_eq!(page["next_offset"], count);
    assert_eq!(page["total"], ids.len());
    assert_eq!(
        page["counts"],
        json!({"ambiguous": 1, "invalid": count - 1})
    );
    let single = call(&c, "validate_ids", json!({"ids": ["Notch"]})).await;
    assert_eq!(rows[0], single["results"][0]);
    assert_eq!(page["truncated"], false);

    args["offset"] = json!(count);
    let next = call(&c, "validate_ids", args).await;
    assert_eq!(next["results"][0]["input"], ids[count]);
    assert_eq!(next["offset"], count);
    assert_eq!(next["next_offset"], Value::Null);
    assert_eq!(next["results"].as_array().unwrap().len(), ids.len() - count);
    assert_eq!(next["counts"], json!({"invalid": ids.len() - count}));
    assert_eq!(next["truncated"], false);
}

#[tokio::test]
async fn oversized_invalid_input_gets_a_bounded_display_and_keeps_its_row() {
    let c = client().await;
    let input = format!(" {} ", "é".repeat(140_000));
    let result = call(
        &c,
        "validate_ids",
        json!({"ids": ["FBgn0004647", input, "FBgn0000311"]}),
    )
    .await;
    let rows = result["results"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["status"], "current");
    assert_eq!(rows[1]["status"], "invalid");
    assert_eq!(rows[1]["input"], "é".repeat(200));
    assert_eq!(rows[1]["input_truncated"], true);
    assert_eq!(rows[1]["input_bytes"], input.len());
    assert_eq!(rows[2]["status"], "updated");
    assert_eq!(result["truncated"], false);
    assert!(result.get("next_offset").is_none());
}

/// The HGNC prefix is case-insensitive, as in get_orthologs; each input is answered as written, and a non-ASCII
/// accession after the prefix is simply unknown.
#[tokio::test]
async fn hgnc_prefix_is_case_insensitive() {
    let c = client().await;
    let s = call(
        &c,
        "validate_ids",
        json!({"ids": ["hgnc:7881", "HGNC:7881", "hgnc:é"]}),
    )
    .await;
    let r = s["results"].as_array().unwrap();
    assert_eq!(r[0]["input"], "hgnc:7881");
    assert_eq!(r[0]["matched_by"], "external_id");
    assert_eq!(r[0]["current_ids"], json!(["FBog0000245780"]));
    assert_eq!(r[0]["current_ids"], r[1]["current_ids"]);
    assert_eq!(r[2]["status"], "unknown");
}
