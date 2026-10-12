//! HTTP API tests (design 3.12, Task 4) against the fake BLAST+ scripts in `tests/fake-blast/`. Each
//! test starts its own router on an ephemeral port over a fresh state root; the fake reads its controls
//! from `<state_root>/fake-blast.env`.
use blast_jobs::advanced::DENY_LIST;
use blast_jobs::auth::Auth;
use blast_jobs::catalogue::Catalogue;
use blast_jobs::config::Config;
use blast_jobs::http::{self, AppState, MAX_BODY_BYTES};
use blast_jobs::runner::{self, HEADER};
use blast_jobs::store::Store;
use blast_jobs::types::Program;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

const TOKEN: &str = "test-token-0123456789";
const VALID_UNKNOWN_ID: &str = "AAAAAAAAAAAAAAAAAAAAAA";

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Server {
    tmp: tempfile::TempDir,
    base: String,
    http: reqwest::Client,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Starts the API over the fixture catalogue and the fake BLAST binaries. `overrides` are merged into
/// the config; `controls` is written to `fake-blast.env`.
async fn start(overrides: Value, controls: &str) -> Server {
    start_with(overrides, controls, None).await
}

/// As `start`; `workers` replaces the runner's workers (for the stopped-worker health case).
async fn start_with(
    overrides: Value,
    controls: &str,
    workers: Option<Vec<JoinHandle<()>>>,
) -> Server {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("fake-blast.env"), controls).unwrap();
    let fixture = manifest().join("tests/fixture");
    let mut v = json!({
        "bind": "127.0.0.1:0",
        "blast_bin_dir": manifest().join("tests/fake-blast"),
        "database_root": fixture.join("db"),
        "catalogue": fixture.join("catalogue.json"),
        "database_release": "FB2023_04",
        "state_root": state,
    });
    for (k, val) in overrides.as_object().unwrap() {
        v[k] = val.clone();
    }
    let path = tmp.path().join("config.json");
    std::fs::write(&path, v.to_string()).unwrap();
    let cfg = Arc::new(Config::load(&path).unwrap());
    let cat = Arc::new(Catalogue::load(&cfg.catalogue, &cfg.database_root).unwrap());
    let store = Store::open(&cfg.state_root, cfg.max_queued).unwrap();
    let mut tasks = Vec::new();
    // Worker tasks end with the test's runtime; the state keeps their handles for health.
    let workers = workers.unwrap_or_else(|| runner::spawn_workers(cfg.clone(), store.clone()));
    let state = AppState::new(cfg.clone(), cat, store, Auth::Token(TOKEN.into()), workers);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tasks.push(tokio::spawn(async move {
        axum::serve(listener, http::router(state)).await.unwrap();
    }));
    Server {
        tmp,
        base: format!("http://{addr}"),
        http: reqwest::Client::new(),
        tasks,
    }
}

fn request(program: &str) -> Value {
    let p = Program::parse(program).unwrap();
    json!({
        "program": program,
        "database": if p.protein_database() { "translation" } else { "assembly" },
        "species": ["dmel"],
        "sequence": if p.protein_query() { ">query\nMKLVQWERTY\n" } else { ">query\nACGTACGTACGT\n" },
        "client": "flybase-mcp",
    })
}

impl Server {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn post(&self, body: &Value) -> reqwest::Response {
        self.http
            .post(self.url("/internal/jobs"))
            .bearer_auth(TOKEN)
            .json(body)
            .send()
            .await
            .unwrap()
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.http
            .get(self.url(path))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
    }

    /// Submits and returns the `202` body.
    async fn submit(&self, body: &Value) -> Value {
        let r = self.post(body).await;
        assert_eq!(r.status(), StatusCode::ACCEPTED, "{:?}", r.text().await);
        r.json().await.unwrap()
    }

    async fn status(&self, id: &str) -> Value {
        let r = self.get(&format!("/internal/jobs/{id}")).await;
        assert_eq!(r.status(), StatusCode::OK);
        r.json().await.unwrap()
    }

    /// Polls until `state` matches `pred`, returning the last status.
    async fn wait_for(&self, id: &str, within: Duration, pred: impl Fn(&str) -> bool) -> Value {
        let end = Instant::now() + within;
        loop {
            let s = self.status(id).await;
            if pred(s["state"].as_str().unwrap()) {
                return s;
            }
            assert!(Instant::now() < end, "job {id} still {s} after {within:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_terminal(&self, id: &str) -> Value {
        self.wait_for(id, Duration::from_secs(20), |s| {
            matches!(s, "complete" | "failed")
        })
        .await
    }

    async fn health(&self, query: &str) -> (StatusCode, Value) {
        let r = self
            .http
            .get(self.url(&format!("/internal/health{query}")))
            .send()
            .await
            .unwrap();
        (r.status(), r.json().await.unwrap())
    }
}

fn canned_rows() -> String {
    std::fs::read_to_string(manifest().join("tests/fake-blast/canned.tsv")).unwrap()
}

#[tokio::test]
async fn happy_path_submit_status_results() {
    let s = start(json!({"concurrency": 1}), "FAKE_BLAST_SLEEP=1\n").await;
    let a = s.submit(&request("blastn")).await;
    let keys: Vec<&String> = a.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "database_release",
            "job_id",
            "poll_after_seconds",
            "queue_position",
            "state"
        ]
    );
    assert_eq!(a["state"], "queued");
    assert_eq!(a["database_release"], "FB2023_04");
    assert_eq!(a["queue_position"], 1);
    assert_eq!(a["poll_after_seconds"], 5, "an idle slot is free");
    let id = a["job_id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 22);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );

    // Running: position 0. A second job queued behind it: position 1, poll later.
    let running = s
        .wait_for(&id, Duration::from_secs(5), |st| st == "running")
        .await;
    assert_eq!(running["queue_position"], 0);
    let b = s.submit(&request("blastn")).await;
    assert_eq!(b["queue_position"], 1);
    assert_eq!(b["poll_after_seconds"], 10, "the only slot is busy");
    let bid = b["job_id"].as_str().unwrap().to_string();
    let queued = s.status(&bid).await;
    assert_eq!(queued["state"], "queued");
    assert_eq!(queued["queue_position"], 1);

    let done = s.wait_terminal(&id).await;
    assert_eq!(
        done,
        json!({"job_id": id, "state": "complete"}),
        "no result_url, downloads, error or queue_position once complete"
    );

    let r = s.get(&format!("/internal/jobs/{id}/results.tsv")).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        r.headers()["content-type"],
        "text/tab-separated-values; charset=utf-8"
    );
    assert!(r.headers().get("x-blast-jobs-truncated").is_none());
    let len: usize = r.headers()["content-length"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let body = r.text().await.unwrap();
    assert_eq!(len, body.len());
    assert_eq!(body, format!("{HEADER}{}", canned_rows()));
    let rows: Vec<&str> = body.lines().skip(1).collect();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.split('\t').count() == 13));
    s.wait_terminal(&bid).await;
}

#[tokio::test]
async fn job_routes_need_the_bearer_token_and_health_does_not() {
    let s = start(json!({}), "").await;
    let paths = [
        format!("/internal/jobs/{VALID_UNKNOWN_ID}"),
        format!("/internal/jobs/{VALID_UNKNOWN_ID}/results.tsv"),
    ];
    let auths: [Option<String>; 5] = [
        None,
        Some("Bearer wrong".into()),
        Some(format!("Bearer {TOKEN}x")),
        Some(format!("Basic {TOKEN}")),
        Some(TOKEN.into()),
    ];
    for auth in &auths {
        let mut post = s
            .http
            .post(s.url("/internal/jobs"))
            .json(&request("blastn"));
        if let Some(a) = auth {
            post = post.header("authorization", a);
        }
        let r = post.send().await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "POST with {auth:?}");
        assert_eq!(
            r.json::<Value>().await.unwrap(),
            json!({"error": "unauthorized"})
        );
        for p in &paths {
            let mut get = s.http.get(s.url(p));
            if let Some(a) = auth {
                get = get.header("authorization", a);
            }
            let r = get.send().await.unwrap();
            assert_eq!(
                r.status(),
                StatusCode::UNAUTHORIZED,
                "GET {p} with {auth:?}"
            );
        }
    }
    let (code, body) = s.health("").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database_release"], "FB2023_04");
    assert_eq!(body["queued"], 0);
    assert_eq!(body["running"], 0);
}

#[tokio::test]
async fn unknown_and_malformed_ids_are_404() {
    let s = start(json!({}), "").await;
    for id in [
        VALID_UNKNOWN_ID,
        "short",
        "has.dot123",
        "x%2F..%2F..%2Fetc",
        &"a".repeat(65),
    ] {
        for suffix in ["", "/results.tsv"] {
            let r = s.get(&format!("/internal/jobs/{id}{suffix}")).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{id}{suffix}");
            assert_eq!(
                r.json::<Value>().await.unwrap(),
                json!({"error": "unknown_job"}),
                "{id}{suffix}"
            );
        }
    }
}

#[tokio::test]
async fn results_before_completion_are_409() {
    let s = start(json!({"concurrency": 1}), "FAKE_BLAST_SLEEP=3\n").await;
    let a = s.submit(&request("blastn")).await;
    let b = s.submit(&request("blastn")).await;
    let (aid, bid) = (a["job_id"].as_str().unwrap(), b["job_id"].as_str().unwrap());
    s.wait_for(aid, Duration::from_secs(5), |st| st == "running")
        .await;
    for (id, state) in [(aid, "running"), (bid, "queued")] {
        let r = s.get(&format!("/internal/jobs/{id}/results.tsv")).await;
        assert_eq!(r.status(), StatusCode::CONFLICT);
        assert_eq!(
            r.json::<Value>().await.unwrap(),
            json!({"error": "not_complete", "state": state})
        );
    }
}

#[tokio::test]
async fn failed_job_reports_error_and_409() {
    let s = start(json!({}), "FAKE_BLAST_FAIL=db\n").await;
    let a = s.submit(&request("blastn")).await;
    let id = a["job_id"].as_str().unwrap();
    let st = s.wait_terminal(id).await;
    assert_eq!(
        st,
        json!({"job_id": id, "state": "failed", "error": "database unavailable"})
    );
    let r = s.get(&format!("/internal/jobs/{id}/results.tsv")).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"error": "not_complete", "state": "failed"})
    );
}

#[tokio::test]
async fn full_queue_is_429_and_fails_health_but_not_liveness() {
    let s = start(
        json!({"concurrency": 1, "max_queued": 2}),
        "FAKE_BLAST_SLEEP=5\n",
    )
    .await;
    let first = s.submit(&request("blastn")).await;
    s.wait_for(
        first["job_id"].as_str().unwrap(),
        Duration::from_secs(5),
        |st| st == "running",
    )
    .await;
    let q1 = s.submit(&request("blastn")).await;
    assert_eq!(q1["queue_position"], 1);
    let (code, _) = s.health("").await;
    assert_eq!(code, StatusCode::OK, "one free queue slot left");
    let q2 = s.submit(&request("blastn")).await;
    assert_eq!(q2["queue_position"], 2);
    assert_eq!(q2["poll_after_seconds"], 10);

    let r = s.post(&request("blastn")).await;
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.headers()["retry-after"], "30");
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"error": "queue_full", "retry_after_seconds": 30})
    );

    let (code, body) = s.health("").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "degraded");
    assert_eq!(body["reason"], "the queue is full");
    let (code, body) = s.health("?liveness=1").await;
    assert_eq!(code, StatusCode::OK, "liveness ignores the queue: {body}");
    assert_eq!(body["queued"], 2);
    assert_eq!(body["running"], 1);
}

#[tokio::test]
async fn health_is_503_when_the_blast_binaries_are_missing() {
    let s = start(json!({"blast_bin_dir": "/nonexistent/blast/bin"}), "").await;
    for q in ["", "?liveness=1"] {
        let (code, body) = s.health(q).await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{q}");
        assert_eq!(body["status"], "degraded");
        assert_eq!(
            body["reason"],
            "BLAST binaries unavailable: blastn is missing"
        );
    }
}

#[tokio::test]
async fn health_is_503_when_a_worker_has_stopped() {
    let dead = tokio::spawn(async {});
    while !dead.is_finished() {
        tokio::task::yield_now().await;
    }
    let s = start_with(json!({}), "", Some(vec![dead])).await;
    let (code, body) = s.health("?liveness=1").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["reason"], "a job worker has stopped");
}

#[tokio::test]
async fn malformed_bodies_are_invalid_request() {
    let s = start(json!({}), "").await;
    let send = |body: &'static str, ct: Option<&'static str>| {
        let mut rb = s
            .http
            .post(s.url("/internal/jobs"))
            .bearer_auth(TOKEN)
            .body(body);
        if let Some(ct) = ct {
            rb = rb.header("content-type", ct);
        }
        rb.send()
    };
    for (body, ct) in [
        ("{not json", Some("application/json")),
        ("[1, 2]", Some("application/json")),
        (r#"{"program": "blastn"}"#, Some("application/json")),
        (
            r#"{"program": 5, "database": "assembly", "sequence": "ACGT"}"#,
            Some("application/json"),
        ),
        (
            r#"{"program": "blastn", "database": "assembly", "sequence": "ACGT"}"#,
            None,
        ),
    ] {
        let r = send(body, ct).await.unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{body} {ct:?}");
        let v: Value = r.json().await.unwrap();
        assert_eq!(v["error"], "invalid_request", "{body}");
        assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()), "{v}");
        assert_eq!(
            v.as_object().unwrap().len(),
            2,
            "exactly error and message: {v}"
        );
    }
}

#[tokio::test]
async fn unknown_extra_key_is_accepted() {
    let s = start(json!({}), "").await;
    let mut body = request("blastn");
    body["future_key"] = json!({"nested": [1, 2, 3]});
    let a = s.submit(&body).await;
    let st = s.wait_terminal(a["job_id"].as_str().unwrap()).await;
    assert_eq!(st["state"], "complete");
}

#[tokio::test]
async fn validation_rejections_carry_the_mcp_codes() {
    let s = start(json!({}), "").await;
    let mut cases: Vec<(Value, &str)> = Vec::new();
    let mut b = request("blastn");
    b["program"] = json!("psiblast");
    cases.push((b, "invalid_program"));
    let mut b = request("blastn");
    b["database"] = json!("nosuchdb");
    cases.push((b, "invalid_database"));
    let mut b = request("blastn");
    b["database"] = json!("translation");
    cases.push((b, "invalid_database"));
    let mut b = request("blastn");
    b["species"] = json!(["dsim"]);
    b["database"] = json!("transcript");
    cases.push((b, "invalid_species"));
    let mut b = request("blastn");
    b["sequence"] = json!(">query\nMKLVQWEFF\n");
    cases.push((b, "invalid_sequence"));
    let mut b = request("blastn");
    b["evalue"] = json!(0);
    cases.push((b, "invalid_option"));
    for (body, code) in cases {
        let r = s.post(&body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{body}");
        let v: Value = r.json().await.unwrap();
        assert_eq!(v["error"], code, "{body} -> {v}");
        assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()));
        assert_eq!(v.as_object().unwrap().len(), 2, "{v}");
    }
}

#[tokio::test]
async fn every_deny_list_entry_is_invalid_option() {
    let s = start(json!({}), "").await;
    assert!(!DENY_LIST.is_empty());
    for (program, adv) in DENY_LIST {
        let mut body = request(program.as_str());
        body["advanced"] = json!(adv);
        let r = s.post(&body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{program} {adv}");
        let v: Value = r.json().await.unwrap();
        assert_eq!(v["error"], "invalid_option", "{program} {adv} -> {v}");
    }
    let (_, h) = s.health("").await;
    assert_eq!(h["queued"], 0, "no rejected request was queued");
}

#[tokio::test]
async fn big_output_is_capped_and_flagged_by_header() {
    // Default cap: 16 MiB, the MCP's Content-Length limit. The fake writes 20 MiB of rows.
    let s = start(json!({}), "FAKE_BLAST_BIG=1\n").await;
    let a = s.submit(&request("blastn")).await;
    let id = a["job_id"].as_str().unwrap();
    let st = s.wait_terminal(id).await;
    assert_eq!(st["state"], "complete");
    assert!(
        st.get("truncated").is_none(),
        "truncation is never in the JSON"
    );
    let r = s.get(&format!("/internal/jobs/{id}/results.tsv")).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        r.headers()
            .get("X-Blast-Jobs-Truncated")
            .expect("X-Blast-Jobs-Truncated header present")
            .to_str()
            .unwrap(),
        "true"
    );
    let len: u64 = r.headers()["content-length"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(len <= 16_777_216, "{len}");
    let body = r.bytes().await.unwrap();
    assert_eq!(body.len() as u64, len);
    assert!(body.starts_with(HEADER.as_bytes()));
    assert!(body.ends_with(b"\n"));
    assert!(
        len > 16_777_216 - 4096,
        "cut near the cap, at a row boundary"
    );

    // A normal job on the same server carries no such header.
    std::fs::write(s.tmp.path().join("state/fake-blast.env"), "").unwrap();
    let b = s.submit(&request("blastn")).await;
    let bid = b["job_id"].as_str().unwrap();
    s.wait_terminal(bid).await;
    let r = s.get(&format!("/internal/jobs/{bid}/results.tsv")).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers().get("X-Blast-Jobs-Truncated").is_none());
}

#[tokio::test]
async fn body_over_the_limit_is_413_invalid_request() {
    let s = start(json!({}), "").await;
    let mut body = request("blastn");
    body["sequence"] = json!(format!(">query\n{}\n", "A".repeat(MAX_BODY_BYTES)));
    let r = s.post(&body).await;
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["error"], "invalid_request");

    // Just under the limit the body is read and validated (here: over the residue cap).
    let mut body = request("blastn");
    body["sequence"] = json!(format!(">query\n{}\n", "A".repeat(200_000)));
    let r = s.post(&body).await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"],
        "invalid_sequence"
    );
}

#[tokio::test]
async fn unknown_route_is_json_404() {
    let s = start(json!({}), "").await;
    let r = s.get("/internal/nothing").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"error": "not_found"})
    );
}
