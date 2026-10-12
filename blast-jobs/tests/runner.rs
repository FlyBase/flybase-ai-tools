//! Runner tests against the fake BLAST+ scripts in `tests/fake-blast/` (design 3.12). Each test gets
//! its own state root; the fake reads its controls from `<state_root>/fake-blast.env` because the
//! runner gives BLAST+ a fixed environment.
use blast_jobs::catalogue::Catalogue;
use blast_jobs::command;
use blast_jobs::config::Config;
use blast_jobs::jobid::JobId;
use blast_jobs::runner::{self, HEADER, OOM_MESSAGE, STDERR_CAP};
use blast_jobs::store::Store;
use blast_jobs::types::{JobStatus, Program, State, SubmitRequest};
use blast_jobs::validate;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

const PROGRAMS: [&str; 5] = ["blastn", "blastp", "blastx", "tblastn", "tblastx"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Env {
    _tmp: tempfile::TempDir,
    cfg: Arc<Config>,
    cat: Catalogue,
    store: Arc<Store>,
    workers: Vec<JoinHandle<()>>,
}

impl Drop for Env {
    fn drop(&mut self) {
        for w in &self.workers {
            w.abort();
        }
    }
}

/// A store and running workers over the fixture catalogue and the fake BLAST binaries. `overrides`
/// are merged into the config; `controls` is written to `fake-blast.env`.
fn setup(overrides: Value, controls: &str) -> Env {
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
    let cat = Catalogue::load(&cfg.catalogue, &cfg.database_root).unwrap();
    let store = Store::open(&cfg.state_root, cfg.max_queued).unwrap();
    let workers = runner::spawn_workers(cfg.clone(), store.clone());
    assert_eq!(workers.len(), cfg.concurrency);
    Env {
        _tmp: tmp,
        cfg,
        cat,
        store,
        workers,
    }
}

fn request(program: &str) -> SubmitRequest {
    let p = Program::parse(program).unwrap();
    SubmitRequest {
        program: program.into(),
        database: if p.protein_database() {
            "translation"
        } else {
            "assembly"
        }
        .into(),
        species: vec!["dmel".into()],
        sequence: if p.protein_query() {
            ">q\nMKLVQWERTY\n"
        } else {
            ">q\nACGTACGTACGT\n"
        }
        .into(),
        ..Default::default()
    }
}

impl Env {
    async fn submit(&self, req: SubmitRequest) -> JobId {
        let v = validate::check(&req, &self.cfg, &self.cat).unwrap();
        self.store.create(v).await.unwrap().0
    }

    fn status(&self, id: &JobId) -> JobStatus {
        self.store.get(id).unwrap()
    }

    async fn wait_terminal(&self, id: &JobId, within: Duration) -> JobStatus {
        let end = Instant::now() + within;
        loop {
            let s = self.status(id);
            if matches!(s.state, State::Complete | State::Failed) {
                return s;
            }
            assert!(
                Instant::now() < end,
                "job still {:?} after {within:?}",
                s.state
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn dir(&self, id: &JobId) -> PathBuf {
        self.store.job_dir(id)
    }
}

fn canned() -> Vec<u8> {
    std::fs::read(manifest().join("tests/fake-blast/canned.tsv")).unwrap()
}

#[test]
fn fake_blast_scripts_are_executable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = manifest().join("tests/fake-blast");
    for p in PROGRAMS {
        let path = dir.join(p);
        let mode = std::fs::metadata(&path)
            .unwrap_or_else(|e| panic!("{} is missing: {e}", path.display()))
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o111,
            0o111,
            "{} must be executable (git update-index --chmod=+x); mode is {mode:o}",
            path.display()
        );
    }
    // In a git checkout, the committed mode must be 100755 too, or a fresh clone loses the bit.
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["ls-files", "-s", "--"])
        .args(PROGRAMS)
        .output();
    match out {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => {
            let listing = String::from_utf8_lossy(&o.stdout);
            for line in listing.lines() {
                assert!(
                    line.starts_with("100755 "),
                    "committed without the executable bit: {line}"
                );
            }
        }
        _ => eprintln!("fake-blast git mode check skipped: not in a git checkout with them staged"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn success_writes_header_then_rows_and_argv_matches_the_builder() {
    let env = setup(json!({}), "");
    for program in PROGRAMS {
        let mut req = request(program);
        if program == "blastn" {
            // Two species: the `-db` value must arrive as one argv element with a space.
            req.species = vec!["dmel".into(), "dsim".into()];
            req.advanced = Some("-dust no -lcase_masking -penalty -3".into());
        }
        let validated = validate::check(&req, &env.cfg, &env.cat).unwrap();
        let id = env.submit(req).await;
        let s = env.wait_terminal(&id, Duration::from_secs(10)).await;
        assert_eq!(s.state, State::Complete, "{program}: {:?}", s.error);
        assert!(!s.truncated);
        let dir = env.dir(&id);
        let body = std::fs::read(dir.join("results.tsv")).unwrap();
        assert_eq!(body, [HEADER.as_bytes(), &canned()].concat(), "{program}");
        assert!(!dir.join("raw.tsv").exists(), "{program}: raw.tsv deleted");
        assert!(dir.join("stderr.txt").exists());

        let want: Vec<String> = command::build(&env.cfg, &validated, &validated.resolved, &dir)
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect();
        let got: Vec<String> = std::fs::read_to_string(dir.join("argv.txt"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(got, want, "{program}: argv as the fake BLAST received it");
        assert!(got[0].ends_with(&format!("/{program}")));
        let query = &got[got.iter().position(|a| a == "-query").unwrap() + 1];
        assert!(
            Path::new(query).is_file(),
            "{program}: -query names the file the store wrote"
        );
        if program == "blastn" {
            let db = &got[got.iter().position(|a| a == "-db").unwrap() + 1];
            assert_eq!(db.split(' ').count(), 2, "one -db element, two prefixes");
            assert!(got.windows(2).any(|w| w == ["-dust", "no"]));
            assert!(got.windows(2).any(|w| w == ["-penalty", "-3"]));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn database_error_becomes_a_fixed_message() {
    let env = setup(json!({}), "FAKE_BLAST_FAIL=db\n");
    let id = env.submit(request("blastn")).await;
    let s = env.wait_terminal(&id, Duration::from_secs(10)).await;
    assert_eq!(s.state, State::Failed);
    assert_eq!(s.error.as_deref(), Some("database unavailable"));
    let dir = env.dir(&id);
    assert!(!dir.join("results.tsv").exists());
    assert!(!dir.join("raw.tsv").exists());
    let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap();
    assert!(
        stderr.contains("BLAST Database error"),
        "stderr kept server-side"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn other_failure_names_the_exit_and_strips_paths() {
    let env = setup(json!({}), "FAKE_BLAST_FAIL=other\n");
    let id = env.submit(request("blastp")).await;
    let s = env.wait_terminal(&id, Duration::from_secs(10)).await;
    assert_eq!(s.state, State::Failed);
    assert_eq!(
        s.error.as_deref(),
        Some("BLAST failed (exit 3): BLAST query/options error: fake failure for <path>")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sigkill_not_from_us_is_out_of_memory() {
    let env = setup(json!({}), "FAKE_BLAST_FAIL=oom\n");
    let id = env.submit(request("tblastn")).await;
    let s = env.wait_terminal(&id, Duration::from_secs(10)).await;
    assert_eq!(s.state, State::Failed);
    assert_eq!(s.error.as_deref(), Some(OOM_MESSAGE));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stderr_is_capped_and_the_message_bounded() {
    let env = setup(json!({}), "FAKE_BLAST_FAIL=noisy\n");
    let id = env.submit(request("blastx")).await;
    let s = env.wait_terminal(&id, Duration::from_secs(10)).await;
    assert_eq!(s.state, State::Failed);
    let msg = s.error.unwrap();
    assert!(
        msg.starts_with("BLAST failed (exit 4): Error: noisy failure"),
        "{msg}"
    );
    assert!(!msg.contains("/some"), "paths never reach the user: {msg}");
    assert!(msg.len() < 300, "{} bytes", msg.len());
    let kept = std::fs::metadata(env.dir(&id).join("stderr.txt"))
        .unwrap()
        .len();
    assert_eq!(
        kept, STDERR_CAP as u64,
        "first 64 KiB kept, the rest drained"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wall_limit_kills_and_fails_the_job() {
    let env = setup(json!({"wall_seconds": 1}), "FAKE_BLAST_SLEEP=5\n");
    let started = Instant::now();
    let id = env.submit(request("blastn")).await;
    let s = env.wait_terminal(&id, Duration::from_secs(10)).await;
    let took = started.elapsed();
    assert_eq!(s.state, State::Failed);
    assert_eq!(
        s.error.as_deref(),
        Some("search exceeded 1 s; try a shorter query or a smaller database")
    );
    assert!(
        took < Duration::from_secs(4),
        "killed at the limit, took {took:?}"
    );
    assert!(!env.dir(&id).join("results.tsv").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn translated_programs_use_the_translated_wall_limit() {
    let env = setup(
        json!({"wall_seconds": 30, "translated_wall_seconds": 1}),
        "FAKE_BLAST_SLEEP=3\n",
    );
    let tx = env.submit(request("tblastx")).await;
    let n = env.submit(request("blastn")).await;
    let s = env.wait_terminal(&tx, Duration::from_secs(10)).await;
    assert_eq!(s.state, State::Failed);
    assert_eq!(
        s.error.as_deref(),
        Some("search exceeded 1 s; try a shorter query or a smaller database")
    );
    let s = env.wait_terminal(&n, Duration::from_secs(10)).await;
    assert_eq!(
        s.state,
        State::Complete,
        "blastn runs under wall_seconds: {:?}",
        s.error
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn big_output_is_cut_at_a_row_boundary_within_the_cap() {
    const CAP: u64 = 1024 * 1024;
    let env = setup(json!({"max_results_bytes": CAP}), "FAKE_BLAST_BIG=1\n");
    let id = env.submit(request("blastn")).await;
    let s = env.wait_terminal(&id, Duration::from_secs(30)).await;
    assert_eq!(s.state, State::Complete, "{:?}", s.error);
    assert!(s.truncated, "the store records the cut");
    let dir = env.dir(&id);
    let body = std::fs::read(dir.join("results.tsv")).unwrap();
    assert!(body.len() as u64 <= CAP, "{} bytes", body.len());
    assert!(body.starts_with(HEADER.as_bytes()));
    assert!(body.ends_with(b"\n"));
    let row = canned().split(|&b| b == b'\n').next().unwrap().len() + 1;
    assert!(
        body.len() as u64 + row as u64 > CAP,
        "longest cut: one more row would pass the cap"
    );
    for line in body[HEADER.len()..]
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
    {
        assert_eq!(line.split(|&b| b == b'\t').count(), 13, "whole rows only");
    }
    assert!(!dir.join("raw.tsv").exists(), "raw.tsv deleted");
}

async fn wait_counts(store: &Store, want: (usize, usize), within: Duration) {
    let end = Instant::now() + within;
    while store.counts() != want {
        assert!(
            Instant::now() < end,
            "counts {:?}, expected {want:?}",
            store.counts()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// `concurrency` jobs run at once and the next waits in the queue until a slot frees.
async fn runs_n_at_a_time(n: usize) {
    let env = setup(json!({"concurrency": n}), "FAKE_BLAST_SLEEP=2\n");
    // Let the workers go idle first, so the jobs reach them through the store's wake-up.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut ids = Vec::new();
    for _ in 0..=n {
        ids.push(env.submit(request("blastn")).await);
    }
    wait_counts(&env.store, (1, n), Duration::from_millis(1500)).await;
    // Hold for a while: no extra job starts while the slots are busy.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(env.store.counts(), (1, n));
    let last = &ids[n];
    assert_eq!(env.status(last).state, State::Queued);
    assert_eq!(env.status(last).queue_position, Some(1));
    for id in &ids[..n] {
        assert_eq!(env.status(id).state, State::Running);
        assert_eq!(env.status(id).queue_position, Some(0));
    }
    // The queued job starts once a slot frees, and everything completes.
    wait_counts(&env.store, (0, 1), Duration::from_secs(5)).await;
    for id in &ids {
        let s = env.wait_terminal(id, Duration::from_secs(10)).await;
        assert_eq!(s.state, State::Complete, "{:?}", s.error);
    }
    assert_eq!(env.store.counts(), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_two_runs_two_and_queues_a_third() {
    runs_n_at_a_time(2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_three_runs_three_and_queues_a_fourth() {
    runs_n_at_a_time(3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_one_runs_jobs_in_submission_order() {
    let env = setup(json!({"concurrency": 1}), "");
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(env.submit(request("blastp")).await);
    }
    let mut finished = Vec::new();
    for id in &ids {
        env.wait_terminal(id, Duration::from_secs(10)).await;
        let m = std::fs::metadata(env.dir(id).join("results.tsv"))
            .unwrap()
            .modified()
            .unwrap();
        finished.push(m);
    }
    assert!(finished.windows(2).all(|w| w[0] <= w[1]), "FIFO order");
}
