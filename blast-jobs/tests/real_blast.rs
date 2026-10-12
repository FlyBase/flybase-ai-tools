//! End-to-end tests against real NCBI BLAST+ (design 3.12, Task 5). Skipped unless
//! `BLAST_JOBS_REAL_BLAST` is set. Its value must be the directory holding the BLAST+ 2.17.0 binaries
//! (`blastn blastp blastx tblastn tblastx makeblastdb`). Every binary comes from that directory, never from
//! `PATH`, and each must report `2.17.0+`; a set but wrong value fails the tests instead of skipping them.
//!
//! Each test builds the fixture databases (`fixture/build-db.sh`, the archive's `makeblastdb` flags) in a
//! temp directory, starts the API over them on an ephemeral port, and drives submit, poll and results.
//! Queries are slices of the fixture's own sequences, so every program has a known hit: N-RA
//! (`FBtr0070507`, `fbdb.dmel.transcript.5460`) and N-PA (`FBpp0070483`, `fbdb.dmel.translation.529`).
//!
//!     BLAST_JOBS_REAL_BLAST=/path/to/ncbi-blast-2.17.0+/bin cargo test --test real_blast
use blast_jobs::auth::Auth;
use blast_jobs::catalogue::Catalogue;
use blast_jobs::config::Config;
use blast_jobs::http::{self, AppState};
use blast_jobs::runner::{self, HEADER};
use blast_jobs::store::Store;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

const ENV: &str = "BLAST_JOBS_REAL_BLAST";
const TOKEN: &str = "real-blast-test-token";
const NOTCH_TRANSCRIPT: &str = "fbdb.dmel.transcript.5460";
const NOTCH_PROTEIN: &str = "fbdb.dmel.translation.529";
/// The pinned BLAST+ release (ruling 6); the suite proves behaviour on exactly this version.
const VERSION: &str = "2.17.0+";
const BINARIES: [&str; 6] = [
    "blastn",
    "blastp",
    "blastx",
    "tblastn",
    "tblastx",
    "makeblastdb",
];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The BLAST+ bin directory, or `None` (with a printed reason) when the test should skip.
fn blast_bin() -> Option<PathBuf> {
    let v = match std::env::var(ENV) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            println!("skipped: {ENV} is unset (set it to a BLAST+ bin directory to run)");
            return None;
        }
    };
    let dir = PathBuf::from(v.trim());
    // Set but unusable is a configuration error, not a skip.
    for b in BINARIES {
        let out = Command::new(dir.join(b))
            .arg("-version")
            .output()
            .unwrap_or_else(|e| panic!("{ENV}={v}: cannot run {b} from that directory: {e}"));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && text.contains(VERSION),
            "{ENV}={v}: {b} -version does not report {VERSION}: {text}"
        );
    }
    Some(dir)
}

/// Reads one FASTA record's residues by its id from a fixture file.
fn fixture_seq(file: &str, id: &str) -> String {
    let text = std::fs::read_to_string(manifest().join("fixture").join(file)).unwrap();
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix('>') {
            inside = h.split_whitespace().next() == Some(id);
        } else if inside {
            out.push_str(line.trim());
        }
    }
    assert!(!out.is_empty(), "{id} not in fixture/{file}");
    out
}

fn notch_rna(range: std::ops::Range<usize>) -> String {
    fixture_seq("notch.fa", NOTCH_TRANSCRIPT)[range].to_string()
}

fn notch_protein(range: std::ops::Range<usize>) -> String {
    fixture_seq("notch-protein.fa", NOTCH_PROTEIN)[range].to_string()
}

struct Server {
    _tmp: tempfile::TempDir,
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

/// One parsed `results.tsv` row.
#[derive(Debug)]
struct Row {
    sseqid: String,
    pident: f64,
    length: u64,
    bitscore: f64,
    stitle: String,
}

impl Server {
    async fn start(bin: &Path) -> Server {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("db");
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let fixture = manifest().join("fixture");
        let out = Command::new("sh")
            .arg(fixture.join("build-db.sh"))
            .args([bin, &fixture, &db])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "build-db.sh failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let cfg = json!({
            "bind": "127.0.0.1:0",
            "blast_bin_dir": bin,
            "database_root": db,
            "catalogue": db.join("catalogue.json"),
            "database_release": "FB2023_04",
            "state_root": state,
            "concurrency": 2,
            "threads_per_job": 1,
            "wall_seconds": 60,
            "translated_wall_seconds": 60,
        });
        let path = tmp.path().join("config.json");
        std::fs::write(&path, cfg.to_string()).unwrap();
        let cfg = Arc::new(Config::load(&path).unwrap());
        let cat = Arc::new(Catalogue::load(&cfg.catalogue, &cfg.database_root).unwrap());
        http::probe_binaries(&cfg.blast_bin_dir).unwrap();
        let store = Store::open(&cfg.state_root, cfg.max_queued).unwrap();
        let workers = runner::spawn_workers(cfg.clone(), store.clone());
        let app = AppState::new(cfg, cat, store, Auth::Token(TOKEN.into()), workers);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, http::router(app)).await.unwrap();
        });
        Server {
            _tmp: tmp,
            base: format!("http://{addr}"),
            http: reqwest::Client::new(),
            tasks: vec![task],
        }
    }

    /// Submits, polls to completion, and returns the parsed rows. Asserts the `202`, the final
    /// `complete` state, the TSV content type and the header line.
    async fn search(&self, req: Value) -> Vec<Row> {
        let r = self
            .http
            .post(format!("{}/internal/jobs", self.base))
            .bearer_auth(TOKEN)
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::ACCEPTED, "{:?}", r.text().await);
        let a: Value = r.json().await.unwrap();
        assert_eq!(a["database_release"], "FB2023_04");
        let id = a["job_id"].as_str().unwrap().to_string();

        let end = Instant::now() + Duration::from_secs(120);
        let status = loop {
            let s: Value = self
                .http
                .get(format!("{}/internal/jobs/{id}", self.base))
                .bearer_auth(TOKEN)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if matches!(s["state"].as_str(), Some("complete" | "failed")) {
                break s;
            }
            assert!(Instant::now() < end, "job {id} still {s}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(status["state"], "complete", "{req} -> {status}");

        let r = self
            .http
            .get(format!("{}/internal/jobs/{id}/results.tsv", self.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let ct = r.headers()["content-type"].to_str().unwrap().to_string();
        assert!(ct.starts_with("text/tab-separated-values"), "{ct}");
        assert!(r.headers().get("x-blast-jobs-truncated").is_none());
        let body = r.text().await.unwrap();
        let (header, rows) = body.split_at(HEADER.len());
        assert_eq!(
            header, HEADER,
            "results.tsv must start with the header line"
        );
        assert_eq!(
            HEADER,
            "qseqid\tsseqid\tpident\tlength\tmismatch\tgapopen\tqstart\tqend\tsstart\tsend\tevalue\tbitscore\tstitle\n"
        );
        rows.lines()
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                assert_eq!(f.len(), 13, "row {l:?}");
                assert_eq!(f[0], "query");
                Row {
                    sseqid: f[1].to_string(),
                    pident: f[2].parse().unwrap(),
                    length: f[3].parse().unwrap(),
                    bitscore: f[11].parse().unwrap(),
                    stitle: f[12].to_string(),
                }
            })
            .collect()
    }
}

fn req(program: &str, database: &str, residues: &str) -> Value {
    json!({
        "program": program,
        "database": database,
        "species": ["dmel"],
        "sequence": format!(">query\n{residues}\n"),
        "client": "flybase-mcp",
    })
}

/// Asserts the best row is the expected subject, carrying the FlyBase id in its title.
fn assert_top_hit(rows: &[Row], sseqid: &str, flybase_id: &str) {
    let top = rows
        .first()
        .unwrap_or_else(|| panic!("no hits; expected {sseqid}"));
    assert_eq!(top.sseqid, sseqid, "{rows:?}");
    assert!(
        top.stitle.contains(&format!("ID={flybase_id};")),
        "stitle {:?} lacks {flybase_id}",
        top.stitle
    );
}

macro_rules! real {
    () => {
        match blast_bin() {
            Some(b) => Server::start(&b).await,
            None => return,
        }
    };
}

#[tokio::test]
async fn blastn_finds_notch_transcript() {
    let s = real!();
    let rows = s
        .search(req("blastn", "transcript", &notch_rna(2000..2400)))
        .await;
    assert_top_hit(&rows, NOTCH_TRANSCRIPT, "FBtr0070507");
    assert_eq!(rows[0].length, 400);
    assert_eq!(rows[0].pident, 100.0);
}

#[tokio::test]
async fn blastn_short_task_via_advanced() {
    let s = real!();
    let q = notch_rna(3000..3022);
    let mut short = req("blastn", "transcript", &q);
    short["advanced"] = json!("-task blastn-short");
    let short = s.search(short).await;
    let plain = s.search(req("blastn", "transcript", &q)).await;
    assert_top_hit(&short, NOTCH_TRANSCRIPT, "FBtr0070507");
    assert_top_hit(&plain, NOTCH_TRANSCRIPT, "FBtr0070507");
    assert_eq!(short[0].length, 22);
    // blastn-short scores a match +1/-3 with word size 7; blastn +2/-3 with word size 11. The same
    // 22-residue exact match therefore scores differently, which shows the task reached BLAST.
    assert!(
        short[0].bitscore > plain[0].bitscore,
        "blastn-short {} vs blastn {}",
        short[0].bitscore,
        plain[0].bitscore
    );
    assert!(
        short.len() > plain.len(),
        "word size 7 finds more short HSPs"
    );
}

#[tokio::test]
async fn blastp_finds_notch_protein() {
    let s = real!();
    let rows = s
        .search(req("blastp", "translation", &notch_protein(100..300)))
        .await;
    assert_top_hit(&rows, NOTCH_PROTEIN, "FBpp0070483");
    assert_eq!(rows[0].length, 200);
    assert_eq!(rows[0].pident, 100.0);
}

#[tokio::test]
async fn tblastn_finds_notch_transcript() {
    let s = real!();
    let rows = s
        .search(req("tblastn", "transcript", &notch_protein(100..300)))
        .await;
    assert_top_hit(&rows, NOTCH_TRANSCRIPT, "FBtr0070507");
    assert_eq!(rows[0].pident, 100.0);
}

#[tokio::test]
async fn blastx_finds_notch_protein() {
    let s = real!();
    // Inside the N-RA coding sequence (the CDS starts at transcript position 799).
    let rows = s
        .search(req("blastx", "translation", &notch_rna(1500..2100)))
        .await;
    assert_top_hit(&rows, NOTCH_PROTEIN, "FBpp0070483");
    assert_eq!(rows[0].length, 200);
    assert_eq!(rows[0].pident, 100.0);
}

#[tokio::test]
async fn tblastx_finds_notch_transcript() {
    let s = real!();
    let rows = s
        .search(req("tblastx", "transcript", &notch_rna(4000..4300)))
        .await;
    assert_top_hit(&rows, NOTCH_TRANSCRIPT, "FBtr0070507");
}

/// The Notch opa repeat (CAG/CAA) is low complexity: the default `-dust yes` masks it, so nothing
/// is found, and `-dust no` finds it in full.
#[tokio::test]
async fn blastn_masks_low_complexity_by_default() {
    let s = real!();
    let q = notch_rna(8400..8510);
    let masked = s.search(req("blastn", "transcript", &q)).await;
    assert!(
        masked.is_empty(),
        "dust should mask the opa repeat: {masked:?}"
    );
    let mut open = req("blastn", "transcript", &q);
    open["advanced"] = json!("-dust no");
    let open = s.search(open).await;
    assert_top_hit(&open, NOTCH_TRANSCRIPT, "FBtr0070507");
    assert_eq!(open[0].length, 110);
}

/// The N-PA polyglutamine stretch: the default `-seg yes` leaves only the unmasked tail to align,
/// and `-seg no` aligns the whole 40-residue query.
#[tokio::test]
async fn blastp_masks_low_complexity_by_default() {
    let s = real!();
    let q = notch_protein(2540..2580);
    let masked = s.search(req("blastp", "translation", &q)).await;
    let longest = masked.iter().map(|r| r.length).max().unwrap_or(0);
    assert!(longest < 40, "seg should mask the polyQ run: {masked:?}");
    let mut open = req("blastp", "translation", &q);
    open["advanced"] = json!("-seg no");
    let open = s.search(open).await;
    assert_top_hit(&open, NOTCH_PROTEIN, "FBpp0070483");
    assert_eq!(open[0].length, 40);
}
