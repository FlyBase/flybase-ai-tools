use super::{
    ReportType, SectionCtx, feature_identity, feature_location, location_json, references_section,
    strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};
use tokio_postgres::Row;

pub struct Transcript;

pub const DEFAULT: &[&str] = &["identity", "gene", "location", "exons", "cds"];
pub const OPTIONAL: &[&str] = &["polypeptide", "utrs", "introns", "references", "synonyms"];

/// `{id, symbol}` of the gene a transcript or polypeptide belongs to (report/transcript/gene.sql handles both).
pub async fn gene_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx.query("report/transcript/gene", &[&ctx.id]).await?;
    Ok(match r.first() {
        None => Value::Null,
        Some(r) => {
            json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol")})
        }
    })
}

/// Located parts of one SO type as `[{name, start, end, strand}]` in genomic order (start 1-based).
fn parts_of(rows: &[Row], kind: &str) -> Vec<Value> {
    let mut v: Vec<(i32, Value)> = rows
        .iter()
        .filter(|r| r.get::<_, String>("type") == kind)
        .filter_map(|r| {
            let fmin = r.get::<_, Option<i32>>("fmin")?;
            let fmax = r.get::<_, Option<i32>>("fmax")?;
            Some((fmin, json!({"name": r.get::<_, Option<String>>("name"), "start": fmin + 1, "end": fmax, "strand": r.get::<_, Option<i16>>("strand").unwrap_or(0)})))
        })
        .collect();
    v.sort_by_key(|(fmin, _)| *fmin);
    v.into_iter().map(|(_, j)| j).collect()
}

#[async_trait::async_trait]
impl ReportType for Transcript {
    fn name(&self) -> &'static str {
        "transcript"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    async fn location(&self, ctx: &SectionCtx) -> ToolResult<Option<(String, i64, i64)>> {
        Ok(feature_location(ctx).await?.map(|l| l.jbrowse()))
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let id = &ctx.id;
        Ok(match name {
            "identity" => feature_identity(ctx).await?,
            "gene" => gene_section(ctx).await?,
            "location" => location_json(&feature_location(ctx).await?),
            "exons" => json!(parts_of(
                &ctx.query("report/transcript/parts", &[id]).await?,
                "exon"
            )),
            "cds" => {
                let segs = parts_of(&ctx.query("report/transcript/parts", &[id]).await?, "CDS");
                let length: i64 = segs
                    .iter()
                    .map(|s| s["end"].as_i64().unwrap_or(0) - s["start"].as_i64().unwrap_or(0) + 1)
                    .sum();
                json!({"segments": segs, "length": length})
            }
            "utrs" => {
                let r = ctx.query("report/transcript/parts", &[id]).await?;
                json!({"five_prime": parts_of(&r, "five_prime_UTR"), "three_prime": parts_of(&r, "three_prime_UTR")})
            }
            "introns" => json!(parts_of(
                &ctx.query("report/transcript/parts", &[id]).await?,
                "intron"
            )),
            "polypeptide" => {
                let r = ctx.query("report/transcript/polypeptide", &[id]).await?;
                match r.first() {
                    None => Value::Null,
                    Some(r) => {
                        json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)), "length": r.get::<_, Option<i32>>("length")})
                    }
                }
            }
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown transcript section '{other}'"),
                });
            }
        })
    }
}
