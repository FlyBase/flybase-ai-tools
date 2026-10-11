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
    pub async fn light(&self) -> OwnedSemaphorePermit {
        self.light
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore open")
    }
    pub async fn heavy(&self) -> OwnedSemaphorePermit {
        self.heavy
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore open")
    }
    pub async fn blast(&self) -> OwnedSemaphorePermit {
        self.blast
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore open")
    }
    pub fn available(&self) -> (usize, usize, usize) {
        (
            self.light.available_permits(),
            self.heavy.available_permits(),
            self.blast.available_permits(),
        )
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
            held.push(l.heavy().await);
        }
        assert_eq!(l.available().1, 0);
        let got = tokio::time::timeout(Duration::from_millis(50), l.light()).await;
        assert!(
            got.is_ok(),
            "light permit must not block when heavy is exhausted"
        );
    }
    #[test]
    fn clamp() {
        assert_eq!(clamp_limit(None), 25);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(5000)), 100);
    }
}
