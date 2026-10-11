mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn groups_for_gene_and_group() {
    let c = client().await;
    let s = call(
        &c,
        "get_gene_groups_and_pathways",
        json!({"id": "FBgn0004647"}),
    )
    .await;
    assert!(
        s["pathways"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "FBgg0001064" && g["is_pathway"] == true)
    );
    assert!(
        s["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "FBgg0001066" && g["is_pathway"] == false)
    );
    assert!(s["pathways"].as_array().unwrap().iter().all(|g| {
        g["link"]
            .as_str()
            .unwrap()
            .ends_with(g["id"].as_str().unwrap())
    }));
    let g = call(
        &c,
        "get_gene_groups_and_pathways",
        json!({"id": "FBgg0000040"}),
    )
    .await;
    assert_eq!(g["type"], "gene_group");
    assert_eq!(g["sections"]["identity"]["symbol"], "E3-RBR");
    if support::full_db() {
        // Member genes are not fixture seeds (Task 4 gates the same way).
        assert_eq!(g["sections"]["members"]["count_shown"], 6);
    }
    assert!(
        g["sections"]["hierarchy"]["parents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "FBgg0000069")
    );
    let r = call_raw(
        &c,
        "get_gene_groups_and_pathways",
        json!({"id": "FBal0012701"}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn disease_models_for_gene_allele_entry_and_term() {
    let c = client().await;
    let g = call(
        &c,
        "get_disease_models",
        json!({"id": "FBgn0004647", "limit": 5}),
    )
    .await;
    assert!(
        g["disease_associations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["term_id"] == "DOID:0060227" && d["qualifier"] == "model of")
    );
    assert_eq!(
        g["allele_annotations"]["items"].as_array().unwrap().len(),
        5
    );
    assert_eq!(g["allele_annotations"]["has_more"], true);
    let first = &g["allele_annotations"]["items"][0];
    assert_eq!(first["term_id"], "DOID:0060227");
    assert_eq!(first["reference_count"], 1);
    assert!(first["reference"].as_str().unwrap().starts_with("FBrf"));
    assert!(
        g["disease_model_entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"] == "FBhh0000679" && h["role"] == "dmel_gene_implicated")
    );
    // Page two continues the same ordering.
    let g2 = call(
        &c,
        "get_disease_models",
        json!({"id": "FBgn0004647", "limit": 5, "offset": 5}),
    )
    .await;
    assert_eq!(g2["offset"], 5);
    assert_ne!(
        g2["allele_annotations"]["items"][0]["allele_id"],
        first["allele_id"]
    );

    let a = call(&c, "get_disease_models", json!({"id": "FBal0125378"})).await;
    assert!(a.get("disease_associations").is_none());
    let ann = a["allele_annotations"]["items"].as_array().unwrap();
    assert!(
        ann.iter().any(|x| x["term_id"] == "DOID:162"
            && x["qualifiers"].as_str().unwrap().contains("model of"))
    );
    assert!(ann[0]["reference"].as_str().unwrap().starts_with("FBrf"));
    assert_eq!(ann[0]["reference_count"], 1);
    assert_eq!(ann[0]["allele_id"], "FBal0125378");
    assert_eq!(ann[0]["allele_symbol"], "N[DN.UAS.cUa]");
    assert!(ann[0]["evidence"].is_string());

    let h = call(
        &c,
        "get_disease_models",
        json!({"id": "FBhh0000003", "limit": 2}),
    )
    .await;
    assert_eq!(h["type"], "disease_model");
    assert_eq!(h["sections"]["identity"]["name"], "Huntington disease");
    assert_eq!(
        h["sections"]["variants"]["items"][0]["allele_id"],
        "FBal0093284"
    );
    assert_eq!(h["sections"]["variants"]["count_shown"], 2);
    if support::full_db() {
        // Hsap\HTT is not a fixture seed feature.
        assert!(
            h["sections"]["human_genes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["id"] == "FBgn0025587")
        );
    }

    // DOID:1289 has 231 entries ordered by name; FBhh0000001 ("polyglutamine diseases, polyQ models") is past page one.
    let d = call(
        &c,
        "get_disease_models",
        json!({"id": "DOID:1289", "limit": 3}),
    )
    .await;
    assert_eq!(d["term"]["id"], "DOID:1289");
    assert_eq!(d["term"]["name"], "neurodegenerative disease");
    assert_eq!(d["disease_model_entries"][0]["id"], "FBhh0000110"); // Alzheimer disease
    assert_eq!(d["disease_model_entries"].as_array().unwrap().len(), 3);
    assert_eq!(d["disease_model_entries_has_more"], true);
    let descendants = d["term"]["descendants"].as_i64().unwrap();
    if support::full_db() {
        assert_eq!(descendants, 321);
        let items = d["allele_annotations"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["term_id"], "DOID:1289");
        assert_eq!(items[0]["disease"], "neurodegenerative disease");
        assert!(items[0]["reference"].is_null());
        assert!(items[0]["reference_count"].as_u64().unwrap() >= 1);
        assert!(items[0]["qualifiers"].is_string());
        assert_eq!(d["allele_annotations"]["has_more"], true);
    } else {
        assert!(descendants > 100);
    }
    let direct = call(
        &c,
        "get_disease_models",
        json!({"id": "DO:1289", "limit": 100, "offset": 100}),
    )
    .await;
    assert_eq!(direct["offset"], 100);
    assert!(
        direct["disease_model_entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"] == "FBhh0000001")
    ); // row 170 of 231 by name
    let r = call_raw(&c, "get_disease_models", json!({"id": "DOID:4"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
    let r = call_raw(&c, "get_disease_models", json!({"id": "GO:0007219"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "get_disease_models", json!({"id": "FBti0000870"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "get_disease_models", json!({"id": "DOID:999999999"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
}

#[tokio::test]
async fn stocks_for_gene_with_filters() {
    let c = client().await;
    let s = call(&c, "get_stocks", json!({"id": "FBgn0004647", "limit": 10})).await;
    let total = s["total"].as_i64().unwrap();
    if support::full_db() {
        assert_eq!(total, 164);
    } else {
        assert!(total > 100);
    }
    assert_eq!(s["stocks"].as_array().unwrap().len(), 10);
    assert_eq!(s["has_more"], true);
    assert_eq!(s["stocks"][0]["id"], "FBst0000045");
    assert_eq!(s["stocks"][0]["provider"], "Bloomington");
    assert_eq!(s["stocks"][0]["allele_symbol"], "N[fa-1]");
    assert_eq!(s["stocks"][0]["via_regulatory_region"], false);
    assert_eq!(s["split_system_combinations"], json!([]));
    let k = call(
        &c,
        "get_stocks",
        json!({"id": "FBgn0004647", "provider": "kyoto", "limit": 100}),
    )
    .await;
    assert!(
        k["stocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|st| st["provider"] == "Kyoto")
    );
    assert!(!k["stocks"].as_array().unwrap().is_empty());
    assert!(k["total"].as_i64().unwrap() < total);
    assert_eq!(k["has_more"], false);
    let g = call(
        &c,
        "get_stocks",
        json!({"id": "FBgn0260400", "gal4_only": true, "limit": 100}),
    )
    .await;
    assert_eq!(g["total"], 17);
    assert!(g["stocks"].as_array().unwrap().iter().all(|st| {
        st["allele_symbol"]
            .as_str()
            .unwrap()
            .starts_with("Scer\\GAL4")
            && st["via_regulatory_region"] == true
    }));
    assert!(
        g["stocks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|st| st["id"] == "FBst0008765")
    );
    // For an allele or insertion the flag keeps stocks that also carry a Scer\GAL4 allele.
    let none = call(
        &c,
        "get_stocks",
        json!({"id": "FBal0012701", "gal4_only": true}),
    )
    .await;
    assert!(none["stocks"].as_array().unwrap().is_empty());
    assert_eq!(none["has_more"], false);
    let ti = call(
        &c,
        "get_stocks",
        json!({"id": "FBti0000870", "gal4_only": true, "provider": "kyoto", "limit": 10}),
    )
    .await;
    let ids: Vec<&str> = ti["stocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|st| st["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["FBst0306721", "FBst0306765", "FBst0306766"]);
    assert!(ti.get("total").is_none());
}

#[tokio::test]
async fn stocks_for_allele_insertion_balancer_and_stock() {
    let c = client().await;
    let a = call(&c, "get_stocks", json!({"id": "FBal0012701"})).await;
    assert_eq!(a["stocks"][0]["id"], "FBst0028813");
    assert_eq!(a["stocks"][0]["genotype"], "N[55e11] P{neoFRT}19A/FM7c");
    assert_eq!(a["split_system_combinations"], json!([])); // N[55e11] is in no split-GAL4 combination
    let i = call(
        &c,
        "get_stocks",
        json!({"id": "FBti0000870", "provider": "kyoto", "limit": 2}),
    )
    .await;
    assert_eq!(i["stocks"].as_array().unwrap().len(), 2);
    assert_eq!(i["stocks"][0]["id"], "FBst0300227");
    assert_eq!(i["has_more"], true);
    let i2 = call(
        &c,
        "get_stocks",
        json!({"id": "FBti0000870", "provider": "kyoto", "limit": 2, "offset": 2}),
    )
    .await;
    assert_eq!(i2["stocks"][0]["id"], "FBst0305132");
    let b = call(&c, "get_stocks", json!({"id": "FM6", "limit": 5})).await;
    assert_eq!(b["id"], "FBba0000003");
    assert_eq!(b["stocks"].as_array().unwrap().len(), 5);
    assert_eq!(b["stocks"][0]["id"], "FBst0000028");
    let st = call(&c, "get_stocks", json!({"id": "FBst0028813"})).await;
    assert_eq!(st["stocks"].as_array().unwrap().len(), 1);
    assert_eq!(st["stocks"][0]["id"], "FBst0028813");
    assert_eq!(st["stocks"][0]["provider"], "Bloomington");
    assert_eq!(
        st["stocks"][0]["provider_name"],
        "Bloomington Drosophila Stock Center"
    );
    assert_eq!(st["stocks"][0]["stock_number"], "28813");
    assert_eq!(st["has_more"], false);
    // gal4_only is ignored for a stock ID, and the summary (the text block) says so.
    let ignored = call_raw(
        &c,
        "get_stocks",
        json!({"id": "FBst0028813", "gal4_only": true}),
    )
    .await
    .unwrap();
    assert_eq!(
        ignored.structured_content.as_ref().unwrap()["stocks"][0]["id"],
        "FBst0028813"
    );
    let text = ignored.content[0].as_text().unwrap().text.clone();
    assert!(text.contains("gal4_only does not apply"), "{text}");
    let miss = call(
        &c,
        "get_stocks",
        json!({"id": "FBst0028813", "provider": "kyoto"}),
    )
    .await;
    assert_eq!(miss["stocks"], json!([]));
    let combo = call(&c, "get_stocks", json!({"id": "FBal0304439"})).await;
    assert!(
        combo["split_system_combinations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == "FBco0000001" && x["stocks_count"] == 0)
    );
}

#[tokio::test]
async fn stocks_rejects_other_ids() {
    let c = client().await;
    let r = call_raw(&c, "get_stocks", json!({"id": "FBrf0227604"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "get_stocks", json!({"id": "no_such_balancer_xyz"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
    // An unknown provider fails schema deserialization (see conformance.rs for the two rmcp shapes).
    match call_raw(
        &c,
        "get_stocks",
        json!({"id": "FBgn0004647", "provider": "harvard"}),
    )
    .await
    {
        Err(e) => assert!(e.to_string().contains("provider"), "{e}"),
        Ok(res) => assert_eq!(res.is_error, Some(true)),
    }
    let r = call_raw(
        &c,
        "get_stocks",
        json!({"id": "FBgn0004647", "limit": 100, "offset": 9950}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}
