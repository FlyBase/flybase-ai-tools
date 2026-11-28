mod support;
use serde_json::{Value, json};
use support::server::{call, call_raw, client};

#[tokio::test]
async fn fbig_report_connects_gene_pairs_to_experimental_evidence() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBIG0000104993"})).await;

    assert_eq!(s["id"], "FBig0000104993");
    assert_eq!(s["type"], "physical_interaction");
    assert_eq!(
        s["links"]["report"],
        "https://flybase.org/reports/FBig0000104993"
    );
    let identity = &s["sections"]["identity"];
    assert_eq!(
        identity["interaction_types"],
        json!(["physical association"])
    );
    assert_eq!(identity["genes"][0]["symbol"], "fz");
    assert_eq!(identity["genes"][1]["id"], "FBgn0284084");
    assert_eq!(identity["genes"][1]["symbol"], "wg");
    assert_eq!(
        identity["genes"][1]["network_url"],
        "https://flybase.org/cgi-bin/get_interactions.html?items=FBgn0284084&mode=ppi"
    );

    let page = &s["sections"]["reported_interactions"];
    assert_eq!(page["count_shown"], 4);
    assert_eq!(page["has_more"], false);
    let record = &page["items"][1];
    assert_eq!(record["record_id"], "FBrf0152103-1.AT.AP");
    assert_eq!(record["reference_ids"], json!(["FBrf0152103"]));
    assert_eq!(record["molecule_pair"], "protein-protein");
    assert!(record["assays"].as_array().unwrap().contains(&json!({
        "id": "MI:0411", "name": "enzyme linked immunosorbent assay"
    })));
    assert_eq!(record["cell_lines"][0]["name"], "S2-DGRC");
    assert!(
        record["comments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|comment| {
                comment["text"] == "The wg-fz binding affinity was measured at 45 nM."
                    && comment["reference_ids"] == json!(["FBrf0152103"])
            })
    );
    let participants = record["participants"].as_array().unwrap();
    assert!(participants.iter().any(|p| {
        p["gene_ids"] == json!(["FBgn0284084"])
            && p["role"] == "bait"
            && p["feature_id"] == "FBpp0090940"
            && p["reported_as"] == json!(["Wg"])
    }));
    assert!(participants.iter().any(|p| {
        p["gene_ids"] == json!(["FBgn0001085"])
            && p["kind"] == "subregion"
            && p["name"] == "cysteine-rich domain (CRD)"
            && p["role"] == "sufficient binding region"
    }));
    assert!(
        page["items"][3]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|term| { term["kind"] == "stage" && term["name"] == "third instar larval stage" })
    );
}

#[tokio::test]
async fn fbig_reports_the_gene_pair_in_a_complex() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBig0000000059", "sections": ["reported_interactions"]}),
    )
    .await;
    let page = &s["sections"]["reported_interactions"];
    assert_eq!(page["count_shown"], 2);
    assert_eq!(page["items"][0]["collections"][0]["name"], "DPiM-2");
    let experiment = &page["items"][1];
    assert_eq!(experiment["record_id"], "FBrf0225716-98.CM.MS");
    let participants = experiment["participants"].as_array().unwrap();
    let mut genes: Vec<&Value> = participants
        .iter()
        .filter(|p| p["kind"] == "participant")
        .map(|p| &p["gene_ids"])
        .collect();
    genes.sort_by_key(|g| g[0].as_str());
    assert_eq!(
        genes,
        vec![&json!(["FBgn0016691"]), &json!(["FBgn0038224"])]
    );
}

#[tokio::test]
async fn fbig_resolves_current_ids_and_rejects_obsolete_ids() {
    let c = client().await;
    let s = call(&c, "resolve", json!({"query": "fbig0000104993"})).await;
    assert_eq!(s["matches"][0]["id"], "FBig0000104993");
    assert_eq!(s["matches"][0]["data_class"], "physical_interaction");
    for id in ["FBig0000000001", "FBig9999999999"] {
        let err = call_raw(
            &c,
            "get_report",
            json!({"id": id, "sections": ["references"]}),
        )
        .await
        .unwrap();
        assert_eq!(err.is_error, Some(true), "{id}: {err:?}");
        assert_eq!(err.structured_content.unwrap()["error"], "not_found");
    }
}

#[tokio::test]
async fn fbig_paginates_records_and_citations() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBig0000104993", "sections": ["reported_interactions", "references"], "limit_per_section": 1}),
    ).await;
    let page = &s["sections"]["reported_interactions"];
    assert_eq!(page["count_shown"], 1);
    assert_eq!(page["has_more"], true);
    let refs = &s["sections"]["references"];
    assert_eq!(refs["items"][0]["id"], "FBrf0244419");
    assert_eq!(refs["items"][0]["pubmed_id"], "31846454");
    assert_eq!(refs["next_offset"], 1);

    let s = call(
        &c,
        "get_report",
        json!({"id": "FBig0000104993", "sections": ["references"], "limit_per_section": 3, "offset": 1}),
    ).await;
    let refs = &s["sections"]["references"];
    assert_eq!(refs["count_shown"], 3);
    assert_eq!(refs["items"][0]["id"], "FBrf0217131");
    assert_eq!(refs["items"][2]["id"], "FBrf0089594");
    assert_eq!(refs["next_offset"], Value::Null);
}

#[tokio::test]
async fn fbig_preserves_comparison_operators_in_comments() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBig0000040694", "sections": ["reported_interactions"]}),
    )
    .await;
    let records = s["sections"]["reported_interactions"]["items"]
        .as_array()
        .unwrap();
    let study = records
        .iter()
        .find(|record| record["record_id"] == "FBrf0227576-49.FRET")
        .unwrap();
    assert!(study["comments"].as_array().unwrap().iter().any(|comment| {
        comment["text"]
            .as_str()
            .unwrap()
            .contains("Kd < 5000 nm were curated by FlyBase.")
    }));
}

#[tokio::test]
async fn fbig_resolves_participants_stored_as_genes() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBig0000113090", "sections": ["reported_interactions"], "limit_per_section": 1}),
    )
    .await;
    let first = &s["sections"]["reported_interactions"]["items"][0];
    assert_eq!(first["record_id"], "FBrf0180423-6.coIP.WB");
    assert!(
        first["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|participant| {
                participant["feature_id"] == "FBgn0260397"
                    && participant["gene_ids"] == json!(["FBgn0260397"])
            })
    );
}
