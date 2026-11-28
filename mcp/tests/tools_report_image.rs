mod support;

use flybase_mcp::{backends::chado::Chado, server::app};
use serde_json::{Value, json};
use support::server::{call, call_raw, client_at};

async fn start(configured: bool) -> (String, tokio::task::JoinHandle<()>) {
    let mut cfg = support::test_config();
    cfg.release = "FB2026_03".into();
    // The image catalog serves reports without any Chado connection.
    cfg.chado_url = "postgres://unused@127.0.0.1:1/unused".into();
    if configured {
        cfg.release_images_path = Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/release_images.json"),
        );
    }
    let chado = Chado::connect(&cfg).await.unwrap();
    let router = app(&cfg, chado).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, server)
}

#[tokio::test]
async fn image_report_returns_release_data_and_pages_annotations() {
    let (base, server) = start(true).await;
    let client = client_at(&base).await;
    let report = call(&client, "get_report", json!({"id": " fbim0000001 "})).await;
    assert_eq!(report["release"], "FB2026_03");
    assert_eq!(report["type"], "image");
    assert_eq!(report["id"], "FBim0000001");
    assert_eq!(
        report["links"]["report"],
        "https://flybase.org/reports/FBim0000001"
    );
    let sections = &report["sections"];
    assert_eq!(
        sections["identity"],
        json!({
            "id": "FBim0000001",
            "name": "Wallace (1934) drawing of male and female",
            "species": "Drosophila melanogaster",
            "created": "2006-08-07",
            "updated": "2006-08-07 18:15:50"
        })
    );
    assert!(
        sections["image_data"]["caption"]
            .as_str()
            .unwrap()
            .contains("The adult external.")
    );
    assert_eq!(
        sections["image_data"]["caption_terms"],
        json!([{"term_id": "FBbt:00004474", "name": "adult external"}])
    );
    assert_eq!(
        sections["image_data"]["image_url"],
        "https://flybase.org/images/FBim/FBim0000001.png"
    );
    assert_eq!(
        sections["image_data"]["thumbnail_url"],
        "https://flybase.org/images/FBim/FBim0000001.thumb.png"
    );
    assert_eq!(sections["references"]["items"], json!([]));
    assert!(sections.get("regions").is_none());

    let annotations = sections["annotations"]["items"].as_array().unwrap();
    assert_eq!(annotations.len(), 6);
    let abdomen = annotations
        .iter()
        .find(|a| a["term_id"] == "FBbt:00004788")
        .unwrap();
    assert_eq!(abdomen["has_region"], true);
    assert_eq!(
        abdomen["url"],
        "https://flybase.org/cgi-bin/cvreport.pl?id=FBbt:00004788"
    );
    assert_eq!(
        annotations
            .iter()
            .find(|a| a["category"] == "imaging_method")
            .unwrap()["term_id"],
        "FBbi:00000239"
    );

    for (section, count) in [("annotations", 6), ("regions", 6)] {
        let mut offset = 0;
        let mut all = Vec::new();
        loop {
            let report = call(
                &client,
                "get_report",
                json!({"id": "FBim0000001", "sections": [section], "limit_per_section": 2, "offset": offset}),
            ).await;
            let page = &report["sections"][section];
            all.extend_from_slice(page["items"].as_array().unwrap());
            let Some(next) = page["next_offset"].as_u64() else {
                break;
            };
            assert_eq!(next, offset + 2);
            offset = next;
        }
        assert_eq!(all.len(), count);
        if section == "regions" {
            assert_eq!(all[0]["term_id"], "FBbt:00004788");
            assert_eq!(all[1]["term_id"], "FBbt:00004788");
            assert_ne!(all[0]["coordinates"], all[1]["coordinates"]);
        }
    }

    let process = call(&client, "get_report", json!({"id": "FBim0000133"})).await;
    assert_eq!(
        process["sections"]["references"]["items"][0]["id"],
        "FBrf0002495"
    );
    assert_eq!(
        process["sections"]["references"]["items"][0]["url"],
        "https://flybase.org/reports/FBrf0002495"
    );
    assert_eq!(
        process["sections"]["references"]["items"][0]["citation"],
        "Strasburger, 1932, Z. wiss. Zool. 140: 539--649"
    );
    assert!(
        process["sections"]["annotations"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["category"] == "process"
                && a["term_id"] == "GO:0002168"
                && a["reference_ids"] == json!(["FBrf0002495"]))
    );
    let stage = call(
        &client,
        "get_report",
        json!({"id": "FBim0000858", "sections": ["annotations"]}),
    )
    .await;
    assert!(
        stage["sections"]["annotations"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["category"] == "developmental_stage" && a["term_id"] == "FBdv:00005336")
    );
    let superscript = call(&client, "get_report", json!({"id": "FBim0000997"})).await;
    assert!(
        superscript["sections"]["identity"]["name"]
            .as_str()
            .unwrap()
            .contains("Antp[LC]")
    );
    assert!(
        superscript["sections"]["image_data"]["caption"]
            .as_str()
            .unwrap()
            .contains("Antp[LC]")
    );
    server.abort();
}

#[tokio::test]
async fn image_report_distinguishes_missing_images_and_missing_projection() {
    let (base, server) = start(true).await;
    let client = client_at(&base).await;
    for (args, code) in [
        (json!({"id": "FBim9999999"}), "not_found"),
        (json!({"id": "FBim0000001", "offset": 1}), "invalid_input"),
        (
            json!({"id": "FBim0000001", "sections": ["image_data"], "offset": 1}),
            "invalid_input",
        ),
    ] {
        let report = call_raw(&client, "get_report", args).await.unwrap();
        assert_eq!(report.structured_content.unwrap()["error"], code);
    }
    server.abort();

    let (base, server) = start(false).await;
    let client = client_at(&base).await;
    let options = call(
        &client,
        "get_report",
        json!({"id": "FBim0000001", "list_options": true}),
    )
    .await;
    assert_eq!(options["optional_sections"], json!(["regions"]));
    let report = call_raw(&client, "get_report", json!({"id": "FBim0000001"}))
        .await
        .unwrap();
    let body: Value = report.structured_content.unwrap();
    assert_eq!(body["error"], "backend_unavailable");
    server.abort();
}
