//! Workers that run queued jobs (design 3.5, 3.6).
//!
//! `concurrency` worker tasks loop on `Store::claim_next`, waiting on the store's `Notify` when the FIFO
//! is empty. Each job runs the argv from `command::build` with `tokio::process::Command`: no shell, a
//! fixed environment, `kill_on_drop`, stderr captured up to 64 KiB into `stderr.txt`. The wall-clock
//! limit (`wall_seconds`, or `translated_wall_seconds` for blastx, tblastn and tblastx) is enforced with
//! `tokio::time::timeout`; on expiry the child gets `start_kill` and is reaped. No process group: BLAST+
//! is one multithreaded process, and the container's `--init` reaps anything else.
//!
//! After a zero exit, `finalize_results` writes `results.tsv` (the header line, then `raw.tsv` cut once
//! at a row boundary so the whole file stays within `max_results_bytes`) and deletes `raw.tsv`.
use crate::command::{self, RAW_FILE};
use crate::config::Config;
use crate::jobid::JobId;
use crate::store::{Outcome, Store};
use crate::validate::Validated;
use std::ffi::OsString;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStderr, Command};
use tokio::task::JoinHandle;

/// The served file's first line: the column names of `-outfmt "6 std stitle"`.
pub const HEADER: &str = "qseqid\tsseqid\tpident\tlength\tmismatch\tgapopen\tqstart\tqend\tsstart\tsend\tevalue\tbitscore\tstitle\n";
pub const RESULTS_FILE: &str = "results.tsv";
pub const STDERR_FILE: &str = "stderr.txt";
/// Most stderr bytes kept per job; the rest is read and discarded so BLAST+ never blocks on the pipe.
pub const STDERR_CAP: usize = 64 * 1024;
/// Longest BLAST+ error line quoted in a user message, in characters.
const DETAIL_MAX: usize = 160;
/// How long to wait for the stderr pipe to close after the child has exited (a grandchild could keep
/// it open); what was captured by then is kept.
const STDERR_GRACE: Duration = Duration::from_secs(2);
/// The environment BLAST+ runs with; nothing is inherited.
const ENV: &[(&str, &str)] = &[
    ("NCBI_DONT_USE_NCBIRC", "1"),
    ("NCBI_DONT_USE_LOCAL_CONFIG", "1"),
    ("BLAST_USAGE_REPORT", "false"),
    ("HOME", "/tmp"),
];

pub const OOM_MESSAGE: &str = "search ran out of memory; try a shorter query or fewer species";

/// The user-facing message for a search killed at the wall-clock limit.
pub fn timeout_message(seconds: u64) -> String {
    format!("search exceeded {seconds} s; try a shorter query or a smaller database")
}

/// Starts `cfg.concurrency` workers. They run until aborted.
pub fn spawn_workers(cfg: Arc<Config>, store: Arc<Store>) -> Vec<JoinHandle<()>> {
    (0..cfg.concurrency)
        .map(|n| tokio::spawn(worker(n, cfg.clone(), store.clone())))
        .collect()
}

async fn worker(n: usize, cfg: Arc<Config>, store: Arc<Store>) {
    loop {
        // Register as a waiter before looking at the FIFO, so a `create` between an empty
        // `claim_next` and the wait still wakes this worker. A notification taken by a worker that
        // then finds a job is forwarded to another waiter when the future is dropped, and the store
        // itself wakes another worker when jobs remain after a claim.
        let notified = store.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        // Never cancelled: a dropped `claim_next` could leave a job running with nobody to finish it.
        let Some((id, req)) = store.claim_next().await else {
            notified.await;
            continue;
        };
        // The job runs in its own task so a panic fails that job, not the worker.
        let task = tokio::spawn(run_job(cfg.clone(), store.clone(), id.clone(), req));
        if let Err(e) = task.await {
            tracing::error!(worker = n, job_id = %id, error = %e, "job task ended abnormally");
            let _ = std::fs::remove_file(store.job_dir(&id).join(RAW_FILE));
            store
                .finish(&id, Outcome::Failed("internal error".into()))
                .await;
        }
    }
}

/// How the child ended.
enum Exit {
    Status(ExitStatus),
    TimedOut,
    /// `wait` itself failed; the child is killed on drop.
    WaitFailed(io::Error),
}

async fn run_job(cfg: Arc<Config>, store: Arc<Store>, id: JobId, req: Validated) {
    let dir = store.job_dir(&id);
    let program = req.program;
    let wall = cfg.wall_seconds_for(program);
    let argv = command::build(&cfg, &req, &req.resolved, &dir);
    tracing::info!(
        job_id = %id, %program, database = %req.database, species = req.species.len(),
        query_length = req.residues.len(), "job started"
    );
    tracing::debug!(job_id = %id, ?argv, "argv");
    let started = Instant::now();

    let outcome = match execute(&argv, &dir, Duration::from_secs(wall)).await {
        Err(e) => {
            tracing::error!(job_id = %id, error = %e, "could not start BLAST");
            Outcome::Failed("BLAST could not be started".into())
        }
        Ok((Exit::TimedOut, _)) => Outcome::Failed(timeout_message(wall)),
        Ok((Exit::WaitFailed(e), _)) => {
            tracing::error!(job_id = %id, error = %e, "waiting for BLAST failed");
            Outcome::Failed("BLAST failed".into())
        }
        Ok((Exit::Status(status), _)) if status.success() => {
            let cap = cfg.max_results_bytes;
            let d = dir.clone();
            match tokio::task::spawn_blocking(move || finalize_results(&d, cap)).await {
                Ok(Ok(truncated)) => Outcome::Complete { truncated },
                Ok(Err(e)) => {
                    tracing::error!(job_id = %id, error = %e, "could not write results.tsv");
                    Outcome::Failed("the results could not be written".into())
                }
                Err(e) => {
                    tracing::error!(job_id = %id, error = %e, "finalize task failed");
                    Outcome::Failed("the results could not be written".into())
                }
            }
        }
        Ok((Exit::Status(status), stderr)) => {
            let first = first_error_line(&stderr);
            tracing::warn!(job_id = %id, ?status, stderr = first.unwrap_or(""), "BLAST failed");
            Outcome::Failed(failure_message(status, first))
        }
    };
    if !matches!(outcome, Outcome::Complete { .. }) {
        let _ = std::fs::remove_file(dir.join(RAW_FILE));
    }
    tracing::info!(
        job_id = %id,
        state = if matches!(outcome, Outcome::Complete { .. }) { "complete" } else { "failed" },
        elapsed_ms = started.elapsed().as_millis() as u64,
        "job finished"
    );
    store.finish(&id, outcome).await;
}

/// Runs `argv` in `dir` under the wall limit; returns how it ended and the captured stderr (also
/// written to `stderr.txt`). `Err` only when the process could not be spawned. The stderr reader is
/// aborted if the pipe is still open `STDERR_GRACE` after the child ended.
async fn execute(argv: &[OsString], dir: &Path, wall: Duration) -> io::Result<(Exit, Vec<u8>)> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .envs(ENV.iter().copied())
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let reader = child
        .stderr
        .take()
        .map(|s| tokio::spawn(collect_stderr(s, captured.clone())));

    let exit = match tokio::time::timeout(wall, child.wait()).await {
        Ok(Ok(status)) => Exit::Status(status),
        Ok(Err(e)) => {
            let _ = child.start_kill();
            Exit::WaitFailed(e)
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            Exit::TimedOut
        }
    };
    if let Some(mut r) = reader
        && tokio::time::timeout(STDERR_GRACE, &mut r).await.is_err()
    {
        r.abort();
    }
    let stderr = std::mem::take(&mut *captured.lock().unwrap_or_else(|p| p.into_inner()));
    if let Err(e) = std::fs::write(dir.join(STDERR_FILE), &stderr) {
        tracing::warn!(error = %e, "could not write stderr.txt");
    }
    Ok((exit, stderr))
}

/// Reads the pipe to its end, keeping the first `STDERR_CAP` bytes.
async fn collect_stderr(mut pipe: ChildStderr, kept: Arc<Mutex<Vec<u8>>>) {
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut k = kept.lock().unwrap_or_else(|p| p.into_inner());
                let room = STDERR_CAP.saturating_sub(k.len());
                k.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
}

/// The stderr line that best explains a failure: the first that starts with `Error` or `BLAST`, else
/// the first containing `error`/`Error`, else the first `Warning`, else the first non-empty line. A
/// benign early warning (BLAST+ warns when `max_target_seqs < 5`) never hides a later error.
fn first_error_line(stderr: &[u8]) -> Option<&str> {
    let text = match std::str::from_utf8(stderr) {
        Ok(t) => t,
        // Cut at the last valid boundary (the 64 KiB cap can split a character).
        Err(e) => std::str::from_utf8(&stderr[..e.valid_up_to()]).unwrap_or(""),
    };
    let lines = || text.lines().map(str::trim).filter(|l| !l.is_empty());
    lines()
        .find(|l| l.starts_with("Error") || l.starts_with("BLAST"))
        .or_else(|| lines().find(|l| l.contains("Error") || l.contains("error")))
        .or_else(|| lines().find(|l| l.contains("Warning")))
        .or_else(|| lines().next())
}

/// `line` with every token that holds an absolute path (at its start after `[`, `(`, a quote or a
/// backtick, or after `=` as in `key=/path`) replaced by `<path>`, cut to `DETAIL_MAX` characters.
fn scrub(line: &str) -> String {
    const OPENERS: [char; 5] = ['[', '(', '"', '\'', '`'];
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|w| {
            let starts_path = |t: &str| t.trim_start_matches(OPENERS).starts_with('/');
            if starts_path(w) || w.split('=').skip(1).any(starts_path) {
                "<path>"
            } else {
                w
            }
        })
        .collect();
    let joined = words.join(" ");
    match joined.char_indices().nth(DETAIL_MAX) {
        None => joined,
        Some((cut, _)) => format!("{}…", &joined[..cut]),
    }
}

/// The user message for a non-zero exit: fixed text for the known BLAST+ failures and the
/// container's memory kill; otherwise `BLAST failed (exit N)` with the first error line, paths
/// removed and length bounded.
fn failure_message(status: ExitStatus, first: Option<&str>) -> String {
    if status.signal() == Some(9) {
        return OOM_MESSAGE.into();
    }
    if let Some(line) = first {
        if line.contains("BLAST Database error") {
            return "database unavailable".into();
        }
        if line.starts_with("Error") && line.contains("genetic code") {
            return "invalid genetic code".into();
        }
        if line.contains("no valid query") || line.contains("No valid query") {
            return "the query has no valid residues".into();
        }
    }
    let head = match (status.code(), status.signal()) {
        (Some(code), _) => format!("BLAST failed (exit {code})"),
        (None, Some(sig)) => format!("BLAST failed (signal {sig})"),
        (None, None) => "BLAST failed".to_string(),
    };
    match first {
        Some(line) => format!("{head}: {}", scrub(line)),
        None => head,
    }
}

/// Writes `results.tsv` in `job_dir`: `HEADER`, then `raw.tsv`. When the two together exceed `cap`
/// bytes, `raw.tsv` is cut after its last `\n` that keeps the file within `cap`, and `true` is
/// returned. `raw.tsv` is deleted afterwards. The file is written to `results.tsv.tmp`, synced and
/// renamed, so a reader never sees a partial file. `cap` must hold the header (`Config` enforces
/// at least 1024 bytes).
pub fn finalize_results(job_dir: &Path, cap: u64) -> io::Result<bool> {
    let raw_path = job_dir.join(RAW_FILE);
    let tmp_path = job_dir.join("results.tsv.tmp");
    let mut raw = std::fs::File::open(&raw_path)?;
    let raw_len = raw.metadata()?.len();
    let header = HEADER.as_bytes();
    let budget = cap.saturating_sub(header.len() as u64);
    let (keep, truncated) = if raw_len <= budget {
        (raw_len, false)
    } else {
        (last_row_end(&mut raw, budget)?, true)
    };
    raw.seek(SeekFrom::Start(0))?;
    let mut out = std::fs::File::create(&tmp_path)?;
    out.write_all(header)?;
    let copied = io::copy(&mut (&mut raw).take(keep), &mut out)?;
    if copied != keep {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "raw.tsv shrank while being copied",
        ));
    }
    out.sync_all()?;
    drop(out);
    std::fs::rename(&tmp_path, job_dir.join(RESULTS_FILE))?;
    std::fs::remove_file(&raw_path)?;
    Ok(truncated)
}

/// Length of the longest prefix of the first `limit` bytes of `f` that ends in `\n` (0 if none),
/// scanning backwards from `limit` in blocks.
fn last_row_end(f: &mut std::fs::File, limit: u64) -> io::Result<u64> {
    const BLOCK: u64 = 64 * 1024;
    let mut end = limit;
    let mut buf = vec![0u8; BLOCK as usize];
    while end > 0 {
        let start = end.saturating_sub(BLOCK);
        let len = (end - start) as usize;
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut buf[..len])?;
        if let Some(i) = buf[..len].iter().rposition(|&b| b == b'\n') {
            return Ok(start + i as u64 + 1);
        }
        end = start;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_dir(raw: &[u8]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(RAW_FILE), raw).unwrap();
        d
    }

    fn results(d: &tempfile::TempDir) -> Vec<u8> {
        std::fs::read(d.path().join(RESULTS_FILE)).unwrap()
    }

    const H: u64 = HEADER.len() as u64;

    #[test]
    fn small_output_is_header_plus_raw() {
        let d = raw_dir(b"a\tb\n");
        assert!(!finalize_results(d.path(), 1024).unwrap());
        assert_eq!(results(&d), [HEADER.as_bytes(), b"a\tb\n"].concat());
        assert!(!d.path().join(RAW_FILE).exists(), "raw.tsv deleted");
        assert!(!d.path().join("results.tsv.tmp").exists());
    }

    #[test]
    fn empty_output_is_header_only() {
        let d = raw_dir(b"");
        assert!(!finalize_results(d.path(), 1024).unwrap());
        assert_eq!(results(&d), HEADER.as_bytes());
    }

    #[test]
    fn exactly_at_cap_is_not_truncated() {
        let row = b"0123456789\n";
        let raw: Vec<u8> = row.repeat(20);
        let cap = H + raw.len() as u64;
        let d = raw_dir(&raw);
        assert!(!finalize_results(d.path(), cap).unwrap());
        assert_eq!(results(&d).len() as u64, cap);
    }

    #[test]
    fn one_byte_over_cuts_the_last_row() {
        let row = b"0123456789\n";
        let raw: Vec<u8> = row.repeat(20);
        let cap = H + raw.len() as u64 - 1;
        let d = raw_dir(&raw);
        assert!(finalize_results(d.path(), cap).unwrap());
        let r = results(&d);
        assert_eq!(r, [HEADER.as_bytes(), &row.repeat(19)].concat());
        assert!(r.len() as u64 <= cap);
    }

    #[test]
    fn cut_lands_on_a_row_boundary_for_every_cap() {
        // Rows of varying length; the cut must keep whole rows only and be the longest such prefix.
        let mut raw = Vec::new();
        for i in 0..300 {
            raw.extend(std::iter::repeat_n(b'x', i % 37 + 1));
            raw.push(b'\n');
        }
        for cap in (H..H + raw.len() as u64 + 5).step_by(97) {
            let d = raw_dir(&raw);
            let truncated = finalize_results(d.path(), cap).unwrap();
            let r = results(&d);
            assert!(r.len() as u64 <= cap, "cap {cap}");
            assert!(r.starts_with(HEADER.as_bytes()));
            let body = &r[HEADER.len()..];
            assert!(raw.starts_with(body));
            assert!(body.is_empty() || body.ends_with(b"\n"));
            assert_eq!(truncated, body.len() < raw.len(), "cap {cap}");
            // Longest: the next row would not have fit.
            if truncated {
                let next_end =
                    raw[body.len()..].iter().position(|&b| b == b'\n').unwrap() + body.len() + 1;
                assert!(H + next_end as u64 > cap, "cap {cap}: cut too early");
            }
        }
    }

    #[test]
    fn row_longer_than_a_scan_block_is_found() {
        // One 200 KiB row then a short one: the last newline within budget is 200 KiB back.
        let mut raw = vec![b'y'; 200 * 1024];
        raw.push(b'\n');
        raw.extend(vec![b'z'; 300 * 1024]);
        raw.push(b'\n');
        let cap = H + 400 * 1024;
        let d = raw_dir(&raw);
        assert!(finalize_results(d.path(), cap).unwrap());
        assert_eq!(results(&d).len(), HEADER.len() + 200 * 1024 + 1);
    }

    #[test]
    fn no_newline_within_budget_leaves_header_only() {
        let d = raw_dir(&vec![b'q'; 4096]);
        assert!(finalize_results(d.path(), 1024).unwrap());
        assert_eq!(results(&d), HEADER.as_bytes());
    }

    #[test]
    fn missing_raw_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(finalize_results(d.path(), 1024).is_err());
        assert!(!d.path().join(RESULTS_FILE).exists());
    }

    #[test]
    fn header_matches_outfmt_columns() {
        let cols: Vec<&str> = HEADER.trim_end().split('\t').collect();
        assert_eq!(cols.len(), 13);
        assert_eq!(cols.first(), Some(&"qseqid"));
        assert_eq!(cols.last(), Some(&"stitle"));
        assert!(HEADER.ends_with('\n') && !HEADER.ends_with("\n\n"));
    }

    fn status(raw: i32) -> ExitStatus {
        ExitStatus::from_raw(raw)
    }

    #[test]
    fn failure_messages() {
        // Raw wait statuses: exit code N is N << 8; signal S is S.
        assert_eq!(failure_message(status(9), None), OOM_MESSAGE);
        assert_eq!(
            failure_message(
                status(2 << 8),
                Some(
                    "BLAST Database error: No alias or index file found for nucleotide database [/db/dmel/x] in search path [/tmp::]"
                )
            ),
            "database unavailable"
        );
        assert_eq!(
            failure_message(status(1 << 8), Some("Error: Invalid genetic code 99")),
            "invalid genetic code"
        );
        assert_eq!(
            failure_message(
                status(1 << 8),
                Some(
                    "Warning: [blastn] Query_1 query: Could not calculate ungapped Karlin-Altschul parameters due to an invalid query sequence or its translation. Please verify the query sequence(s) and/or filtering options; no valid query"
                )
            ),
            "the query has no valid residues"
        );
        let m = failure_message(
            status(3 << 8),
            Some("BLAST query/options error: Invalid value for word_size in /etc/x/y.conf"),
        );
        assert_eq!(
            m,
            "BLAST failed (exit 3): BLAST query/options error: Invalid value for word_size in <path>"
        );
        assert_eq!(
            failure_message(status(3 << 8), None),
            "BLAST failed (exit 3)"
        );
        assert_eq!(
            failure_message(status(15), None),
            "BLAST failed (signal 15)"
        );
        assert_eq!(
            scrub("cannot open `/data/blast/x' or file=/data/y (key='/z') ok=1 a/b"),
            "cannot open <path> or <path> <path> ok=1 a/b"
        );
        let long = "Error: ".to_string() + &"e".repeat(5000);
        assert!(failure_message(status(1 << 8), Some(&long)).chars().count() < DETAIL_MAX + 40);
    }

    #[test]
    fn first_error_line_prefers_errors() {
        let s = b"\n  some banner\nWarning: thing\nError: bad\n";
        assert_eq!(first_error_line(s), Some("Error: bad"));
        let s = b"Warning: [blastn] Examining 5 or more matches is recommended\nBLAST query/options error: bad word_size\n";
        assert_eq!(
            first_error_line(s),
            Some("BLAST query/options error: bad word_size")
        );
        assert_eq!(
            first_error_line(b"banner\nsomething error-ish\nWarning: w\n"),
            Some("something error-ish")
        );
        assert_eq!(
            first_error_line(b"banner\nWarning: w\n"),
            Some("Warning: w")
        );
        assert_eq!(first_error_line(b"\n\nplain\n"), Some("plain"));
        assert_eq!(first_error_line(b""), None);
        // A cut multi-byte character at the end is tolerated.
        let mut cut = b"Error: caf".to_vec();
        cut.push(0xC3);
        assert_eq!(first_error_line(&cut), Some("Error: caf"));
    }
}
