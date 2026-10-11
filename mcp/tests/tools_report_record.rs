mod support;
use serde_json::json;
use support::server::{call, client};

#[tokio::test]
async fn cell_line_report() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBtc0000150", "sections": ["identity", "properties", "references", "cvterms", "synonyms"], "limit_per_section": 3})).await;
    assert_eq!(s["type"], "cell_line");
    assert_eq!(s["sections"]["identity"]["name"], "S2R+");
    assert_eq!(s["sections"]["identity"]["species"], "Dmel");
    assert!(
        s["sections"]["properties"]["comment"]
            .as_array()
            .unwrap()
            .len()
            > 3
    );
    assert_eq!(s["sections"]["references"]["has_more"], true);
    assert!(
        s["sections"]["cvterms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["term_id"] == "FBbt:00000052")
    );
    assert!(
        s["sections"]["synonyms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "CVCL_Z831")
    );
}

#[tokio::test]
async fn strain_report() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBsn0000001"})).await;
    assert_eq!(s["type"], "strain");
    assert_eq!(s["sections"]["identity"]["name"], "Oregon-R-modENCODE");
    assert_eq!(
        s["sections"]["properties"]["strain_type"],
        json!(["wild type"])
    );
    assert_eq!(s["sections"]["references"]["count_shown"], 2);
}

#[tokio::test]
async fn library_report_with_features() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBlc0000001", "sections": ["identity", "features"], "limit_per_section": 3}),
    )
    .await;
    assert_eq!(s["type"], "library");
    assert_eq!(s["sections"]["identity"]["name"], "AT_cDNA");
    assert_eq!(s["sections"]["identity"]["type"], "reagent collection");
    let f = &s["sections"]["features"];
    assert_eq!(f["items"].as_array().unwrap().len(), 3);
    assert_eq!(f["has_more"], true);
    if support::full_db() {
        assert_eq!(f["total"], 22565); // the fixture carries the first 5,000 library_feature rows of this collection
    } else {
        assert!(f["total"].as_u64().unwrap() >= 3);
    }
    assert_eq!(f["items"][0]["type"], "cDNA_clone");
}

#[tokio::test]
async fn library_cvterms_and_props() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBlc0000085", "sections": ["cvterms", "properties"]}),
    )
    .await;
    assert!(
        s["sections"]["cvterms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["term"] == "RNA-Seq")
    );
    assert!(
        s["sections"]["properties"]["description"][0]
            .as_str()
            .unwrap()
            .starts_with("Umbrella record")
    );
}

#[tokio::test]
async fn long_tail_feature_reports() {
    let c = client().await;
    let chem = call(&c, "get_report", json!({"id": "FBch0000003"})).await;
    assert_eq!(chem["type"], "chemical");
    assert_eq!(chem["sections"]["identity"]["symbol"], "caffeine");
    assert_eq!(chem["sections"]["location"]["located"], false);
    assert!(
        chem["sections"]["properties"]["inchikey"][0]
            .as_str()
            .unwrap()
            .ends_with("UHFFFAOYSA-N")
    );

    let combo = call(&c, "get_report", json!({"id": "FBco0000001"})).await;
    assert_eq!(combo["type"], "split_system_combination");
    let rels = combo["sections"]["components"]["items"].as_array().unwrap();
    assert!(
        rels.iter()
            .any(|r| r["id"] == "FBal0304439" && r["relationship"] == "partially_produced_by")
    );
    assert!(combo["sections"].get("relationships").is_none());

    let og = call(
        &c,
        "get_report",
        json!({"id": "FBog0000245780", "sections": ["identity", "external_links"]}),
    )
    .await;
    assert_eq!(og["type"], "ortholog");
    assert_eq!(og["sections"]["identity"]["species"], "Hsap");
    assert!(
        og["sections"]["external_links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["database"] == "HGNC" && l["accession"] == "7881")
    );

    let sf = call(
        &c,
        "get_report",
        json!({"id": "FBsf0000096789", "sections": ["location"]}),
    )
    .await;
    assert_eq!(sf["type"], "sequence_feature");
    assert_eq!(sf["sections"]["location"]["chromosome"], "2L");
    assert!(sf["links"]["jbrowse"].is_string());

    for (id, ty) in [
        ("FBcl0000001", "clone"),
        ("FBte0000001", "transposable_element"),
        ("FBmc0000001", "engineered_plasmid"),
    ] {
        let s = call(
            &c,
            "get_report",
            json!({"id": id, "sections": ["identity"]}),
        )
        .await;
        assert_eq!(s["type"], ty, "{id}");
    }
}
