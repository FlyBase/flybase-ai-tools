//! The internal job API (`docs/handoff-private-side.md` §7, design 3.6 to 3.8).
//!
//! - `POST /internal/jobs` -> `202 SubmitResponse`; `400 {error, message}` for a rejected request
//!   (`invalid_request` when the body is not a request object at all, `413` for a body over
//!   `MAX_BODY_BYTES`); `429 {error:"queue_full", retry_after_seconds}`; `503` when the job cannot be
//!   stored.
//! - `GET /internal/jobs/{id}` -> `200 JobStatus` (no `result_url`/`downloads`, ruling 5) or
//!   `404 {error:"unknown_job"}`.
//! - `GET /internal/jobs/{id}/results.tsv` -> `200 text/tab-separated-values` with `Content-Length`
//!   and `X-Blast-Jobs-Truncated: true` when the file was cut at the cap; `409 {error:"not_complete",
//!   state}` before completion; `404`.
//! - `GET /internal/health[?liveness=1]` -> `200` or `503`, unauthenticated.
//!
//! The three job routes sit behind the bearer middleware (`auth::require_bearer`). Job ids pass
//! `JobId::parse` before any lookup, so a malformed id is a `404` and never reaches a path.
use crate::auth::{self, Auth};
use crate::catalogue::Catalogue;
use crate::config::Config;
use crate::jobid::JobId;
use crate::runner::RESULTS_FILE;
use crate::store::{CreateError, Store};
use crate::types::{Program, State, SubmitRequest, SubmitResponse};
use crate::validate;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, State as AxState};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use serde::Deserialize;
use serde_json::json;
use std::path::Path as FsPath;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinHandle;

/// Largest accepted request body. A maximal query (100,000 residues) is about 100 KB of JSON.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// `retry_after_seconds` of a `429`, the MCP's default as well.
pub const RETRY_AFTER_SECONDS: u32 = 30;
/// Response header set on `results.tsv` when the file was cut at `max_results_bytes` (ruling 3).
pub const TRUNCATED_HEADER: &str = "x-blast-jobs-truncated";
/// `Content-Type` of `results.tsv`.
pub const TSV_CONTENT_TYPE: &str = "text/tab-separated-values; charset=utf-8";
/// `poll_after_seconds` when the job should start within the next free slot, and otherwise.
pub const POLL_SOON: u32 = 5;
pub const POLL_LATER: u32 = 10;

/// What every handler shares.
pub struct AppState {
    cfg: Arc<Config>,
    catalogue: Arc<Catalogue>,
    store: Arc<Store>,
    auth: Auth,
    /// The runner's worker tasks; health is degraded once any of them has ended.
    workers: Vec<JoinHandle<()>>,
}

impl AppState {
    pub fn new(
        cfg: Arc<Config>,
        catalogue: Arc<Catalogue>,
        store: Arc<Store>,
        auth: Auth,
        workers: Vec<JoinHandle<()>>,
    ) -> Arc<AppState> {
        Arc::new(AppState {
            cfg,
            catalogue,
            store,
            auth,
            workers,
        })
    }

    pub fn auth(&self) -> &Auth {
        &self.auth
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }
}

/// The four routes. Health is outside the bearer layer.
pub fn router(state: Arc<AppState>) -> Router {
    let jobs = Router::new()
        .route("/internal/jobs", post(submit))
        .route("/internal/jobs/{id}", get(status))
        .route("/internal/jobs/{id}/results.tsv", get(results))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer,
        ));
    Router::new()
        .merge(jobs)
        .route("/internal/health", get(health))
        .fallback(|| async { error(StatusCode::NOT_FOUND, json!({"error": "not_found"})) })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

fn error(status: StatusCode, body: serde_json::Value) -> Response {
    (status, Json(body)).into_response()
}

/// Longest extractor message passed back in an `invalid_request`.
const MESSAGE_MAX: usize = 200;

/// `s` cut on a char boundary to at most `max` characters, with `…` when cut.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn unknown_job() -> Response {
    error(StatusCode::NOT_FOUND, json!({"error": "unknown_job"}))
}

/// `poll_after_seconds` (design 3.6): 5 when the job is within the slots free right now, else 10.
pub fn poll_after(queue_position: u32, running: usize, concurrency: usize) -> u32 {
    let idle = concurrency.saturating_sub(running);
    if (queue_position as usize) <= idle {
        POLL_SOON
    } else {
        POLL_LATER
    }
}

async fn submit(
    AxState(st): AxState<Arc<AppState>>,
    body: Result<Json<SubmitRequest>, JsonRejection>,
) -> Response {
    let started = Instant::now();
    let req = match body {
        Ok(Json(r)) => r,
        Err(rej) => {
            let status = if rej.status() == StatusCode::PAYLOAD_TOO_LARGE {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            let message = if status == StatusCode::PAYLOAD_TOO_LARGE {
                format!("the request body is larger than {MAX_BODY_BYTES} bytes")
            } else {
                // serde_json's message names the field and position; a value it quotes is clipped.
                clip(&rej.body_text(), MESSAGE_MAX)
            };
            tracing::info!(
                outcome = "invalid_request",
                status = status.as_u16(),
                duration_ms = started.elapsed().as_millis() as u64,
                "submit rejected"
            );
            return error(
                status,
                json!({"error": "invalid_request", "message": message}),
            );
        }
    };
    let client = req.client.clone();
    let v = match validate::check(&req, &st.cfg, &st.catalogue) {
        Ok(v) => v,
        Err(rej) => {
            tracing::info!(
                outcome = rej.code.as_str(),
                program = %validate::quoted(&req.program),
                database = %validate::quoted(&req.database),
                species = req.species.len(),
                client = %validate::quoted(client.as_deref().unwrap_or("")),
                duration_ms = started.elapsed().as_millis() as u64,
                "submit rejected"
            );
            return (StatusCode::BAD_REQUEST, Json(rej)).into_response();
        }
    };
    let (program, database, species) = (v.program, v.database.clone(), v.species.len());
    match st.store.create(v).await {
        Ok((id, position)) => {
            let (_, running) = st.store.counts();
            let poll = poll_after(position, running, st.cfg.concurrency);
            tracing::info!(
                job_id = %id, %program, %database, species,
                client = client.as_deref().unwrap_or(""),
                queue_position = position,
                duration_ms = started.elapsed().as_millis() as u64,
                outcome = "queued",
                "job submitted"
            );
            let body = SubmitResponse {
                job_id: id.to_string(),
                state: State::Queued,
                queue_position: Some(position),
                poll_after_seconds: Some(poll),
                database_release: Some(st.cfg.database_release.clone()),
            };
            (StatusCode::ACCEPTED, Json(body)).into_response()
        }
        Err(CreateError::QueueFull) => {
            tracing::warn!(
                %program, %database, species,
                client = client.as_deref().unwrap_or(""),
                duration_ms = started.elapsed().as_millis() as u64,
                outcome = "queue_full",
                "submit refused"
            );
            let mut r = error(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error": "queue_full", "retry_after_seconds": RETRY_AFTER_SECONDS}),
            );
            r.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(RETRY_AFTER_SECONDS));
            r
        }
        Err(CreateError::Io(e)) => {
            tracing::error!(
                %program, %database, error = %e, outcome = "unavailable",
                duration_ms = started.elapsed().as_millis() as u64,
                "submit failed: the job could not be stored"
            );
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": "unavailable", "message": "the job could not be stored; try again later"}),
            )
        }
    }
}

async fn status(AxState(st): AxState<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let Some(id) = JobId::parse(&id) else {
        return unknown_job();
    };
    match st.store.get(&id) {
        Some(s) => Json(s).into_response(),
        None => unknown_job(),
    }
}

async fn results(AxState(st): AxState<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let started = Instant::now();
    let Some(id) = JobId::parse(&id) else {
        return unknown_job();
    };
    let Some(s) = st.store.get(&id) else {
        return unknown_job();
    };
    if s.state != State::Complete {
        return error(
            StatusCode::CONFLICT,
            json!({"error": "not_complete", "state": s.state}),
        );
    }
    let path = st.store.job_dir(&id).join(RESULTS_FILE);
    let body = match tokio::fs::read(&path).await {
        Ok(b) => b,
        // Swept between the lookup and the read.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return unknown_job(),
        Err(e) => {
            tracing::error!(job_id = %id, error = %e, "could not read results.tsv");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": "unavailable", "message": "the results could not be read; try again later"}),
            );
        }
    };
    if body.len() as u64 > st.cfg.max_results_bytes {
        // The runner never writes more than the cap; refuse rather than send the MCP an over-limit body.
        tracing::error!(job_id = %id, bytes = body.len(), "results.tsv is larger than max_results_bytes");
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": "unavailable", "message": "the results file is larger than the service limit"}),
        );
    }
    tracing::info!(
        job_id = %id, bytes = body.len(), truncated = s.truncated,
        duration_ms = started.elapsed().as_millis() as u64,
        "results served"
    );
    let mut r = (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static(TSV_CONTENT_TYPE),
        )],
        body,
    )
        .into_response();
    if s.truncated {
        r.headers_mut()
            .insert(TRUNCATED_HEADER, HeaderValue::from_static("true"));
    }
    r
}

#[derive(Deserialize)]
struct HealthQuery {
    liveness: Option<String>,
}

/// Every BLAST+ program binary under `dir` is a regular file with an execute bit. Used by health and
/// by `blast-jobs check`.
pub fn probe_binaries(dir: &FsPath) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    for p in Program::ALL {
        let path = dir.join(p.as_str());
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() && m.permissions().mode() & 0o111 != 0 => {}
            Ok(_) => return Err(format!("{p} is not an executable file")),
            Err(_) => return Err(format!("{p} is missing")),
        }
    }
    Ok(())
}

/// `200 {status:"ok", queued, running, database_release}` when jobs can be accepted, else
/// `503 {status:"degraded", reason}` (design 3.8). `?liveness=1` skips the queue check, so a busy
/// service is not restarted by Docker's health check. Never touches BLAST.
async fn health(AxState(st): AxState<Arc<AppState>>, Query(q): Query<HealthQuery>) -> Response {
    let liveness = q
        .liveness
        .as_deref()
        .is_some_and(|v| v == "1" || v == "true");
    let (queued, running) = st.store.counts();
    let reason = if st.workers.iter().any(|w| w.is_finished()) {
        Some("a job worker has stopped".to_string())
    } else if let Err(e) = probe_binaries(&st.cfg.blast_bin_dir) {
        Some(format!("BLAST binaries unavailable: {e}"))
    } else if !st.store.state_writable() {
        Some("the state directory is not writable".to_string())
    } else if !liveness && queued >= st.cfg.max_queued {
        Some("the queue is full".to_string())
    } else {
        None
    };
    match reason {
        None => Json(json!({
            "status": "ok",
            "queued": queued,
            "running": running,
            "database_release": st.cfg.database_release,
        }))
        .into_response(),
        Some(reason) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status": "degraded", "reason": reason, "queued": queued, "running": running}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_bounds_on_char_boundaries() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip("ééééé", 2), "éé…");
    }

    #[test]
    fn poll_after_uses_free_slots() {
        // Two slots, none busy: positions 1 and 2 start now.
        assert_eq!(poll_after(1, 0, 2), POLL_SOON);
        assert_eq!(poll_after(2, 0, 2), POLL_SOON);
        assert_eq!(poll_after(3, 0, 2), POLL_LATER);
        // One busy: only position 1 starts now.
        assert_eq!(poll_after(1, 1, 2), POLL_SOON);
        assert_eq!(poll_after(2, 1, 2), POLL_LATER);
        // All busy (or more running than slots after a race).
        assert_eq!(poll_after(1, 2, 2), POLL_LATER);
        assert_eq!(poll_after(1, 5, 2), POLL_LATER);
    }

    #[test]
    fn probe_reports_missing_and_non_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        assert_eq!(probe_binaries(d.path()).unwrap_err(), "blastn is missing");
        for p in Program::ALL {
            let f = d.path().join(p.as_str());
            std::fs::write(&f, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        probe_binaries(d.path()).unwrap();
        let f = d.path().join("tblastx");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            probe_binaries(d.path()).unwrap_err(),
            "tblastx is not an executable file"
        );
    }
}
