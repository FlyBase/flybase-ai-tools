mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn converts_a_mixed_batch() {
    let c = client().await;
    let s = call(&c, "convert_coordinates", json!({"coordinates": ["2R:1..10000", "3RHet:1964215..1936890", "2L:22420300..22420400", "2L:21485600", "nonsense"]})).await;
    assert_eq!(s["pair"]["from_release"], 5);
    assert_eq!(s["pair"]["to_release"], 6);
    let v = s["conversions"].as_array().unwrap();
    assert_eq!(v.len(), 5);
    assert_eq!(v[0]["status"], "mapped");
    assert_eq!(
        v[0]["to"],
        json!({"chromosome": "2R", "start": 4112496, "end": 4122495})
    );
    assert_eq!(v[1]["status"], "mapped");
    assert_eq!(v[1]["to"]["chromosome"], "3R");
    assert_eq!(v[1]["strand_flipped"], true);
    assert_eq!(v[1]["regions_spanned"], 5);
    assert_eq!(v[1]["from_length"], 27326);
    assert_eq!(v[1]["to_length"], 61424);
    assert_eq!(v[2]["status"], "partial");
    assert!(v[2]["to_length"].is_null());
    assert_eq!(v[2]["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(v[3]["status"], "unmapped");
    assert_eq!(v[4]["status"], "invalid");
    assert!(
        v[4]["notes"][0]
            .as_str()
            .unwrap()
            .contains("chromosome:position")
    );
    assert_eq!(
        s["counts"],
        json!({"mapped": 2, "partial": 1, "unmapped": 1, "invalid": 1})
    );
    assert_eq!(
        s["links"]["converter"],
        "https://flybase.org/convert/coordinates"
    );
}

#[tokio::test]
async fn list_options_and_unsupported_pair() {
    let c = client().await;
    let o = call(&c, "convert_coordinates", json!({"list_options": true})).await;
    assert_eq!(o["pairs"][0]["species"], "Dmel");
    assert_eq!(o["pairs"][0]["from_release"], 5);
    assert!(
        o["pairs"][0]["chromosomes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "2LHet")
    );
    let r = call_raw(
        &c,
        "convert_coordinates",
        json!({"from_release": 4, "to_release": 5, "coordinates": ["X:1"]}),
    )
    .await
    .unwrap();
    let e = r.structured_content.unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(e["message"].as_str().unwrap().contains("Dmel 5 -> 6"));
    assert!(e["message"].as_str().unwrap().contains("from_release"));
    let r = call_raw(
        &c,
        "convert_coordinates",
        json!({"to_release": 7, "coordinates": ["X:1"]}),
    )
    .await
    .unwrap();
    let e = r.structured_content.unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(e["message"].as_str().unwrap().contains("to_release"), "{e}");
    let r = call_raw(&c, "convert_coordinates", json!({"species": "Dpse", "from_release": 2, "to_release": 3, "coordinates": ["XL_group1a:1"]})).await.unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
}

#[tokio::test]
async fn batch_limits() {
    let c = client().await;
    let r = call_raw(&c, "convert_coordinates", json!({"coordinates": []}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let many: Vec<String> = (0..501).map(|i| format!("X:{}", i + 1)).collect();
    let r = call_raw(&c, "convert_coordinates", json!({"coordinates": many}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "limit_exceeded");
}

#[tokio::test]
async fn truncated_batch_says_how_many_are_shown() {
    let c = client().await;
    let wide: Vec<String> = (0..500)
        .map(|i| format!("2LHet:{}..300000", i + 1))
        .collect();
    let r = call_raw(&c, "convert_coordinates", json!({"coordinates": wide}))
        .await
        .unwrap();
    let s = r.structured_content.clone().unwrap();
    assert_eq!(s["truncated"], true);
    let shown = s["conversions"].as_array().unwrap().len();
    assert!(shown < 500);
    let text = r.content[0].as_text().unwrap().text.clone();
    let (summary, _) = text.split_once("\n\n").unwrap();
    assert!(
        summary.contains(&format!("Showing {shown} of 500 conversions")),
        "{summary}"
    );
    assert!(summary.contains("smaller batches"), "{summary}");
}
