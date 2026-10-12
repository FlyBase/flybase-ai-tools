//! In-memory caches (spec 5.3): the lazily loaded RNA-seq matrix per dataset (this task), the bounded
//! response cache (Task 6), and the BLAST job-token table (Task 7). All are process-wide and shared by
//! every per-request server through `Caches`.
use crate::error::{ToolError, ToolResult};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

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

/// Everything the server caches in memory. Built once in `server::app`, shared by every request.
pub struct Caches {
    pub matrix: MatrixCache,
}

impl Caches {
    pub fn new() -> Self {
        Self {
            matrix: MatrixCache::new(),
        }
    }
}

impl Default for Caches {
    fn default() -> Self {
        Self::new()
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
}
