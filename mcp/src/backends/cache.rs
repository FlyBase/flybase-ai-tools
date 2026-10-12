//! In-memory caches (spec 5.3): the lazily loaded RNA-seq matrix per dataset, the bounded response
//! cache in front of every chado-backed tool, and the BLAST job-token table (Task 7). All are process-wide and shared by
//! every per-request server through `Caches`.
use crate::error::{ToolError, ToolResult};
use rmcp::model::CallToolResult;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::OnceCell;

pub const RESPONSE_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

pub struct MatrixRow {
    pub gene_id: String,
    pub vector: Vec<f32>,
}

/// Expression vectors for one dataset, transformed to log2(RPKM + 1) and z-scored per gene, genes sorted
/// by ID. Pearson correlation between two genes is the dot product of their z-scores over the sample count.
pub struct Matrix {
    pub dataset: String,
    pub n_samples: usize,
    pub genes: Vec<String>,
    z: Vec<f32>,
}

impl Matrix {
    /// Rows with the wrong length or no variance after the log transform are skipped.
    pub fn from_rows(dataset: &str, n_samples: usize, mut rows: Vec<MatrixRow>) -> Self {
        rows.sort_by(|a, b| a.gene_id.cmp(&b.gene_id));
        let mut genes = Vec::with_capacity(rows.len());
        let mut z = Vec::with_capacity(rows.len() * n_samples);
        for r in rows {
            if r.vector.len() != n_samples || n_samples == 0 {
                continue;
            }
            let v: Vec<f32> = r.vector.iter().map(|x| (x.max(0.0) + 1.0).log2()).collect();
            let n = n_samples as f32;
            let mean = v.iter().sum::<f32>() / n;
            let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n).sqrt();
            if sd.is_nan() || sd <= 1e-6 {
                continue;
            }
            genes.push(r.gene_id);
            z.extend(v.iter().map(|x| (x - mean) / sd));
        }
        Self {
            dataset: dataset.to_string(),
            n_samples,
            genes,
            z,
        }
    }

    pub fn index_of(&self, gene: &str) -> Option<usize> {
        self.genes.binary_search_by(|g| g.as_str().cmp(gene)).ok()
    }

    fn row(&self, i: usize) -> &[f32] {
        &self.z[i * self.n_samples..(i + 1) * self.n_samples]
    }

    /// Pearson r of gene `i` against every gene (including itself), in gene order.
    pub fn correlate(&self, i: usize) -> Vec<(usize, f32)> {
        let q = self.row(i);
        let n = self.n_samples as f32;
        (0..self.genes.len())
            .map(|j| {
                let r = q.iter().zip(self.row(j)).map(|(a, b)| a * b).sum::<f32>() / n;
                (j, r.clamp(-1.0, 1.0))
            })
            .collect()
    }

    pub fn bytes(&self) -> usize {
        self.z.len() * std::mem::size_of::<f32>()
            + self.genes.iter().map(|g| g.len() + 24).sum::<usize>()
    }
}

#[derive(Default)]
pub struct MatrixCache {
    cells: Mutex<HashMap<String, Arc<OnceCell<Arc<Matrix>>>>>,
}

impl MatrixCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The matrix for `dataset`, loading it once per process. Concurrent first callers share one load.
    pub async fn get_or_load<F, Fut>(&self, dataset: &str, load: F) -> ToolResult<Arc<Matrix>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ToolResult<Matrix>>,
    {
        // The std mutex guard is dropped at the end of this statement, before any await.
        let cell = self
            .cells
            .lock()
            .expect("matrix cache lock")
            .entry(dataset.to_string())
            .or_default()
            .clone();
        let m = cell
            .get_or_try_init(|| async {
                let m = load().await?;
                metrics::counter!("flybase_mcp_rnaseq_matrix_loads_total", "dataset" => m.dataset.clone()).increment(1);
                tracing::info!(dataset = %m.dataset, genes = m.genes.len(), bytes = m.bytes(), "rnaseq matrix loaded");
                Ok::<_, ToolError>(Arc::new(m))
            })
            .await?;
        Ok(m.clone())
    }

    /// (dataset, genes, bytes) for every loaded matrix, for /health.
    pub fn loaded(&self) -> Vec<(String, usize, usize)> {
        let cells = self.cells.lock().expect("matrix cache lock");
        let mut v: Vec<_> = cells
            .iter()
            .filter_map(|(k, c)| c.get().map(|m| (k.clone(), m.genes.len(), m.bytes())))
            .collect();
        v.sort();
        v
    }
}

/// Why a computed result was not stored: it was an error, or a success that must not be served again.
#[derive(Debug)]
enum NotStored {
    Failed(ToolError),
    Uncacheable(CallToolResult),
}

/// Finished tool results keyed by tool name and the caller's typed arguments (spec 5.3), bounded by
/// weighted bytes. Identical in-flight requests are coalesced by moka so the database sees one query.
pub struct ResponseCache {
    cache: moka::future::Cache<String, Arc<CallToolResult>>,
    enabled: bool,
}

/// Bytes a cached result holds: key, text block, and the structured JSON (weighted four times, since
/// serde_json::Value trees cost several times their serialized size in memory).
fn weight(key: &str, v: &CallToolResult) -> u32 {
    let text: usize = v
        .content
        .iter()
        .filter_map(|c| c.as_text())
        .map(|t| t.text.len())
        .sum();
    let json = v
        .structured_content
        .as_ref()
        .map(|s| s.to_string().len())
        .unwrap_or(0);
    (key.len() + text + 4 * json + 256).min(u32::MAX as usize) as u32
}

/// A report whose sections partly failed (timeouts, busy) must be retried next time, not replayed.
fn cacheable(r: &CallToolResult) -> bool {
    r.is_error != Some(true)
        && r.structured_content
            .as_ref()
            .and_then(|s| s.get("section_errors"))
            .and_then(|e| e.as_array())
            .is_none_or(|a| a.is_empty())
}

fn event(kind: &'static str) {
    metrics::counter!("flybase_mcp_cache_events_total", "cache" => "response", "event" => kind)
        .increment(1);
}

impl ResponseCache {
    /// `max_bytes == 0` disables storage: every call computes and nothing is inserted or counted.
    pub fn new(max_bytes: u64, ttl: Duration) -> Self {
        Self {
            cache: moka::future::Cache::builder()
                .name("response")
                .max_capacity(max_bytes.max(1))
                .weigher(|k: &String, v: &Arc<CallToolResult>| weight(k, v))
                .time_to_live(ttl)
                .build(),
            enabled: max_bytes > 0,
        }
    }

    /// Tool name plus the JSON of the typed parameters as the caller sent them (after serde defaults).
    /// Structs serialize fields in declaration order, so equal arguments give equal keys.
    pub fn key<P: Serialize>(tool: &'static str, p: &P) -> String {
        format!(
            "{tool}\n{}",
            serde_json::to_string(p).unwrap_or_else(|_| "?".into())
        )
    }

    /// Return the cached result for `key`, or run `compute` (once per key even under concurrent identical
    /// calls), store it when cacheable, and return it.
    pub async fn get_or_else(
        &self,
        key: String,
        compute: impl Future<Output = ToolResult<CallToolResult>>,
    ) -> ToolResult<CallToolResult> {
        if !self.enabled {
            return compute.await;
        }
        if let Some(hit) = self.cache.get(&key).await {
            event("hit");
            return Ok(CallToolResult::clone(&hit));
        }
        event("miss");
        let outcome = self
            .cache
            .try_get_with(key, async {
                match compute.await {
                    Ok(r) if cacheable(&r) => Ok(Arc::new(r)),
                    Ok(r) => Err(NotStored::Uncacheable(r)),
                    Err(e) => Err(NotStored::Failed(e)),
                }
            })
            .await;
        match outcome {
            Ok(stored) => Ok(CallToolResult::clone(&stored)),
            Err(not_stored) => match &*not_stored {
                NotStored::Uncacheable(r) => Ok(r.clone()),
                NotStored::Failed(e) => Err(e.clone()),
            },
        }
    }

    /// (entries, weighted bytes) after pending maintenance, for /health.
    pub async fn stats(&self) -> (u64, u64) {
        self.cache.run_pending_tasks().await;
        (self.cache.entry_count(), self.cache.weighted_size())
    }
}

/// Everything the server caches in memory. Built once in `server::app`, shared by every request.
pub struct Caches {
    pub response: ResponseCache,
    pub matrix: MatrixCache,
    /// BLAST job_id -> token hash; the only state the BLAST tools keep.
    pub blast_tokens: crate::backends::blast::JobTokens,
}

impl Caches {
    /// `response_bytes` is the response cache bound (0 disables it).
    pub fn new(response_bytes: u64) -> Self {
        Self {
            response: ResponseCache::new(response_bytes, RESPONSE_CACHE_TTL),
            matrix: MatrixCache::new(),
            blast_tokens: crate::backends::blast::JobTokens::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raw RPKM values chosen as 2^k - 1 so the log2(x + 1) transform gives the integers k.
    fn row(id: &str, logs: &[f32]) -> MatrixRow {
        MatrixRow {
            gene_id: id.into(),
            vector: logs.iter().map(|k| 2f32.powf(*k) - 1.0).collect(),
        }
    }

    #[test]
    fn pearson_matches_hand_values() {
        let m = Matrix::from_rows(
            "t",
            4,
            vec![
                row("D", &[1.0, 3.0, 2.0, 4.0]),
                row("A", &[1.0, 2.0, 3.0, 4.0]),
                row("B", &[2.0, 4.0, 6.0, 8.0]),
                row("C", &[4.0, 3.0, 2.0, 1.0]),
            ],
        );
        assert_eq!(m.genes, vec!["A", "B", "C", "D"]);
        let a = m.index_of("A").unwrap();
        let r: Vec<f32> = m.correlate(a).into_iter().map(|(_, r)| r).collect();
        assert!((r[0] - 1.0).abs() < 1e-4);
        assert!((r[1] - 1.0).abs() < 1e-4, "scaled copy correlates 1");
        assert!((r[2] + 1.0).abs() < 1e-4, "reversed correlates -1");
        assert!((r[3] - 0.8).abs() < 1e-4, "D vs A is 0.8");
        assert!(m.index_of("Z").is_none());
        assert_eq!(m.bytes(), 16 * 4 + 4 * 25);
    }

    /// Review focus 3: flat and malformed rows never enter the matrix.
    #[test]
    fn flat_rows_are_excluded() {
        let m = Matrix::from_rows(
            "t",
            3,
            vec![
                MatrixRow {
                    gene_id: "flat".into(),
                    vector: vec![5.0, 5.0, 5.0],
                },
                row("ok", &[1.0, 2.0, 3.0]),
                row("short", &[1.0, 2.0]),
            ],
        );
        assert_eq!(m.genes, vec!["ok"]);
        assert!(m.index_of("flat").is_none());
    }

    #[tokio::test]
    async fn matrix_loads_once() {
        let c = MatrixCache::new();
        let loads = std::sync::atomic::AtomicUsize::new(0);
        for _ in 0..3 {
            let m = c
                .get_or_load("t", || async {
                    loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(Matrix::from_rows("t", 2, vec![row("a", &[1.0, 2.0])]))
                })
                .await
                .unwrap();
            assert_eq!(m.genes.len(), 1);
        }
        assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(c.loaded(), vec![("t".to_string(), 1, 8 + 25)]);
    }

    use rmcp::model::ContentBlock;

    fn ok_result(json: serde_json::Value) -> CallToolResult {
        let mut r = CallToolResult::structured(json);
        r.content = vec![ContentBlock::text("hello")];
        r
    }

    #[tokio::test]
    async fn response_cache_stores_ok_only_and_counts_bytes() {
        let c = ResponseCache::new(1024 * 1024, Duration::from_secs(60));
        let key = ResponseCache::key("t", &serde_json::json!({"a": 1}));
        let err: ToolResult<CallToolResult> = Err(ToolError::NotFound {
            tool: "t",
            message: "x".into(),
        });
        assert_eq!(
            c.get_or_else(key.clone(), async { err })
                .await
                .unwrap_err()
                .code(),
            "not_found"
        );
        assert_eq!(c.stats().await.0, 0);
        let ok = ok_result(serde_json::json!({"n": 1}));
        let calls = std::sync::atomic::AtomicUsize::new(0);
        for _ in 0..2 {
            let r = c
                .get_or_else(key.clone(), async {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(ok.clone())
                })
                .await
                .unwrap();
            assert_eq!(r.structured_content, ok.structured_content);
        }
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "second call served from cache"
        );
        let (entries, bytes) = c.stats().await;
        assert_eq!(entries, 1);
        assert!(bytes > 256 && bytes < 1024, "{bytes}");
        assert_ne!(ResponseCache::key("t", &serde_json::json!({"a": 2})), key);
    }

    /// A report with section_errors is returned but never stored, so the next call retries the sections.
    #[tokio::test]
    async fn partial_reports_are_not_stored() {
        let c = ResponseCache::new(1024 * 1024, Duration::from_secs(60));
        let key = ResponseCache::key("get_report", &serde_json::json!({"id": "FBgn0004647"}));
        let partial = ok_result(
            serde_json::json!({"sections": {"identity": {}}, "section_errors": ["__timeout_probe"]}),
        );
        let calls = std::sync::atomic::AtomicUsize::new(0);
        for _ in 0..2 {
            let r = c
                .get_or_else(key.clone(), async {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(partial.clone())
                })
                .await
                .unwrap();
            assert_eq!(r.structured_content, partial.structured_content);
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(c.stats().await.0, 0);
        let clean = ok_result(serde_json::json!({"sections": {}, "section_errors": []}));
        c.get_or_else(key.clone(), async { Ok(clean) })
            .await
            .unwrap();
        assert_eq!(c.stats().await.0, 1);
    }

    #[tokio::test]
    async fn disabled_cache_never_stores() {
        let c = ResponseCache::new(0, Duration::from_secs(60));
        let key = ResponseCache::key("t", &1);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        for _ in 0..2 {
            c.get_or_else(key.clone(), async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ok_result(serde_json::json!(1)))
            })
            .await
            .unwrap();
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(c.stats().await, (0, 0));
    }

    /// Concurrent identical calls run the computation once.
    #[tokio::test]
    async fn identical_in_flight_calls_are_coalesced() {
        let c = Arc::new(ResponseCache::new(1024 * 1024, Duration::from_secs(60)));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let key = ResponseCache::key("t", &"same");
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let (c, calls, key) = (c.clone(), calls.clone(), key.clone());
            tasks.push(tokio::spawn(async move {
                c.get_or_else(key, async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(ok_result(serde_json::json!({"v": 1})))
                })
                .await
                .unwrap()
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
