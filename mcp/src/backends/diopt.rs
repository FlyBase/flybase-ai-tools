//! DIOPT: the release's Dmel-human ortholog file (migration 006 loader) and the detail block chado stores on
//! orthologous_to relationships for every non-Drosophila species.
use anyhow::Context;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use tokio_postgres::Client;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Detail {
    pub methods: Vec<String>,
    pub best_score: Option<bool>,
    pub best_score_rev: Option<bool>,
}

/// Parse `diopt methods:\tA, B\ndiopt best_score:\tYes\ndiopt best_score_rev:\tNo\n...`. Anything else (OrthoDB
/// pairs between Drosophila species, NULL) gives an empty Detail.
pub fn parse_detail(value: Option<&str>) -> Detail {
    let mut d = Detail::default();
    for line in value.unwrap_or("").lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match key.trim() {
            "diopt methods" => {
                d.methods = val
                    .split(',')
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty())
                    .collect()
            }
            "diopt best_score" => d.best_score = Some(val.eq_ignore_ascii_case("yes")),
            "diopt best_score_rev" => d.best_score_rev = Some(val.eq_ignore_ascii_case("yes")),
            _ => {}
        }
    }
    d
}

/// Replace mcp.diopt_orthologs with the rows of the release file (plain or gzip). Comment lines start with '#';
/// data lines have eight tab-separated columns; empty OMIM columns become NULL.
pub async fn load(client: &mut Client, path: &Path) -> anyhow::Result<u64> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader: Box<dyn Read> = if path.extension().is_some_and(|e| e == "gz") {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut rows: Vec<[Option<String>; 8]> = Vec::new();
    for (n, line) in BufReader::new(reader).lines().enumerate() {
        let line = line.with_context(|| format!("read line {}", n + 1))?;
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        anyhow::ensure!(
            cols.len() >= 6 && cols[0].starts_with("FBgn"),
            "line {}: expected 8 tab-separated columns starting with an FBgn, got {:?}",
            n + 1,
            line.chars().take(60).collect::<String>()
        );
        let col = |i: usize| {
            cols.get(i)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        rows.push([
            col(0),
            col(1),
            col(2),
            col(3),
            col(4),
            col(5),
            col(6),
            col(7),
        ]);
    }
    anyhow::ensure!(!rows.is_empty(), "no data rows in {}", path.display());
    let tx = client.transaction().await?;
    tx.batch_execute("truncate mcp.diopt_orthologs").await?;
    let stmt = tx
        .prepare("insert into mcp.diopt_orthologs (dmel_gene_id, dmel_symbol, hgnc_id, omim_gene_id, human_symbol, score, omim_phenotype_ids, omim_phenotypes) values ($1, $2, $3, $4, $5, $6, $7, $8) on conflict (dmel_gene_id, hgnc_id) do nothing")
        .await?;
    let mut n = 0u64;
    for r in &rows {
        let score: Option<i32> = r[5]
            .as_deref()
            .map(|s| s.parse())
            .transpose()
            .context("DIOPT_score is not an integer")?;
        n += tx
            .execute(
                &stmt,
                &[&r[0], &r[1], &r[2], &r[3], &r[4], &score, &r[6], &r[7]],
            )
            .await?;
    }
    tx.batch_execute("analyze mcp.diopt_orthologs").await?;
    tx.commit().await?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detail_block_parses() {
        let d = parse_detail(Some(
            "diopt methods:\torthoMCL, SonicParanoid, OrthoDB\ndiopt best_score:\tYes\ndiopt best_score_rev:\tNo\ndiopt proteinid1:\t270579\nFlyBase_geneid2:\tFBgn0004647",
        ));
        assert_eq!(d.methods, vec!["orthoMCL", "SonicParanoid", "OrthoDB"]);
        assert_eq!(d.best_score, Some(true));
        assert_eq!(d.best_score_rev, Some(false));
        assert_eq!(parse_detail(Some("EOG09190041")), Detail::default());
        assert_eq!(parse_detail(None), Detail::default());
    }
}
