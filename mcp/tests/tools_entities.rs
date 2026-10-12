mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn genes_for_go_term_with_descendants() {
    let c = client().await;
    let s = call(
        &c,
        "entities_for_term",
        json!({"term_id": "GO:0007219", "data_class": "gene", "limit": 100}),
    )
    .await;
    assert_eq!(s["term"]["name"], "Notch signaling pathway");
    assert_eq!(s["term"]["descendants"], 5);
    let e = s["entities"].as_array().unwrap();
    let n = e
        .iter()
        .find(|x| x["id"] == "FBgn0004647")
        .expect("Notch annotated");
    assert!(n["reference_count"].as_u64().unwrap() >= 10);
    assert!(n["evidence"].as_str().unwrap().contains("inferred from"));
    assert_eq!(e[0]["species"], "Dmel");
    if support::full_db() {
        assert_eq!(e[0]["id"], "FBgn0000463"); // Delta, 24 references
    }
}

#[tokio::test]
async fn alleles_for_disease_and_anatomy_terms() {
    let c = client().await;
    let d = call(
        &c,
        "entities_for_term",
        json!({"term_id": "DOID:162", "data_class": "allele", "limit": 5}),
    )
    .await;
    if support::full_db() {
        assert!(
            d["entities"][0]["qualifiers"]
                .as_str()
                .unwrap()
                .contains("model of")
        );
        assert_eq!(d["entities"].as_array().unwrap().len(), 5);
        assert_eq!(d["has_more"], true);
        assert_eq!(d["entities"][0]["id"], "FBal0060587"); // Ras85D[V12.UAS], 74 references
    } else {
        // The fixture's first page is led by puc[UAS.cMa] (ameliorates only); the seeded N[DN.UAS.cUa] is a model.
        let n = d["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "FBal0125378")
            .expect("seeded N[DN.UAS.cUa]");
        assert!(n["qualifiers"].as_str().unwrap().contains("model of"));
    }
    let w = call(&c, "entities_for_term", json!({"term_id": "FBbt:00004729", "data_class": "allele", "include_descendants": false, "species": "Dmel", "limit": 3})).await;
    assert!(w["entities"][0]["statements"].as_u64().unwrap() > 0);
    assert_eq!(w["term"]["name"], "wing");
    if support::full_db() {
        assert_eq!(w["entities"].as_array().unwrap().len(), 3);
    } else {
        assert!(
            w["entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["id"] == "FBal0012701")
        );
    }
}

#[tokio::test]
async fn disease_models_and_gene_groups() {
    let c = client().await;
    let m = call(
        &c,
        "entities_for_term",
        json!({"term_id": "DOID:1289", "data_class": "disease_model", "limit": 3}),
    )
    .await;
    assert!(!m["entities"].as_array().unwrap().is_empty());
    assert!(m["entities"][0]["name"].is_string());
    let g = call(
        &c,
        "entities_for_term",
        json!({"term_id": "GO:0007219", "data_class": "gene_group"}),
    )
    .await;
    assert!(
        g["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == "FBgg0001066")
    );
}

/// Review focus 3.
#[tokio::test]
async fn broad_term_is_refused() {
    let c = client().await;
    let r = call_raw(
        &c,
        "entities_for_term",
        json!({"term_id": "GO:0008150", "data_class": "gene"}),
    )
    .await
    .unwrap();
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "limit_exceeded");
    assert!(
        s["message"]
            .as_str()
            .unwrap()
            .contains("descendants (max 5000)")
    );
    if support::full_db() {
        // FB2026_02; the FB2026_03 fixture has 25,630.
        assert!(s["message"].as_str().unwrap().contains("25969 descendants"));
    }
    // Without descendants the same term is allowed and reports the direct-annotation page.
    let ok = call(&c, "entities_for_term", json!({"term_id": "GO:0008150", "data_class": "gene", "include_descendants": false, "limit": 2})).await;
    assert_eq!(ok["term"]["descendants_included"], false);
    assert!(ok["entities"].as_array().unwrap().len() <= 2);
}

/// The 200,000-hit cap counts only annotations the page can return (current features of the class, in the requested
/// species). FBcv:0000347 (phenotypic class, 249 descendants) reaches 200,001 phenotype statements on current alleles
/// and is refused; a Hsap filter brings it to 6,281 hits. No DOID term (the feature_cvterm allele path) and no gene
/// term trips the filtered cap on FB2026_02.
#[tokio::test]
async fn annotation_cap_is_refused() {
    let c = client().await;
    for term in ["FBcv:0000347"] {
        let r = call_raw(
            &c,
            "entities_for_term",
            json!({"term_id": term, "data_class": "allele"}),
        )
        .await
        .unwrap();
        let s = r.structured_content.unwrap();
        if support::full_db() {
            assert_eq!(s["error"], "limit_exceeded", "{term}");
            assert!(
                s["message"]
                    .as_str()
                    .unwrap()
                    .contains("more than 200000 matching annotations; add a species filter")
            );
        } else {
            // The fixture holds only the seed features' annotations, so the same call is an ordinary page.
            assert!(s["error"].is_null(), "{term}");
            assert_eq!(s["has_more"], true);
        }
        // The species filter is applied before the cap, so it is a real way out.
        let h = call(
            &c,
            "entities_for_term",
            json!({"term_id": term, "data_class": "allele", "species": "Hsap", "limit": 3}),
        )
        .await;
        let e = h["entities"].as_array().unwrap();
        assert!(!e.is_empty(), "{term}");
        assert!(e.iter().all(|a| a["species"] == "Hsap"));
    }
    // SO:0001217 (protein_coding_gene) has 218,662 raw annotations but only 16,095 on current genes, so it is a page.
    let g = call(
        &c,
        "entities_for_term",
        json!({"term_id": "SO:0001217", "data_class": "gene", "limit": 2}),
    )
    .await;
    assert_eq!(g["entities"].as_array().unwrap().len(), 2);
    assert_eq!(g["has_more"], true);
}

/// A filter that matches nothing is an empty page (no error, and the SQL's sentinel row is not reported as an entity).
#[tokio::test]
async fn empty_page_has_no_sentinel() {
    let c = client().await;
    let s = call(
        &c,
        "entities_for_term",
        json!({"term_id": "SO:0001217", "data_class": "gene", "species": "Hsap"}),
    )
    .await;
    assert_eq!(s["entities"], json!([]));
    assert_eq!(s["has_more"], false);
}

#[tokio::test]
async fn bad_term_ids() {
    let c = client().await;
    let r = call_raw(
        &c,
        "entities_for_term",
        json!({"term_id": "GO:9999999", "data_class": "gene"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
    let r = call_raw(
        &c,
        "entities_for_term",
        json!({"term_id": "wing", "data_class": "allele"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    // species is a feature filter; it has no meaning for disease models or gene groups.
    let r = call_raw(
        &c,
        "entities_for_term",
        json!({"term_id": "DOID:1289", "data_class": "disease_model", "species": "Dmel"}),
    )
    .await
    .unwrap();
    let s = r.structured_content.unwrap();
    assert_eq!(s["error"], "invalid_input");
    assert!(
        s["message"]
            .as_str()
            .unwrap()
            .contains("species does not apply to disease model")
    );
}
