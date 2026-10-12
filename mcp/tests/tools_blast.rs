//! The three BLAST tools against a wiremock stand-in for the private job API. Every test here starts its
//! own server with its own mock, so the default `base()` server (unconfigured) stays BLAST-free.
mod support;
use serde_json::json;
use support::server::{call, call_raw, client, client_at, start};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TSV: &str = "qseqid\tsseqid\tpident\tlength\tmismatch\tgapopen\tqstart\tqend\tsstart\tsend\tevalue\tbitscore\tstitle\n\
query\tFBtr0070507\t100.000\t60\t0\t0\t1\t60\t101\t160\t1e-30\t111\tFBtr0070507 type=mRNA; loc=X:complement(3134870..3172221); name=N-RA\n\
query\tFBtr0070507\t95.000\t40\t2\t0\t5\t44\t900\t939\t1e-10\t60.2\tFBtr0070507 type=mRNA; loc=X:complement(3134870..3172221); name=N-RA\n\
query\tFBtr0300399\t90.000\t50\t5\t0\t1\t50\t10\t59\t1e-12\t65.8\tFBtr0300399 type=mRNA; name=N-RB\n";

/// A BLAST-configured server in front of a fresh mock service.
async fn blast_server() -> (MockServer, String) {
    let mock = MockServer::start().await;
    let mut cfg = support::test_config();
    cfg.blast_base_url = Some(mock.uri().parse().unwrap());
    cfg.blast_token = Some("test-token".into());
    let base = start(cfg);
    (mock, base)
}

fn submit_mock() -> Mock {
    Mock::given(method("POST"))
        .and(path("/internal/jobs"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_partial_json(json!({
            "program": "blastn", "database": "assembly", "species": ["dmel"], "client": "flybase-mcp"
        })))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "job_id": "job-0001", "state": "queued", "queue_position": 1,
            "poll_after_seconds": 5, "database_release": "FB2023_04 (Dmel r6.53)"
        })))
}

#[tokio::test]
async fn submit_status_results_round_trip() {
    let (mock, base) = blast_server().await;
    submit_mock().expect(1).mount(&mock).await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job_id": "job-0001", "state": "complete", "queue_position": 0, "error": null,
            "result_url": "https://flybase.org/blast/results/job-0001",
            "downloads": {
                "tsv": "https://flybase.org/blast/results/job-0001.tsv",
                "xml": "https://flybase.org/blast/results/job-0001.xml",
                "txt": "https://flybase.org/blast/results/job-0001.txt"
            }
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001/results.tsv"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(TSV)
                .insert_header("content-type", "text/tab-separated-values"),
        )
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let s = call(
        &c,
        "blast_submit",
        json!({"program": "blastn", "database": "assembly",
            "sequence": ">q\nACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT\n"}),
    )
    .await;
    assert_eq!(s["job_id"], "job-0001");
    let token = s["job_token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(s["state"], "queued");
    assert_eq!(s["poll_after_seconds"], 5);
    assert!(s["note"].as_str().unwrap().contains("FB2023_04"));
    let st = call(
        &c,
        "blast_status",
        json!({"job_id": "job-0001", "job_token": token}),
    )
    .await;
    assert_eq!(st["state"], "complete");
    assert_eq!(
        st["downloads"]["tsv"],
        "https://flybase.org/blast/results/job-0001.tsv"
    );
    assert_eq!(
        st["links"]["result_page"],
        "https://flybase.org/blast/results/job-0001"
    );
    let r = call(
        &c,
        "blast_results",
        json!({"job_id": "job-0001", "job_token": token, "max_hits": 1, "max_hsps_per_hit": 1}),
    )
    .await;
    assert_eq!(r["total_hits"], 2);
    let hits = r["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["subject_id"], "FBtr0070507");
    assert_eq!(hits[0]["subject_accession"], "FBtr0070507");
    assert_eq!(hits[0]["flybase_id"], "FBtr0070507");
    assert_eq!(hits[0]["hsp_count"], 2);
    assert_eq!(hits[0]["hsps"].as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["evalue"], 1e-30);
    assert_eq!(hits[0]["bitscore"], 111.0);
    assert_eq!(hits[0]["hsps"][0]["subject_start"], 101);
    assert_eq!(hits[0]["link"], "https://flybase.org/reports/FBtr0070507");
    assert_eq!(
        r["downloads"]["xml"],
        "https://flybase.org/blast/results/job-0001.xml"
    );
    let all = call(
        &c,
        "blast_results",
        json!({"job_id": "job-0001", "job_token": token}),
    )
    .await;
    assert_eq!(all["hits"].as_array().unwrap().len(), 2);
    assert_eq!(all["hits"][1]["flybase_id"], "FBtr0300399");
    mock.verify().await;
}

/// Review focus 4.
#[tokio::test]
async fn wrong_token_never_reaches_backend() {
    let (mock, base) = blast_server().await;
    submit_mock().mount(&mock).await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"job_id": "job-0001", "state": "running"})),
        )
        .expect(0)
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001/results.tsv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TSV))
        .expect(0)
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    call(
        &c,
        "blast_submit",
        json!({"program": "blastn", "database": "assembly", "sequence": "ACGTACGTACGTACGTACGT"}),
    )
    .await;
    let bad = "0".repeat(64);
    for tool in ["blast_status", "blast_results"] {
        let r = call_raw(&c, tool, json!({"job_id": "job-0001", "job_token": bad}))
            .await
            .unwrap();
        let e = r.structured_content.unwrap();
        assert_eq!(e["error"], "not_found", "{tool}");
        assert!(e["message"].as_str().unwrap().contains("token"));
    }
    let r = call_raw(
        &c,
        "blast_status",
        json!({"job_id": "never-issued", "job_token": bad}),
    )
    .await
    .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
    mock.verify().await;
}

#[tokio::test]
async fn unconfigured_server_reports_backend_unavailable() {
    let c = client().await;
    for (tool, args) in [
        (
            "blast_submit",
            json!({"program": "blastn", "database": "assembly", "sequence": "ACGT"}),
        ),
        ("blast_status", json!({"job_id": "x", "job_token": "y"})),
        ("blast_results", json!({"job_id": "x", "job_token": "y"})),
    ] {
        let r = call_raw(&c, tool, args).await.unwrap();
        let e = r.structured_content.unwrap();
        assert_eq!(e["error"], "backend_unavailable", "{tool}");
        assert!(e["message"].as_str().unwrap().contains("not configured"));
    }
    let o = call(&c, "blast_submit", json!({"list_options": true})).await;
    assert_eq!(o["configured"], false);
    assert!(
        o["databases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["name"] == "translation" && d["protein"] == true)
    );
    assert!(o["species"].as_array().unwrap().iter().any(|s| s == "Dmel"));
    assert_eq!(o["programs"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn submit_validation_happens_before_the_service() {
    let (mock, base) = blast_server().await;
    submit_mock().expect(0).mount(&mock).await;
    let c = client_at(&base).await;
    let cases = [
        (
            json!({"program": "blastp", "database": "assembly", "sequence": "MKV"}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": ""}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": ">only a header"}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": "ACGT", "species": ["Hsap"]}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": "ACGT", "advanced": "-x; rm -rf /"}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": "ACGT", "evalue": 0}),
            "invalid_input",
        ),
        (
            json!({"program": "tblastn", "database": "assembly", "sequence": "MKVL", "megablast": true}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": "ACGT", "matrix": "BLOSUM62"}),
            "invalid_input",
        ),
        (
            json!({"program": "blastn", "database": "assembly", "sequence": "A".repeat(100_001)}),
            "limit_exceeded",
        ),
    ];
    for (args, code) in cases {
        let r = call_raw(&c, "blast_submit", args.clone()).await.unwrap();
        assert_eq!(r.structured_content.unwrap()["error"], code, "{args}");
    }
    mock.verify().await;
}

#[tokio::test]
async fn service_errors_map_to_tool_errors() {
    let (mock, base) = blast_server().await;
    let c = client_at(&base).await;
    let seq = json!({"program": "blastn", "database": "assembly", "sequence": "ACGTACGTAC"});
    Mock::given(method("POST"))
        .and(path("/internal/jobs"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({"error": "invalid_sequence", "message": "sequence has no valid residues"}),
        ))
        .expect(1)
        .mount(&mock)
        .await;
    let e = call_raw(&c, "blast_submit", seq.clone())
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(
        e["message"].as_str().unwrap().contains("invalid sequence"),
        "{e}"
    );
    assert!(e["message"].as_str().unwrap().contains("no valid residues"));
    mock.reset().await;
    Mock::given(method("POST"))
        .and(path("/internal/jobs"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(
                json!({"error": "invalid_option", "message": "unknown option -foo"}),
            ),
        )
        .mount(&mock)
        .await;
    let e = call_raw(&c, "blast_submit", seq.clone())
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert!(
        e["message"].as_str().unwrap().contains("invalid advanced"),
        "{e}"
    );
    mock.reset().await;
    Mock::given(method("POST"))
        .and(path("/internal/jobs"))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_json(json!({"job_id": "bad id", "state": "queued"})),
        )
        .mount(&mock)
        .await;
    let e = call_raw(&c, "blast_submit", seq.clone())
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(
        e["error"], "backend_unavailable",
        "malformed job id from the service is a service fault"
    );
    mock.reset().await;
    Mock::given(method("POST"))
        .and(path("/internal/jobs"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({"error": "queue_full", "retry_after_seconds": 30})),
        )
        .mount(&mock)
        .await;
    let e = call_raw(&c, "blast_submit", seq.clone())
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(e["error"], "backend_unavailable");
    assert!(e["message"].as_str().unwrap().contains("30"));
    mock.reset().await;
    Mock::given(method("POST"))
        .and(path("/internal/jobs"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock)
        .await;
    let e = call_raw(&c, "blast_submit", seq)
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(e["error"], "backend_unavailable");
    assert!(
        !e["message"].as_str().unwrap().contains("127.0.0.1"),
        "no service address leaks"
    );
}

#[tokio::test]
async fn results_before_completion_and_cache_bypass() {
    let (mock, base) = blast_server().await;
    submit_mock().mount(&mock).await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"job_id": "job-0001", "state": "running", "queue_position": 0}),
            ),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001/results.tsv"))
        .respond_with(
            ResponseTemplate::new(409)
                .set_body_json(json!({"error": "not_complete", "state": "running"})),
        )
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    // The Prometheus recorder is process-global, so this counter is shared by every server in the binary.
    // The assertion holds because no other test in this binary calls a chado-backed tool concurrently.
    let metrics = || async {
        reqwest::get(format!("{base}/metrics"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("flybase_mcp_cache_events_total"))
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let before = metrics().await;
    let s = call(
        &c,
        "blast_submit",
        json!({"program": "blastn", "database": "assembly", "sequence": "ACGTACGTACGTACGTACGT"}),
    )
    .await;
    let token = s["job_token"].as_str().unwrap().to_string();
    let st = call(
        &c,
        "blast_status",
        json!({"job_id": "job-0001", "job_token": token}),
    )
    .await;
    assert_eq!(st["state"], "running");
    assert!(st["links"].get("result_page").is_none());
    let e = call_raw(
        &c,
        "blast_results",
        json!({"job_id": "job-0001", "job_token": token}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(e["message"].as_str().unwrap().contains("running"));
    assert_eq!(
        metrics().await,
        before,
        "BLAST tools never touch the response cache"
    );
}

/// A redirect from the service is a fault, never followed: the body at the other end must not become hits.
#[tokio::test]
async fn results_redirect_is_not_followed() {
    let (mock, base) = blast_server().await;
    let elsewhere = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/stolen.tsv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TSV))
        .expect(0)
        .mount(&elsewhere)
        .await;
    submit_mock().mount(&mock).await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"job_id": "job-0001", "state": "complete", "queue_position": 0}),
            ),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001/results.tsv"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "location",
            format!("{}/stolen.tsv", elsewhere.uri()).as_str(),
        ))
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let s = call(
        &c,
        "blast_submit",
        json!({"program": "blastn", "database": "assembly", "sequence": "ACGTACGTACGTACGTACGT"}),
    )
    .await;
    let token = s["job_token"].as_str().unwrap().to_string();
    let e = call_raw(
        &c,
        "blast_results",
        json!({"job_id": "job-0001", "job_token": token}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    assert_eq!(e["error"], "backend_unavailable", "{e}");
    assert!(e.get("hits").is_none());
    elsewhere.verify().await;
}

/// A 200 that is not a TSV table (a proxy login or maintenance page) is a fault, not "no hits".
#[tokio::test]
async fn results_with_html_content_type_is_a_fault() {
    let (mock, base) = blast_server().await;
    submit_mock().mount(&mock).await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"job_id": "job-0001", "state": "complete", "queue_position": 0}),
            ),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/internal/jobs/job-0001/results.tsv"))
        .respond_with(
            // set_body_raw: wiremock derives the content-type from the body setter, so set_body_string
            // plus insert_header would still answer text/plain.
            ResponseTemplate::new(200).set_body_raw(
                "<html><body>Sign in</body></html>",
                "text/html; charset=utf-8",
            ),
        )
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let s = call(
        &c,
        "blast_submit",
        json!({"program": "blastn", "database": "assembly", "sequence": "ACGTACGTACGTACGTACGT"}),
    )
    .await;
    let token = s["job_token"].as_str().unwrap().to_string();
    let e = call_raw(
        &c,
        "blast_results",
        json!({"job_id": "job-0001", "job_token": token}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    assert_eq!(e["error"], "backend_unavailable", "{e}");
    assert!(!e["message"].as_str().unwrap().contains("no hits"));
}

/// More than one FASTA record is rejected instead of being concatenated into one query.
#[tokio::test]
async fn multi_record_fasta_is_rejected() {
    let (mock, base) = blast_server().await;
    submit_mock().expect(0).mount(&mock).await;
    let c = client_at(&base).await;
    let e = call_raw(
        &c,
        "blast_submit",
        json!({"program": "blastn", "database": "assembly", "sequence": ">a\nACGTACGT\n>b\nTTTTGGGG\n"}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    assert_eq!(e["error"], "invalid_input");
    assert!(
        e["message"]
            .as_str()
            .unwrap()
            .contains("one sequence per job"),
        "{e}"
    );
    mock.verify().await;
}

#[tokio::test]
async fn health_reports_blast_reachability() {
    let (mock, base) = blast_server().await;
    let h: serde_json::Value = reqwest::get(format!("{base}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        h["blast"], "unreachable",
        "no /internal/health route mounted yet: {h}"
    );
    assert_eq!(h["status"], "ok", "BLAST reachability is a separate flag");
    Mock::given(method("GET"))
        .and(path("/internal/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let h: serde_json::Value = reqwest::get(format!("{base}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["blast"], "ok");
}
