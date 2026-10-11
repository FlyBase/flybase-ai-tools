use crate::error::{ToolError, ToolResult};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const LIGHT_PERMITS: usize = 24;
pub const HEAVY_PERMITS: usize = 6;
pub const BLAST_PERMITS: usize = 2;
pub const MAX_JSON_BYTES: usize = 256 * 1024;
pub const MAX_FASTA_BYTES: usize = 512 * 1024;
pub const MAX_ROWS: i64 = 1000;
pub const DEFAULT_LIMIT: u32 = 25;
pub const MAX_LIMIT: u32 = 100;
pub const MAX_BATCH: usize = 500;
pub const POOL_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Limits {
    light: Arc<Semaphore>,
    heavy: Arc<Semaphore>,
    blast: Arc<Semaphore>,
}

impl Limits {
    pub fn new() -> Self {
        Self {
            light: Arc::new(Semaphore::new(LIGHT_PERMITS)),
            heavy: Arc::new(Semaphore::new(HEAVY_PERMITS)),
            blast: Arc::new(Semaphore::new(BLAST_PERMITS)),
        }
    }
    pub async fn light(&self, tool: &'static str) -> ToolResult<OwnedSemaphorePermit> {
        acquire(&self.light, tool).await
    }
    pub async fn heavy(&self, tool: &'static str) -> ToolResult<OwnedSemaphorePermit> {
        acquire(&self.heavy, tool).await
    }
    pub async fn blast(&self, tool: &'static str) -> ToolResult<OwnedSemaphorePermit> {
        acquire(&self.blast, tool).await
    }
    pub fn available(&self) -> (usize, usize, usize) {
        (
            self.light.available_permits(),
            self.heavy.available_permits(),
            self.blast.available_permits(),
        )
    }
}

/// Wait at most POOL_WAIT for a permit; a saturated server answers busy instead of queueing forever.
async fn acquire(sem: &Arc<Semaphore>, tool: &'static str) -> ToolResult<OwnedSemaphorePermit> {
    match tokio::time::timeout(POOL_WAIT, sem.clone().acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(_)) | Err(_) => {
            tracing::warn!(tool, "no permit within {:?}", POOL_WAIT);
            Err(ToolError::BackendUnavailable {
                tool,
                message: "server busy; try again shortly".into(),
            })
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

/// Clamp a caller-supplied limit into [1, MAX_LIMIT], defaulting when absent.
pub fn clamp_limit(limit: Option<u32>) -> i64 {
    i64::from(limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn light_permits_independent_of_heavy() {
        let l = Limits::new();
        let mut held = Vec::new();
        for _ in 0..HEAVY_PERMITS {
            held.push(l.heavy("t").await.unwrap());
        }
        assert_eq!(l.available().1, 0);
        let got = tokio::time::timeout(Duration::from_millis(50), l.light("t")).await;
        assert!(
            got.is_ok_and(|p| p.is_ok()),
            "light permit must not block when heavy is exhausted"
        );
    }
    #[tokio::test(start_paused = true)]
    async fn exhausted_semaphore_returns_busy_after_pool_wait() {
        let l = Limits::new();
        let mut held = Vec::new();
        for _ in 0..BLAST_PERMITS {
            held.push(l.blast("blast_submit").await.unwrap());
        }
        let started = tokio::time::Instant::now();
        let e = l.blast("blast_submit").await.unwrap_err();
        assert_eq!(e.code(), "backend_unavailable");
        assert!(e.to_string().contains("server busy"));
        let waited = started.elapsed();
        assert!(
            waited >= POOL_WAIT && waited < POOL_WAIT + Duration::from_secs(1),
            "waited {waited:?}"
        );
        drop(held);
        assert!(l.blast("blast_submit").await.is_ok());
    }
    #[test]
    fn clamp() {
        assert_eq!(clamp_limit(None), 25);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(5000)), 100);
    }
}
