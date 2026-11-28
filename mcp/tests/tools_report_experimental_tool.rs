mod support;
use serde_json::json;
use support::server::{call, client};

#[tokio::test]
async fn experimental_tool_identity_and_uses() {
    let c = client().await;
    let options = call(
        &c,
        "get_report",
        json!({"id": "fbto0000001", "list_options": true}),
    )
    .await;
    assert_eq!(options["type"], "experimental_tool");
    assert_eq!(
        options["default_sections"],
        json!(["identity", "description", "uses"])
    );
    assert_eq!(
        options["optional_sections"],
        json!([
            "related_tools",
            "constructs",
            "insertions",
            "synonyms",
            "secondary_ids",
            "external_links",
            "references"
        ])
    );

    let resolved = call(&c, "resolve", json!({"query": "fbto0000001"})).await;
    assert_eq!(resolved["matches"][0]["id"], "FBto0000001");
    assert_eq!(resolved["matches"][0]["data_class"], "experimental_tool");

    let report = call(&c, "get_report", json!({"id": "fbto0000001"})).await;
    assert_eq!(report["id"], "FBto0000001");
    assert_eq!(
        report["links"]["report"],
        "https://flybase.org/reports/FBto0000001"
    );
    let sections = &report["sections"];
    assert_eq!(sections["identity"]["symbol"], "C-Cerulean");
    assert_eq!(sections["identity"]["name"], "Cerulean C-terminal fragment");
    let description = &sections["description"]["items"][0];
    assert_eq!(description["kind"], "description");
    assert!(
        description["text"]
            .as_str()
            .unwrap()
            .starts_with("C-Cerulean corresponds to a C-terminal portion")
    );
    assert_eq!(description["reference_ids"], json!(["FBrf0238070"]));
    let scfv = call(
        &c,
        "get_report",
        json!({"id": "FBto0000778", "sections": ["description"]}),
    )
    .await;
    assert!(
        scfv["sections"]["description"]["items"][0]["text"]
            .as_str()
            .unwrap()
            .contains("binds the Tag:HA epitope")
    );
    assert_eq!(
        sections["uses"]["items"],
        json!([{
            "id": "FBcv:0005052",
            "name": "split fluorescent protein",
            "url": "https://flybase.org/cgi-bin/cvreport.pl?id=FBcv:0005052",
            "reference_ids": ["FBrf0238070"]
        }])
    );
}

#[tokio::test]
async fn experimental_tool_linked_records_and_evidence() {
    let c = client().await;
    let report = call(
        &c,
        "get_report",
        json!({
            "id": "FBto0000001",
            "sections": ["related_tools", "constructs", "insertions", "synonyms", "secondary_ids"],
            "limit_per_section": 100
        }),
    )
    .await;
    assert!(report.get("section_errors").is_none(), "{report}");
    let sections = &report["sections"];
    let related = sections["related_tools"]["items"].as_array().unwrap();
    assert!(related.iter().any(|tool| {
        tool["id"] == "FBto0000051"
            && tool["category"] == "compatible"
            && tool["relationships"] == json!(["compatible_tool"])
            && tool["use_ids"] == json!(["FBcv:0005052"])
            && tool["reference_ids"] == json!(["FBrf0238070"])
    }));
    assert!(related.iter().any(|tool| {
        tool["category"] == "common_gene_origin" && tool["origin_gene"]["id"] == "FBgn0014446"
    }));

    let construct = sections["constructs"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["component_id"] == "FBal0348459")
        .unwrap();
    assert_eq!(construct["relationship"], "tagged_with");
    assert_eq!(construct["construct_ids"], json!(["FBtp0134360"]));
    assert_eq!(construct["tag_ids"], json!(["FBto0000001"]));
    assert_eq!(construct["reference_ids"], json!(["FBrf0240351"]));
    assert!(construct["stock_count"].is_u64());

    assert_eq!(sections["insertions"]["items"], json!([]));
    assert!(
        sections["synonyms"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| {
                name["name"] == "T:Avic\\GFP[CC]" && name["reference_ids"] == json!(["FBrf0105495"])
            })
    );
    assert_eq!(sections["secondary_ids"]["items"], json!(["FBal0265902"]));

    let references = call(
        &c,
        "get_report",
        json!({"id": "FBto0000001", "sections": ["references"], "limit_per_section": 2}),
    )
    .await;
    let first = &references["sections"]["references"];
    assert_eq!(first["has_more"], true);
    assert_eq!(first["next_offset"], 2);
    let references = call(
        &c,
        "get_report",
        json!({"id": "FBto0000001", "sections": ["references"], "offset": 2, "limit_per_section": 100}),
    )
    .await;
    let rest = &references["sections"]["references"];
    assert_eq!(rest["count_shown"], 3);
    assert_eq!(rest["next_offset"], json!(null));
}

#[tokio::test]
async fn experimental_tool_insertion_links_and_sparse_sections() {
    let c = client().await;
    let report = call(
        &c,
        "get_report",
        json!({"id": "FBto0000349", "sections": ["insertions"], "limit_per_section": 1}),
    )
    .await;
    assert!(report.get("section_errors").is_none(), "{report}");
    let insertions = &report["sections"]["insertions"];
    assert_eq!(insertions["has_more"], true);
    let item = &insertions["items"][0];
    assert!(item["id"].as_str().unwrap().starts_with("FBti"));
    assert!(item["construct_id"].as_str().unwrap().starts_with("FBtp"));
    assert!(
        item["tool_annotations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|annotation| {
                annotation["relationship"] == "carries_tool"
                    && !annotation["reference_ids"].as_array().unwrap().is_empty()
            })
    );

    let loxp = call(
        &c,
        "get_report",
        json!({"id": "FBto0000349", "sections": ["description"]}),
    )
    .await;
    assert!(
        loxp["sections"]["description"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "notes")
    );

    let cerulean = call(
        &c,
        "get_report",
        json!({"id": "FBto0000008", "sections": ["external_links"]}),
    )
    .await;
    assert!(
        cerulean["sections"]["external_links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| {
                link["database"] == "FPbase"
                    && link["accession"] == "CVUYM"
                    && link["url"].is_string()
            })
    );

    let sparse = call(
        &c,
        "get_report",
        json!({
            "id": "FBto0000713",
            "sections": ["identity", "uses", "related_tools", "constructs", "insertions", "secondary_ids", "external_links"]
        }),
    )
    .await;
    assert!(sparse.get("section_errors").is_none(), "{sparse}");
    let sections = &sparse["sections"];
    assert_eq!(sections["identity"]["symbol"], "Tag:NLS(His2B)");
    assert_eq!(sections["uses"]["items"][0]["id"], "FBcv:0005024");
    for name in ["related_tools", "constructs", "insertions", "secondary_ids"] {
        assert_eq!(sections[name]["items"], json!([]), "{name}");
        assert_eq!(sections[name]["has_more"], false, "{name}");
    }
    assert_eq!(sections["external_links"], json!([]));
}

#[tokio::test]
async fn experimental_tool_current_symbol_keeps_all_synonym_citations() {
    let c = client().await;
    let report = call(
        &c,
        "get_report",
        json!({"id": "FBto0000097", "sections": ["synonyms"]}),
    )
    .await;
    let current_symbol: Vec<_> = report["sections"]["synonyms"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["name"] == "mTagBFP2" && item["kind"] == "symbol")
        .collect();
    assert_eq!(
        current_symbol,
        vec![&json!({
            "name": "mTagBFP2",
            "kind": "symbol",
            "is_current": true,
            "reference_ids": ["FBrf0105495", "FBrf0238070"]
        })]
    );
}
