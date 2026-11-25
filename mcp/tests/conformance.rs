mod support;
use serde_json::json;
use support::server::{base, call, call_raw, client};

#[tokio::test]
async fn health_reports_release_migrations_backends_and_caches() {
    let v: serde_json::Value = reqwest::get(format!("{}/health", base()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["status"], "ok", "{v}");
    assert_eq!(v["release"], "FB_TEST");
    assert_eq!(v["database"], "ok");
    assert_eq!(v["migrations"], "ok");
    assert_eq!(v["blast"], "unconfigured");
    assert_eq!(v["wiki"], "unconfigured");
    assert!(v["pool"]["size"].is_number());
    assert!(v["caches"]["response"]["entries"].is_number());
    assert!(v["caches"]["rnaseq_matrix"].is_array());
    assert_eq!(v["caches"]["blast_jobs"], 0);
}

#[tokio::test]
async fn lists_tools_and_calls_list_species() {
    let c = client().await;
    let tools = c.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names.len(), 25, "tool count with the wiki tools: {names:?}");
    for t in [
        "list_species",
        "convert_coordinates",
        "rnaseq_profile_search",
        "rnaseq_similar_genes",
        "blast_submit",
        "blast_status",
        "blast_results",
        "search_wiki",
        "get_wiki_page",
    ] {
        assert!(names.contains(&t), "missing {t}");
    }
    assert!(
        tools
            .iter()
            .all(|t| t.description.as_deref().is_some_and(|d| d.len() > 40)),
        "every tool is described"
    );
    let s = call(&c, "list_species", json!({})).await;
    assert_eq!(s["release"], "FB_TEST");
    let assemblies = s["species"].as_array().unwrap();
    assert_eq!(assemblies[0]["abbreviation"], "Dmel");
    assert_eq!(assemblies[0]["genus"], "Drosophila");
    assert_eq!(assemblies[0]["species"], "melanogaster");
    assert!(assemblies.iter().all(|x| x["has_assembly"] == true));
    assert_eq!(s["count_shown"], assemblies.len());
    assert_eq!(s["assemblies_total"], assemblies.len());
    assert_eq!(s["has_more"], false);
    assert!(s["next_offset"].is_null());
    let synteny = s["muller_synteny"].as_array().unwrap();
    assert_eq!(synteny.len(), 6);
    assert_eq!(synteny[0]["muller_element"], "A");
    assert_eq!(synteny[0]["Dmel"], "X");

    let second = call(&c, "list_species", json!({"limit": 1, "offset": 1})).await;
    assert_eq!(second["species"], json!([assemblies[1]]));
    assert_eq!(second["assemblies_total"], s["assemblies_total"]);
    assert_eq!(second["organisms_total"], s["organisms_total"]);

    let all = call(&c, "list_species", json!({"include_unsequenced": true})).await;
    assert_eq!(all["count_shown"], 100);
    assert_eq!(all["next_offset"], 100);
    assert!(
        all["species"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["has_assembly"] == false)
    );
}

#[tokio::test]
async fn list_species_pages_past_the_old_cap() {
    let c = client().await;
    let (db, connection) =
        tokio_postgres::connect(&support::test_config().chado_url, tokio_postgres::NoTls)
            .await
            .unwrap();
    tokio::spawn(connection);
    let eligible: i64 = db
        .query_one(
            "select count(*) from organism where abbreviation is not null",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(eligible > 1000, "the fixture must include the old boundary");

    let mut offset = 0;
    let mut abbreviations = std::collections::HashSet::new();
    while offset < eligible {
        let page = call(
            &c,
            "list_species",
            json!({"include_unsequenced": true, "limit": 1000, "offset": offset}),
        )
        .await;
        let species = page["species"].as_array().unwrap();
        assert_eq!(species.len() as i64, (eligible - offset).min(1000));
        assert_eq!(page["count_shown"], species.len());
        assert_eq!(page["organisms_total"], eligible);
        assert_eq!(page["truncated"], false);
        for row in species {
            let abbreviation = row["abbreviation"].as_str().unwrap();
            assert!(
                abbreviations.insert(abbreviation.to_owned()),
                "{abbreviation}"
            );
        }
        offset += species.len() as i64;
        assert_eq!(page["has_more"], offset < eligible);
        assert_eq!(
            page["next_offset"],
            json!((offset < eligible).then_some(offset))
        );
    }
    let empty = call(
        &c,
        "list_species",
        json!({"include_unsequenced": true, "offset": eligible}),
    )
    .await;
    assert_eq!(empty["species"], json!([]));
    assert_eq!(empty["organisms_total"], eligible);
    assert_eq!(empty["count_shown"], 0);
    assert_eq!(empty["has_more"], false);
    assert!(empty["next_offset"].is_null());
}

#[tokio::test]
async fn unknown_argument_is_rejected() {
    let c = client().await;
    // rmcp validates arguments before the handler and returns JSON-RPC invalid_params naming the field.
    match call_raw(&c, "list_species", json!({"limt": 10})).await {
        Err(e) => assert!(
            e.to_string().contains("limt"),
            "error should name the bad field: {e}"
        ),
        Ok(res) => {
            // rmcp 3.5.1 reports a deserialize failure as a tool-level error with text content only
            // ("failed to deserialize parameters: unknown field `limt`, ..."), no structured content.
            assert_eq!(res.is_error, Some(true));
            match res.structured_content {
                Some(s) => assert_eq!(s["error"], "invalid_input"),
                None => {
                    let text = res
                        .content
                        .iter()
                        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
                        .collect::<String>();
                    assert!(
                        text.contains("limt"),
                        "error should name the bad field: {text}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn metrics_count_tool_calls() {
    let c = client().await;
    call(&c, "list_species", json!({})).await;
    let body = reqwest::get(format!("{}/metrics", base()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("# "), "{body}");
    assert!(
        body.lines()
            .any(|l| l.starts_with("flybase_mcp_tool_calls_total{")
                && l.contains("tool=\"list_species\"")
                && l.contains("outcome=\"ok\"")),
        "{body}"
    );
    assert!(body.contains("flybase_mcp_tool_duration_seconds"), "{body}");
}
