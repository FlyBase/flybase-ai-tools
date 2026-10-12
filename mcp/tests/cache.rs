mod support;
use serde_json::json;
use support::server::{base, call, call_raw, client};

/// Value of a counter line in /metrics whose label set contains every given label, else 0.
async fn counter(name: &str, labels: &[&str]) -> u64 {
    let body = reqwest::get(format!("{}/metrics", base()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    body.lines()
        .filter(|l| l.starts_with(&format!("{name}{{")) && labels.iter().all(|x| l.contains(x)))
        .filter_map(|l| l.rsplit(' ').next()?.parse::<f64>().ok())
        .map(|v| v as u64)
        .sum()
}

async fn hits() -> u64 {
    counter(
        "flybase_mcp_cache_events_total",
        &["cache=\"response\"", "event=\"hit\""],
    )
    .await
}

async fn misses() -> u64 {
    counter(
        "flybase_mcp_cache_events_total",
        &["cache=\"response\"", "event=\"miss\""],
    )
    .await
}

/// Review focus 5. One test, because the counters are process-global and tests in a binary run in parallel;
/// this binary holds no other test that calls a cached tool.
#[tokio::test]
async fn cache_hits_and_key_sensitivity() {
    if std::env::var("FLYBASE_MCP_RESPONSE_CACHE_MB").is_ok_and(|v| v.trim() == "0") {
        eprintln!("response cache disabled by FLYBASE_MCP_RESPONSE_CACHE_MB=0; skipping");
        return;
    }
    let c = client().await;
    let args = json!({"id": "FBgn0003996", "sections": ["identity"], "limit_per_section": 7});
    let (h0, m0) = (hits().await, misses().await);
    let first = call(&c, "get_report", args.clone()).await;
    let (h1, m1) = (hits().await, misses().await);
    assert_eq!(m1, m0 + 1, "first call is a miss");
    assert_eq!(h1, h0, "first call is not a hit");
    let second = call(&c, "get_report", args.clone()).await;
    assert_eq!(second, first, "cached result is byte-identical");
    let (h2, m2) = (hits().await, misses().await);
    assert_eq!(h2, h1 + 1, "second identical call is a hit");
    assert_eq!(m2, m1);
    call(
        &c,
        "get_report",
        json!({"id": "FBgn0003996", "sections": ["identity"], "limit_per_section": 8}),
    )
    .await;
    let (h3, m3) = (hits().await, misses().await);
    assert_eq!(
        (h3, m3),
        (h2, m2 + 1),
        "a different limit is a different key"
    );
    // Errors are never cached: an unknown ID misses twice.
    for _ in 0..2 {
        let r = call_raw(&c, "get_report", json!({"id": "FBgn9999999"}))
            .await
            .unwrap();
        assert_eq!(r.is_error, Some(true));
    }
    let (h4, m4) = (hits().await, misses().await);
    assert_eq!((h4, m4), (h3, m3 + 2));
    // A report with a failed section is returned but not stored: the probe section (registered on insertion
    // reports by Plan 2 Task 1 when FLYBASE_MCP_TEST_SECTIONS=1) times out (10 s) twice.
    for _ in 0..2 {
        let r = call(
            &c,
            "get_report",
            json!({"id": "FBti0000488", "sections": ["identity", "__timeout_probe"]}),
        )
        .await;
        assert_eq!(r["section_errors"], json!(["__timeout_probe"]));
    }
    let (h5, m5) = (hits().await, misses().await);
    assert_eq!((h5, m5), (h4, m4 + 2), "partial reports are recomputed");
    // Pure tools bypass the cache entirely.
    call(&c, "convert_coordinates", json!({"coordinates": ["X:330"]})).await;
    assert_eq!((hits().await, misses().await), (h5, m5));
    // Six concurrent identical calls: each records one event (a hit or a miss; the miss counter is incremented
    // before the coalescing step, so the split is timing-dependent). The unit test
    // identical_in_flight_calls_are_coalesced is the proof that the computation runs once.
    let fresh = json!({"id": "FBgn0004647", "sections": ["identity"], "limit_per_section": 9});
    let calls: Vec<_> = (0..6)
        .map(|_| call(&c, "get_report", fresh.clone()))
        .collect();
    futures::future::join_all(calls).await;
    let (h6, m6) = (hits().await, misses().await);
    assert_eq!(
        (m6 - m5) + (h6 - h5),
        6,
        "every call records exactly one cache event"
    );
}
