mod support;
use serde_json::json;
use support::server::{base, call, call_raw, client};

#[tokio::test]
async fn health_reports_release() {
    let v: serde_json::Value = reqwest::get(format!("{}/health", base()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["release"], "FB_TEST");
    assert_eq!(v["database"], "ok");
}

#[tokio::test]
async fn lists_tools_and_calls_list_species() {
    let c = client().await;
    let tools = c.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|t| t.name == "list_species"));
    let s = call(&c, "list_species", json!({})).await;
    assert_eq!(s["release"], "FB_TEST");
    assert!(
        s["species"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["abbreviation"] == "Dmel")
    );
    assert!(
        s["species"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["has_assembly"] == true)
    );
    assert!(s["muller_synteny"].as_array().unwrap().len() >= 6);
    let all = call(&c, "list_species", json!({"include_unsequenced": true})).await;
    assert!(
        all["species"].as_array().unwrap().len() > s["species"].as_array().unwrap().len(),
        "include_unsequenced must add organisms without assemblies"
    );
    assert!(
        all["species"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["has_assembly"] == false)
    );
}

#[tokio::test]
async fn unknown_argument_is_rejected() {
    let c = client().await;
    // rmcp validates arguments before the handler and returns JSON-RPC invalid_params naming the field.
    match call_raw(&c, "list_species", json!({"limit": "ten"})).await {
        Err(e) => assert!(
            e.to_string().contains("limit"),
            "error should name the bad field: {e}"
        ),
        Ok(res) => {
            // rmcp 3.5.1 reports a deserialize failure as a tool-level error with text content only
            // ("failed to deserialize parameters: unknown field `limit`, ..."), no structured content.
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
                        text.contains("limit"),
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
