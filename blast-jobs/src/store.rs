//! The job store: an in-memory FIFO plus a map of every known job behind one mutex, a `Notify` that
//! wakes workers, and one directory per job under `state_root/jobs/<id>/` (design 3.6).
//!
//! On disk a job directory holds `job.json` (request summary, timestamps, state, error, truncated;
//! rewritten atomically through `job.json.tmp` + rename at every transition), `query.fa`, and what the
//! runner adds (`raw.tsv`, `results.tsv`, `stderr.txt`). The query itself is not persisted in
//! `job.json`: a job that was queued or running when the service stopped is marked `failed` at the next
//! start, never re-run.
//!
//! Blocking: `create`, `claim_next` and `finish` are async and run their file IO (small files and an
//! fsync) on the blocking pool via `spawn_blocking`, never while the mutex is held. `open` and `sweep`
//! are synchronous (startup, and the sweeper already runs `sweep` on the blocking pool). `get`,
//! `counts`, `state_writable`, `job_dir` and `notified` are memory-only.
//!
//! Cancellation: the in-memory transition of `claim_next` and `finish` happens before their first
//! await, so a dropped future loses at most the `job.json` write. Workers should not cancel
//! `claim_next` (a dropped one would leave a job `running` with nobody to finish it). A dropped
//! `create` gives back its queue slot; a directory it had already written stays `queued` on disk and
//! the next start marks it failed, after which the sweep removes it.
//!
//! One writer per job: after `claim_next` hands a job out, only that worker calls `finish` for it, so
//! the two `job.json` writes of a job never race. `sweep` only touches terminal jobs.
use crate::command::QUERY_FILE;
use crate::jobid::JobId;
use crate::types::{JobStatus, Program, State};
use crate::validate::Validated;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

/// The error a queued or running job gets when the service finds it on disk at startup.
pub const RESTART_MESSAGE: &str = "service restarted before the search finished";
/// How often `spawn_sweeper` callers should run the retention sweep (design 3.6).
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(300);

const JOBS_DIR: &str = "jobs";
const JOB_FILE: &str = "job.json";
const JOB_TMP: &str = "job.json.tmp";
const PROBE_FILE: &str = ".write-probe";

/// Why `Store::create` refused a job. The API maps `QueueFull` to `429 queue_full` and `Io` to `503`.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("the queue is full")]
    QueueFull,
    #[error("the job directory could not be written: {0}")]
    Io(#[from] io::Error),
}

/// How a running job ended, as the runner reports it to `Store::finish`.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Complete {
        truncated: bool,
    },
    /// The user-facing error message (already free of paths).
    Failed(String),
}

/// `job.json`. Never contains the query residues.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Record {
    job_id: String,
    state: State,
    submitted_at_ms: u64,
    #[serde(default)]
    started_at_ms: Option<u64>,
    #[serde(default)]
    finished_at_ms: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    truncated: bool,
    program: Program,
    database: String,
    species: Vec<String>,
    query_length: usize,
    #[serde(default)]
    client: Option<String>,
}

struct Job {
    record: Record,
    /// Present while queued; taken by `claim_next`. Jobs reloaded from disk never have one.
    request: Option<Validated>,
}

#[derive(Default)]
struct Inner {
    jobs: HashMap<JobId, Job>,
    fifo: VecDeque<JobId>,
    /// Slots taken by `create` calls that are writing their directory outside the lock.
    reserved: usize,
}

pub struct Store {
    jobs_root: PathBuf,
    max_queued: usize,
    inner: Mutex<Inner>,
    notify: Notify,
    writable: AtomicBool,
}

impl Store {
    /// Opens (creating if needed) `state_root/jobs`, records whether it is writable, reloads `complete`
    /// and `failed` jobs, and marks `queued` and `running` ones `failed` with `RESTART_MESSAGE`.
    /// Entries that are not a job directory with a readable `job.json` naming itself are left alone and
    /// logged here, once. An unwritable directory is not an error (health reports it); an existing
    /// directory that cannot be listed is.
    pub fn open(state_root: &Path, max_queued: usize) -> anyhow::Result<Arc<Store>> {
        let jobs_root = state_root.join(JOBS_DIR);
        let writable = match std::fs::create_dir_all(&jobs_root).and_then(|()| probe(&jobs_root)) {
            Ok(()) => true,
            Err(e) => {
                tracing::error!(path = %jobs_root.display(), error = %e, "job directory is not writable");
                false
            }
        };
        let store = Store {
            jobs_root,
            max_queued,
            inner: Mutex::new(Inner::default()),
            notify: Notify::new(),
            writable: AtomicBool::new(writable),
        };

        let entries = match std::fs::read_dir(&store.jobs_root) {
            Ok(rd) => Some(rd),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(e).with_context(|| format!("listing {}", store.jobs_root.display()));
            }
        };
        let (mut loaded, mut recovered) = (0usize, 0usize);
        let now = now_ms(SystemTime::now());
        let mut jobs = HashMap::new();
        for entry in entries.into_iter().flatten() {
            let entry = entry.with_context(|| format!("listing {}", store.jobs_root.display()))?;
            let name = entry.file_name();
            if name == PROBE_FILE {
                continue;
            }
            let Some((id, mut record)) = load_entry(&entry, &name) else {
                tracing::warn!(entry = %name.to_string_lossy(), "leaving unknown entry in jobs/ alone");
                continue;
            };
            if matches!(record.state, State::Queued | State::Running) {
                record.state = State::Failed;
                record.error = Some(RESTART_MESSAGE.to_owned());
                record.finished_at_ms = Some(now);
                let written = write_record(&store.job_dir(&id), &record);
                store.note_write(&id, written);
                recovered += 1;
            }
            loaded += 1;
            jobs.insert(
                id,
                Job {
                    record,
                    request: None,
                },
            );
        }
        store.lock().jobs = jobs;
        tracing::info!(
            loaded,
            recovered,
            writable,
            path = %store.jobs_root.display(),
            "job store opened"
        );
        Ok(Arc::new(store))
    }

    /// Queues a validated request: writes `job.json` then `query.fa`, pushes it on the FIFO and wakes
    /// a worker. Returns the id and the 1-based queue position. `QueueFull` when the FIFO already holds
    /// `max_queued` jobs (running ones do not count); `Io` when the directory cannot be written, which
    /// also turns `state_writable` false until a later write succeeds.
    pub async fn create(&self, req: Validated) -> Result<(JobId, u32), CreateError> {
        let slot = self.reserve()?;
        let id = JobId::new();
        let record = Record {
            job_id: id.to_string(),
            state: State::Queued,
            submitted_at_ms: now_ms(SystemTime::now()),
            started_at_ms: None,
            finished_at_ms: None,
            error: None,
            truncated: false,
            program: req.program,
            database: req.database.clone(),
            species: req.species.clone(),
            query_length: req.residues.len(),
            client: req.client.clone(),
        };
        let (jobs_root, dir) = (self.jobs_root.clone(), self.job_dir(&id));
        let joined = tokio::task::spawn_blocking(move || {
            let written = write_new_job(&jobs_root, &dir, &req.residues, &record);
            (written, req, record)
        })
        .await;
        let (req, record) = match joined {
            Ok((Ok(()), req, record)) => (req, record),
            Ok((Err(e), _, _)) => return Err(self.create_failed(e)),
            Err(e) => return Err(self.create_failed(io::Error::other(e))),
        };
        self.writable.store(true, Ordering::Relaxed);
        let mut inner = self.lock();
        slot.commit(&mut inner);
        inner.jobs.insert(
            id.clone(),
            Job {
                record,
                request: Some(req),
            },
        );
        inner.fifo.push_back(id.clone());
        let position = u32::try_from(inner.fifo.len()).unwrap_or(u32::MAX);
        drop(inner);
        // `notify_one` stores a permit when no worker is waiting yet, so a worker that found the FIFO
        // empty just before this push still wakes on its next `notified().await`.
        self.notify.notify_one();
        Ok((id, position))
    }

    /// Pops the oldest queued job, marks it `running` (memory, then `job.json`) and hands its request
    /// to the caller. `None` when the FIFO is empty. Workers loop: `claim_next().await`, else
    /// `notified().await`, then try again. When jobs remain after the pop another worker is woken, so
    /// two creates that coalesced into one stored permit still reach two idle workers.
    pub async fn claim_next(&self) -> Option<(JobId, Validated)> {
        let (id, req, snapshot, more) = {
            let mut inner = self.lock();
            loop {
                let id = inner.fifo.pop_front()?;
                let Some(job) = inner.jobs.get_mut(&id) else {
                    continue;
                };
                let Some(req) = job.request.take() else {
                    continue;
                };
                job.record.state = State::Running;
                job.record.started_at_ms = Some(now_ms(SystemTime::now()));
                let snapshot = job.record.clone();
                break (id, req, snapshot, !inner.fifo.is_empty());
            }
        };
        if more {
            self.notify.notify_one();
        }
        self.persist(&id, snapshot).await;
        Some((id, req))
    }

    /// Resolves when `create` has queued a job (or immediately, if one was queued since the last
    /// wake-up). Spurious wake-ups are possible; `claim_next` decides.
    pub fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.notify.notified()
    }

    /// The contract's status for a known job: `queue_position` 0 while running, the 1-based FIFO
    /// position while queued, absent once terminal. `truncated` is set for a cut `results.tsv`.
    pub fn get(&self, id: &JobId) -> Option<JobStatus> {
        let inner = self.lock();
        let job = inner.jobs.get(id)?;
        let queue_position = match job.record.state {
            State::Running => Some(0),
            State::Queued => inner
                .fifo
                .iter()
                .position(|j| j == id)
                .map(|p| u32::try_from(p + 1).unwrap_or(u32::MAX)),
            _ => None,
        };
        Some(JobStatus {
            job_id: id.to_string(),
            state: job.record.state,
            queue_position,
            error: job.record.error.clone(),
            result_url: None,
            downloads: None,
            truncated: job.record.truncated,
        })
    }

    /// `state_root/jobs/<id>`. Only a parsed `JobId` can name it, so the result never leaves `jobs/`.
    pub fn job_dir(&self, id: &JobId) -> PathBuf {
        self.jobs_root.join(id.as_str())
    }

    /// Records the end of a job and rewrites its `job.json`. A job already terminal or unknown (swept)
    /// is left as it is.
    pub async fn finish(&self, id: &JobId, outcome: Outcome) {
        let snapshot = {
            let mut inner = self.lock();
            let Some(job) = inner.jobs.get_mut(id) else {
                tracing::warn!(job_id = %id, "finish for an unknown job ignored");
                return;
            };
            let was_queued = match job.record.state {
                State::Queued => true,
                State::Running => false,
                _ => {
                    tracing::warn!(job_id = %id, "finish for a job already finished ignored");
                    return;
                }
            };
            match outcome {
                Outcome::Complete { truncated } => {
                    job.record.state = State::Complete;
                    job.record.truncated = truncated;
                    job.record.error = None;
                }
                Outcome::Failed(message) => {
                    job.record.state = State::Failed;
                    job.record.truncated = false;
                    job.record.error = Some(message);
                }
            }
            job.record.finished_at_ms = Some(now_ms(SystemTime::now()));
            job.request = None;
            let snapshot = job.record.clone();
            if was_queued {
                inner.fifo.retain(|j| j != id);
            }
            snapshot
        };
        self.persist(id, snapshot).await;
    }

    /// Removes terminal jobs submitted more than `retention` before `now`, from memory and disk.
    /// Queued and running jobs are never removed, and directories the store does not know (logged
    /// by `open`) are never touched. Returns the number of jobs removed.
    pub fn sweep(&self, now: SystemTime, retention: Duration) -> usize {
        let cutoff =
            now_ms(now).saturating_sub(u64::try_from(retention.as_millis()).unwrap_or(u64::MAX));
        let expired: Vec<JobId> = {
            let mut inner = self.lock();
            let ids: Vec<JobId> = inner
                .jobs
                .iter()
                .filter(|(_, j)| {
                    matches!(
                        j.record.state,
                        State::Complete | State::Failed | State::Cancelled
                    ) && j.record.submitted_at_ms < cutoff
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                inner.jobs.remove(id);
            }
            ids
        };
        for id in &expired {
            match std::fs::remove_dir_all(self.job_dir(id)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    // Still terminal on disk: the next start reloads it and a later sweep retries.
                    tracing::warn!(job_id = %id, error = %e, "could not remove an expired job directory");
                }
            }
        }
        if !expired.is_empty() {
            tracing::info!(removed = expired.len(), "retention sweep");
        }
        expired.len()
    }

    /// `(queued, running)`.
    pub fn counts(&self) -> (usize, usize) {
        let inner = self.lock();
        let running = inner
            .jobs
            .values()
            .filter(|j| j.record.state == State::Running)
            .count();
        (inner.fifo.len(), running)
    }

    /// Whether the last write to the state directory (the startup probe, then every job write)
    /// succeeded.
    pub fn state_writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }

    /// Runs `sweep(now, retention)` every `every` (use `SWEEP_INTERVAL`) on the blocking pool, until
    /// the returned task is aborted. The first sweep runs after one interval.
    pub fn spawn_sweeper(
        self: &Arc<Self>,
        every: Duration,
        retention: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let store = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let s = Arc::clone(&store);
                if let Err(e) =
                    tokio::task::spawn_blocking(move || s.sweep(SystemTime::now(), retention)).await
                {
                    tracing::error!(error = %e, "retention sweep panicked");
                }
            }
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves plain data behind; keep serving.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes a queue slot, or `QueueFull`. The slot is given back when the guard drops uncommitted.
    fn reserve(&self) -> Result<Slot<'_>, CreateError> {
        let mut inner = self.lock();
        if inner.fifo.len() + inner.reserved >= self.max_queued {
            return Err(CreateError::QueueFull);
        }
        inner.reserved += 1;
        Ok(Slot {
            store: self,
            live: true,
        })
    }

    fn create_failed(&self, e: io::Error) -> CreateError {
        self.writable.store(false, Ordering::Relaxed);
        tracing::error!(error = %e, "could not write a new job directory");
        CreateError::Io(e)
    }

    async fn persist(&self, id: &JobId, record: Record) {
        let dir = self.job_dir(id);
        let written = tokio::task::spawn_blocking(move || write_record(&dir, &record))
            .await
            .unwrap_or_else(|e| Err(io::Error::other(e)));
        self.note_write(id, written);
    }

    fn note_write(&self, id: &JobId, written: io::Result<()>) {
        self.writable.store(written.is_ok(), Ordering::Relaxed);
        if let Err(e) = written {
            tracing::error!(job_id = %id, error = %e, "could not write job.json");
        }
    }
}

/// A reserved queue slot held by an in-flight `create`.
struct Slot<'a> {
    store: &'a Store,
    live: bool,
}

impl Slot<'_> {
    /// Turns the reservation into a FIFO entry the caller is about to push (lock already held).
    fn commit(mut self, inner: &mut Inner) {
        inner.reserved -= 1;
        self.live = false;
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if self.live {
            self.store.lock().reserved -= 1;
        }
    }
}

/// Creates `dir` and writes `job.json` (queued) before `query.fa`, so a crash between the two leaves a
/// known job that restart recovery fails and the sweep later removes, never a stray directory holding a
/// query. Removes the directory again on any error.
fn write_new_job(jobs_root: &Path, dir: &Path, residues: &str, record: &Record) -> io::Result<()> {
    std::fs::create_dir_all(jobs_root)?;
    // `create_dir`, not `_all`: an existing directory is an error, never reused or removed.
    std::fs::create_dir(dir)?;
    let res = (|| {
        write_record(dir, record)?;
        let mut q = std::fs::File::create(dir.join(QUERY_FILE))?;
        q.write_all(b">query\n")?;
        q.write_all(residues.as_bytes())?;
        q.write_all(b"\n")?;
        q.sync_all()
    })();
    if res.is_err() {
        let _ = std::fs::remove_dir_all(dir);
    }
    res
}

/// Writes `job.json` atomically: the whole record to `job.json.tmp`, fsync, rename over `job.json`.
fn write_record(dir: &Path, record: &Record) -> io::Result<()> {
    let tmp = dir.join(JOB_TMP);
    let body = serde_json::to_vec_pretty(record).map_err(io::Error::other)?;
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&body)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, dir.join(JOB_FILE))
}

fn probe(jobs_root: &Path) -> io::Result<()> {
    let p = jobs_root.join(PROBE_FILE);
    std::fs::write(&p, b"ok")?;
    std::fs::remove_file(&p)
}

/// A job directory: a real directory (not a symlink) whose name parses as a `JobId` and whose
/// `job.json` parses and names the same id.
fn load_entry(entry: &std::fs::DirEntry, name: &OsStr) -> Option<(JobId, Record)> {
    let id = JobId::parse(name.to_str()?)?;
    if !entry.file_type().ok()?.is_dir() {
        return None;
    }
    let bytes = std::fs::read(entry.path().join(JOB_FILE)).ok()?;
    let record: Record = serde_json::from_slice(&bytes).ok()?;
    (record.job_id == id.as_str()).then_some((id, record))
}

fn now_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Resolved;
    use std::future::Future;
    use std::os::unix::fs::PermissionsExt;

    fn validated(residues: &str) -> Validated {
        Validated {
            program: Program::Blastn,
            database: "assembly".into(),
            species: vec!["dmel".into()],
            residues: residues.into(),
            evalue: 10.0,
            max_hits: 100,
            megablast: false,
            matrix: None,
            genetic_code: None,
            advanced: vec![],
            resolved: Resolved {
                protein: false,
                prefixes: vec![PathBuf::from("/db/dmel/dmel-all-chromosome")],
            },
            client: Some("test".into()),
        }
    }

    fn record_on_disk(store_root: &Path, id: &str, state: State, submitted: SystemTime) {
        let dir = store_root.join(JOBS_DIR).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let r = Record {
            job_id: id.into(),
            state,
            submitted_at_ms: now_ms(submitted),
            started_at_ms: None,
            finished_at_ms: None,
            error: None,
            truncated: false,
            program: Program::Blastp,
            database: "translation".into(),
            species: vec!["dmel".into()],
            query_length: 10,
            client: None,
        };
        std::fs::write(dir.join(JOB_FILE), serde_json::to_vec(&r).unwrap()).unwrap();
    }

    fn read_record(store: &Store, id: &JobId) -> Record {
        serde_json::from_slice(&std::fs::read(store.job_dir(id).join(JOB_FILE)).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn create_writes_query_and_job_json() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();
        assert!(store.state_writable());
        let (id, pos) = store.create(validated("ACGTACGT")).await.unwrap();
        assert_eq!(pos, 1);
        let dir = store.job_dir(&id);
        assert_eq!(dir, tmp.path().join("jobs").join(id.as_str()));
        assert_eq!(
            std::fs::read_to_string(dir.join(QUERY_FILE)).unwrap(),
            ">query\nACGTACGT\n"
        );
        let r = read_record(&store, &id);
        assert_eq!(r.state, State::Queued);
        assert_eq!(r.job_id, id.as_str());
        assert_eq!(r.query_length, 8);
        let raw = std::fs::read_to_string(dir.join(JOB_FILE)).unwrap();
        assert!(!raw.contains("ACGTACGT"), "job.json must not hold residues");
        assert!(!dir.join(JOB_TMP).exists());
        // The probe file is cleaned up.
        assert!(!tmp.path().join("jobs").join(PROBE_FILE).exists());
    }

    #[tokio::test]
    async fn fifo_positions_queue_full_and_claim_shifts() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 3).unwrap();
        let (a, pa) = store.create(validated("AAAA")).await.unwrap();
        let (b, pb) = store.create(validated("CCCC")).await.unwrap();
        let (c, pc) = store.create(validated("GGGG")).await.unwrap();
        assert_eq!((pa, pb, pc), (1, 2, 3));
        assert!(matches!(
            store.create(validated("TTTT")).await,
            Err(CreateError::QueueFull)
        ));
        assert_eq!(store.counts(), (3, 0));
        for (id, p) in [(&a, 1), (&b, 2), (&c, 3)] {
            let s = store.get(id).unwrap();
            assert_eq!((s.state, s.queue_position), (State::Queued, Some(p)));
        }

        let (claimed, req) = store.claim_next().await.unwrap();
        assert_eq!(claimed, a, "FIFO order");
        assert_eq!(req.residues, "AAAA");
        let s = store.get(&a).unwrap();
        assert_eq!((s.state, s.queue_position), (State::Running, Some(0)));
        assert_eq!(read_record(&store, &a).state, State::Running);
        assert!(read_record(&store, &a).started_at_ms.is_some());
        assert_eq!(store.get(&b).unwrap().queue_position, Some(1));
        assert_eq!(store.get(&c).unwrap().queue_position, Some(2));
        assert_eq!(store.counts(), (2, 1));

        // A running job frees its queue slot.
        let (d, pd) = store.create(validated("TTTT")).await.unwrap();
        assert_eq!(pd, 3);
        assert!(matches!(
            store.create(validated("TTTT")).await,
            Err(CreateError::QueueFull)
        ));

        assert_eq!(store.claim_next().await.unwrap().0, b);
        assert_eq!(store.claim_next().await.unwrap().0, c);
        assert_eq!(store.claim_next().await.unwrap().0, d);
        assert!(store.claim_next().await.is_none());
        assert_eq!(store.counts(), (0, 4));
    }

    #[tokio::test]
    async fn finish_records_outcome_and_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();
        let (ok, _) = store.create(validated("AAAA")).await.unwrap();
        let (bad, _) = store.create(validated("CCCC")).await.unwrap();
        store.claim_next().await.unwrap();
        store.claim_next().await.unwrap();
        store
            .finish(&ok, Outcome::Complete { truncated: true })
            .await;
        store
            .finish(&bad, Outcome::Failed("BLAST failed (exit 2)".into()))
            .await;
        // A second finish does not overwrite the first.
        store.finish(&ok, Outcome::Failed("late".into())).await;
        assert_eq!(store.counts(), (0, 0));

        let s = store.get(&ok).unwrap();
        assert_eq!(
            (s.state, s.queue_position, s.truncated, s.error.as_deref()),
            (State::Complete, None, true, None)
        );
        for id in [&ok, &bad] {
            assert!(!store.job_dir(id).join(JOB_TMP).exists());
        }
        drop(store);

        let store = Store::open(tmp.path(), 5).unwrap();
        let s = store.get(&ok).unwrap();
        assert_eq!((s.state, s.truncated), (State::Complete, true));
        let s = store.get(&bad).unwrap();
        assert_eq!(s.state, State::Failed);
        assert_eq!(s.error.as_deref(), Some("BLAST failed (exit 2)"));
        assert!(
            store.claim_next().await.is_none(),
            "terminal jobs are not requeued"
        );
    }

    #[tokio::test]
    async fn finish_on_a_queued_job_removes_it_from_the_fifo() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();
        let (a, _) = store.create(validated("AAAA")).await.unwrap();
        let (b, _) = store.create(validated("CCCC")).await.unwrap();
        store.finish(&a, Outcome::Failed("x".into())).await;
        assert_eq!(store.get(&b).unwrap().queue_position, Some(1));
        assert_eq!(store.claim_next().await.unwrap().0, b);
        assert!(store.get(&JobId::new()).is_none());
    }

    #[tokio::test]
    async fn restart_fails_queued_and_running_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();
        let (running, _) = store.create(validated("AAAA")).await.unwrap();
        let (queued, _) = store.create(validated("CCCC")).await.unwrap();
        let (done, _) = store.create(validated("GGGG")).await.unwrap();
        assert_eq!(store.claim_next().await.unwrap().0, running);
        store
            .finish(&done, Outcome::Complete { truncated: false })
            .await;
        assert_eq!(read_record(&store, &queued).state, State::Queued);
        // The process dies here: `running` is running and `queued` queued on disk.
        drop(store);

        let store = Store::open(tmp.path(), 5).unwrap();
        for id in [&running, &queued] {
            let s = store.get(id).unwrap();
            assert_eq!(s.state, State::Failed, "{id}");
            assert_eq!(s.error.as_deref(), Some(RESTART_MESSAGE));
            assert_eq!(s.queue_position, None);
            let on_disk = read_record(&store, id);
            assert_eq!(on_disk.state, State::Failed, "recovery is persisted");
            assert_eq!(on_disk.error.as_deref(), Some(RESTART_MESSAGE));
            assert!(on_disk.finished_at_ms.is_some());
        }
        assert_eq!(store.get(&done).unwrap().state, State::Complete);
        assert_eq!(store.counts(), (0, 0));
        assert!(store.claim_next().await.is_none());
    }

    #[tokio::test]
    async fn open_leaves_foreign_entries_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let jobs = tmp.path().join("jobs");
        std::fs::create_dir_all(jobs.join("not a job")).unwrap();
        std::fs::create_dir_all(jobs.join("validname_nojson")).unwrap();
        std::fs::create_dir_all(jobs.join("validname_badjson")).unwrap();
        std::fs::write(jobs.join("validname_badjson").join(JOB_FILE), b"{not json").unwrap();
        // job.json naming another id.
        record_on_disk(
            tmp.path(),
            "validname_mismatch",
            State::Complete,
            SystemTime::now(),
        );
        let mut r: Record = serde_json::from_slice(
            &std::fs::read(jobs.join("validname_mismatch").join(JOB_FILE)).unwrap(),
        )
        .unwrap();
        r.job_id = "someoneelse".into();
        std::fs::write(
            jobs.join("validname_mismatch").join(JOB_FILE),
            serde_json::to_vec(&r).unwrap(),
        )
        .unwrap();
        std::fs::write(jobs.join("stray-file.txt"), b"x").unwrap();

        let store = Store::open(tmp.path(), 5).unwrap();
        for name in [
            "validname_nojson",
            "validname_badjson",
            "validname_mismatch",
        ] {
            assert!(store.get(&JobId::parse(name).unwrap()).is_none(), "{name}");
        }
        assert_eq!(store.counts(), (0, 0));
        assert_eq!(
            store.sweep(
                SystemTime::now() + Duration::from_secs(1 << 30),
                Duration::ZERO
            ),
            0
        );
        for name in [
            "not a job",
            "validname_nojson",
            "validname_badjson",
            "validname_mismatch",
            "stray-file.txt",
        ] {
            assert!(jobs.join(name).exists(), "{name} must be left alone");
        }
    }

    #[tokio::test]
    async fn sweep_removes_only_expired_terminal_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        let day = Duration::from_secs(86_400);
        let now = SystemTime::now();
        record_on_disk(tmp.path(), "old_complete", State::Complete, now - 2 * day);
        record_on_disk(tmp.path(), "old_failed__", State::Failed, now - 2 * day);
        record_on_disk(tmp.path(), "young_complete", State::Complete, now - day / 2);
        std::fs::create_dir_all(tmp.path().join("jobs").join("foreign-dir")).unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();
        // Fresh jobs: one running, one queued; never swept however old `now` makes them.
        let (running, _) = store.create(validated("AAAA")).await.unwrap();
        store.claim_next().await.unwrap();
        let (queued, _) = store.create(validated("CCCC")).await.unwrap();

        assert_eq!(store.sweep(now, day), 2);
        let jobs = tmp.path().join("jobs");
        assert!(!jobs.join("old_complete").exists());
        assert!(!jobs.join("old_failed__").exists());
        assert!(store.get(&JobId::parse("old_complete").unwrap()).is_none());
        assert!(jobs.join("young_complete").exists());
        assert!(
            store
                .get(&JobId::parse("young_complete").unwrap())
                .is_some()
        );
        assert!(jobs.join("foreign-dir").exists());

        let far = now + 30 * day;
        assert_eq!(store.sweep(far, day), 1, "young_complete only");
        assert!(!jobs.join("young_complete").exists());
        assert!(jobs.join("foreign-dir").exists());
        assert!(store.job_dir(&running).exists() && store.job_dir(&queued).exists());
        assert_eq!(store.get(&queued).unwrap().state, State::Queued);
        assert_eq!(store.get(&running).unwrap().state, State::Running);

        store
            .finish(&running, Outcome::Complete { truncated: false })
            .await;
        assert_eq!(store.sweep(far, day), 1);
        assert!(!store.job_dir(&running).exists());
        assert!(store.get(&running).is_none());
        // `finish` after a sweep is ignored rather than resurrecting the directory.
        store.finish(&running, Outcome::Failed("late".into())).await;
        assert!(!store.job_dir(&running).exists());
    }

    #[tokio::test]
    async fn read_only_directory_gives_io_and_flips_writable() {
        let tmp = tempfile::tempdir().unwrap();
        let jobs = tmp.path().join("jobs");
        std::fs::create_dir_all(&jobs).unwrap();
        std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(jobs.join("x"), b"x").is_ok() {
            // Running as root: permissions are not enforced, so this case cannot be produced here.
            std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipped: permissions are not enforced for this user");
            return;
        }
        let store = Store::open(tmp.path(), 5).unwrap();
        assert!(!store.state_writable());
        assert!(matches!(
            store.create(validated("AAAA")).await,
            Err(CreateError::Io(_))
        ));
        assert!(!store.state_writable());
        assert_eq!(store.counts(), (0, 0), "a failed create takes no slot");

        std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (id, pos) = store.create(validated("AAAA")).await.unwrap();
        assert_eq!(pos, 1);
        assert!(store.state_writable(), "a successful write clears the flag");
        assert!(store.job_dir(&id).join(JOB_FILE).exists());
    }

    #[tokio::test]
    async fn open_creates_missing_state_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("a").join("b");
        let store = Store::open(&root, 5).unwrap();
        assert!(store.state_writable());
        assert!(root.join("jobs").is_dir());
    }

    #[tokio::test]
    async fn create_wakes_a_waiting_worker_and_never_loses_a_wakeup() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();

        // A worker already waiting is woken.
        let s = Arc::clone(&store);
        let worker = tokio::spawn(async move {
            loop {
                if let Some((id, _)) = s.claim_next().await {
                    return id;
                }
                s.notified().await;
            }
        });
        tokio::task::yield_now().await;
        let (id, _) = store.create(validated("AAAA")).await.unwrap();
        let got = tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .expect("worker woke")
            .unwrap();
        assert_eq!(got, id);

        // A worker that saw an empty FIFO just before the push still wakes (stored permit).
        assert!(store.claim_next().await.is_none());
        store.create(validated("CCCC")).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), store.notified())
            .await
            .expect("permit was stored");
        assert!(store.claim_next().await.is_some());
    }

    #[tokio::test]
    async fn sweeper_runs_on_its_interval() {
        let tmp = tempfile::tempdir().unwrap();
        record_on_disk(
            tmp.path(),
            "old_complete",
            State::Complete,
            SystemTime::now() - Duration::from_secs(10 * 86_400),
        );
        let store = Store::open(tmp.path(), 5).unwrap();
        let every = Duration::from_millis(200);
        let handle = store.spawn_sweeper(every, Duration::from_secs(86_400));
        let id = JobId::parse("old_complete").unwrap();
        assert!(
            store.get(&id).is_some(),
            "no sweep before the first interval"
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while store.get(&id).is_some() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(store.get(&id).is_none(), "swept after one interval");
        assert!(!store.job_dir(&id).exists());
        handle.abort();
    }

    #[tokio::test]
    async fn coalesced_wakeups_still_reach_every_job() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 5).unwrap();
        // Two creates with no worker waiting: `Notify` keeps a single permit.
        let (a, _) = store.create(validated("AAAA")).await.unwrap();
        let (b, _) = store.create(validated("CCCC")).await.unwrap();
        // Worker one takes the permit and claims `a`; the claim must pass the wake-up on.
        tokio::time::timeout(Duration::from_secs(5), store.notified())
            .await
            .expect("first permit");
        assert_eq!(store.claim_next().await.unwrap().0, a);
        // Worker two, already past its empty-FIFO check, must be woken for `b`.
        tokio::time::timeout(Duration::from_secs(5), store.notified())
            .await
            .expect("claim_next chains the wake-up while jobs remain");
        assert_eq!(store.claim_next().await.unwrap().0, b);
        assert_eq!(store.counts(), (0, 2));
    }

    #[tokio::test]
    async fn crash_before_query_fa_leaves_a_recoverable_sweepable_job() {
        // What `write_new_job` leaves if the process dies after `job.json` and before `query.fa`.
        let tmp = tempfile::tempdir().unwrap();
        let old = SystemTime::now() - Duration::from_secs(2 * 86_400);
        record_on_disk(tmp.path(), "crashed_mid_create", State::Queued, old);
        let store = Store::open(tmp.path(), 5).unwrap();
        let id = JobId::parse("crashed_mid_create").unwrap();
        let s = store.get(&id).unwrap();
        assert_eq!(
            (s.state, s.error.as_deref()),
            (State::Failed, Some(RESTART_MESSAGE))
        );
        assert_eq!(
            store.sweep(SystemTime::now(), Duration::from_secs(86_400)),
            1
        );
        assert!(!store.job_dir(&id).exists());
    }

    #[tokio::test]
    async fn dropped_create_gives_its_slot_back() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 1).unwrap();
        {
            // Polled once (slot reserved, write handed to the blocking pool), then dropped.
            let mut fut = Box::pin(store.create(validated("AAAA")));
            let waker = std::task::Waker::noop();
            let _ = fut
                .as_mut()
                .poll(&mut std::task::Context::from_waker(waker));
        }
        assert_eq!(store.counts(), (0, 0));
        store.create(validated("CCCC")).await.unwrap();
    }
}
