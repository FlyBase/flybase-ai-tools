use super::{
    ReportType, SectionCtx, paged, references_section, rows_to_json, strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub struct Gene;

pub const DEFAULT: &[&str] = &[
    "identity",
    "location",
    "snapshot",
    "go_summary",
    "ortholog_count",
    "phenotype_summary",
    "stock_count",
];
pub const OPTIONAL: &[&str] = &[
    "alleles",
    "transcripts",
    "go_full",
    "synonyms",
    "references",
    "external_links",
];

#[async_trait::async_trait]
impl ReportType for Gene {
    fn name(&self) -> &'static str {
        "gene"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    async fn location(&self, ctx: &SectionCtx) -> ToolResult<Option<(String, i64, i64)>> {
        let rows = ctx.query("report/gene/location", &[&ctx.id]).await?;
        // fmin and fmax are nullable: a gene placed on an arm without coordinates has no span.
        Ok(rows.first().and_then(|r| {
            Some((
                r.get::<_, String>("chromosome"),
                i64::from(r.get::<_, Option<i32>>("fmin")?),
                i64::from(r.get::<_, Option<i32>>("fmax")?),
            ))
        }))
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let id = &ctx.id;
        let t = ctx.tool;
        Ok(match name {
            "identity" => {
                let r = ctx.query("report/gene/identity", &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{id} not found"),
                })?;
                json!({
                    "id": id,
                    "symbol": r.get::<_, Option<String>>("symbol"),
                    "name": r.get::<_, Option<String>>("fullname"),
                    "species": r.get::<_, Option<String>>("species"),
                    "gene_type": r.get::<_, Option<String>>("gene_type").map(|s| strip_markup(&s)),
                    "annotation_id": r.get::<_, Option<String>>("annotation_id"),
                    "cytology": r.get::<_, Option<String>>("cytology").map(|s| s.split(';').next().unwrap_or("").trim().to_string()),
                })
            }
            "location" => {
                let r = ctx.query("report/gene/location", &[id]).await?;
                match r.first() {
                    None => json!({"located": false}),
                    Some(r) => match (
                        r.get::<_, Option<i32>>("fmin"),
                        r.get::<_, Option<i32>>("fmax"),
                    ) {
                        (Some(fmin), Some(fmax)) => json!({
                            "located": true,
                            "chromosome": r.get::<_, String>("chromosome"),
                            "start": fmin + 1,
                            "end": fmax,
                            "strand": r.get::<_, Option<i16>>("strand").unwrap_or(0),
                        }),
                        _ => json!({"located": false}),
                    },
                }
            }
            "snapshot" => {
                let r = ctx.query("report/gene/snapshot", &[id]).await?;
                json!({
                    "text": r.first().and_then(|r| r.get::<_, Option<String>>("value")).map(|s| strip_markup(&s)),
                    "date": r.first().and_then(|r| r.get::<_, Option<String>>("date")),
                })
            }
            "go_summary" | "go_full" => {
                let r = ctx.query("report/gene/go", &[id]).await?;
                let mut by_aspect: Map<String, Value> = Map::new();
                for aspect in [
                    "biological_process",
                    "molecular_function",
                    "cellular_component",
                ] {
                    by_aspect.insert(aspect.into(), json!([]));
                }
                for row in &r {
                    let aspect: String = row.get("aspect");
                    let entry = json!({
                        "term_id": format!("GO:{}", row.get::<_, String>("accession")),
                        "term": row.get::<_, String>("term"),
                        "evidence": row.get::<_, Option<String>>("evidence"),
                        "qualifier": row.get::<_, Option<String>>("qualifier"),
                        "is_experimental": row.get::<_, bool>("is_experimental"),
                    });
                    if let Some(Value::Array(a)) = by_aspect.get_mut(&aspect) {
                        a.push(entry);
                    }
                }
                if name == "go_summary" {
                    for v in by_aspect.values_mut() {
                        if let Value::Array(a) = v {
                            let mut seen = HashSet::new();
                            a.retain(|e| {
                                seen.insert(e["term_id"].as_str().unwrap_or("").to_string())
                            });
                            a.truncate(ctx.limit as usize);
                        }
                    }
                }
                Value::Object(by_aspect)
            }
            "ortholog_count" => {
                let r = ctx.query("report/gene/ortholog_count", &[id]).await?;
                json!(r.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0))
            }
            "phenotype_summary" => {
                let r = ctx
                    .query("report/gene/phenotype_summary", &[id, &ctx.fetch])
                    .await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({
                            "phenotype": r.get::<_, String>("term"),
                            "term_id": r.get::<_, Option<String>>("term_id"),
                            "allele_count": r.get::<_, i64>("allele_count"),
                        })
                    }),
                    ctx.limit,
                )
            }
            "stock_count" => {
                let r = ctx.query("report/gene/stock_count", &[id]).await?;
                json!(r.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0))
            }
            "alleles" => {
                let r = ctx.query("report/gene/alleles", &[id, &ctx.fetch]).await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({
                            "id": r.get::<_, Option<String>>("fbal_id"),
                            "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                            "is_construct": r.get::<_, Option<bool>>("is_construct"),
                            "stocks": r.get::<_, Option<i64>>("stocks_count"),
                            "papers": r.get::<_, Option<i64>>("paper_count"),
                            "classes": r.get::<_, Option<String>>("classes"),
                        })
                    }),
                    ctx.limit,
                )
            }
            "transcripts" => {
                let r = ctx
                    .query("report/gene/transcripts", &[id, &ctx.fetch])
                    .await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({
                            "id": r.get::<_, String>("uniquename"),
                            "symbol": r.get::<_, Option<String>>("name"),
                            "type": r.get::<_, String>("type"),
                            "length": r.get::<_, Option<i32>>("seqlen"),
                            "polypeptide": r.get::<_, Option<String>>("polypeptide"),
                        })
                    }),
                    ctx.limit,
                )
            }
            "synonyms" => synonyms_section(ctx).await?,
            "references" => references_section(ctx).await?,
            "external_links" => {
                let r = ctx.query("report/gene/external_links", &[id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({
                        "database": r.get::<_, String>("db"),
                        "accession": r.get::<_, String>("accession"),
                        "url": r.get::<_, Option<String>>("url"),
                    })
                }))
            }
            other => {
                return Err(ToolError::InvalidInput {
                    tool: t,
                    arg: "sections",
                    message: format!("unknown gene section '{other}'"),
                });
            }
        })
    }
}
