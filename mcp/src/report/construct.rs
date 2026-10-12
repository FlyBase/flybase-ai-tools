use super::{
    ReportType, SectionCtx, feature_identity, paged, references_section, rows_to_json,
    stocks_section, strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_postgres::Row;

pub struct Construct;

pub const DEFAULT: &[&str] = &[
    "identity",
    "components",
    "regulatory_regions",
    "encoded_products",
];
pub const OPTIONAL: &[&str] = &["alleles", "insertions", "stocks", "references", "synonyms"];

/// Distinct genes from allele_genes rows for which `flag` is true, in symbol order.
fn genes_where(rows: &[Row], flag: &str) -> Vec<Value> {
    let mut genes: BTreeMap<String, String> = BTreeMap::new();
    for r in rows {
        if r.get::<_, Option<bool>>(flag).unwrap_or(false)
            && let Some(id) = r.get::<_, Option<String>>("gene_id")
        {
            genes.insert(
                id,
                r.get::<_, Option<String>>("gene_symbol")
                    .map(|s| strip_markup(&s))
                    .unwrap_or_default(),
            );
        }
    }
    genes
        .into_iter()
        .map(|(id, symbol)| json!({"id": id, "symbol": symbol}))
        .collect()
}

#[async_trait::async_trait]
impl ReportType for Construct {
    fn name(&self) -> &'static str {
        "construct"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let id = &ctx.id;
        Ok(match name {
            "identity" => feature_identity(ctx).await?,
            "components" => {
                let r = ctx.query("report/construct/components", &[id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({
                        "relationship": r.get::<_, String>("relationship"),
                        "id": r.get::<_, String>("id"),
                        "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                        "type": r.get::<_, String>("type"),
                    })
                }))
            }
            "regulatory_regions" | "encoded_products" => {
                let r = ctx.query("report/construct/allele_genes", &[id]).await?;
                let flag = if name == "regulatory_regions" {
                    "gene_is_regulatory_region"
                } else {
                    "is_alleleof"
                };
                json!(genes_where(&r, flag))
            }
            "alleles" => {
                let r = ctx.query("report/construct/allele_genes", &[id]).await?;
                // One item per allele. Rows come ordered is_alleleof first, so the gene shown is the one the allele belongs
                // to when there is one, else the regulatory-region gene (an allele with only that row is still listed).
                let mut seen = std::collections::HashSet::new();
                let items: Vec<Value> = r
                    .iter()
                    .filter(|r| seen.insert(r.get::<_, String>("allele_id")))
                    .map(|r| {
                        json!({
                            "id": r.get::<_, String>("allele_id"),
                            "symbol": r.get::<_, Option<String>>("allele_symbol").map(|s| strip_markup(&s)),
                            "gene_id": r.get::<_, Option<String>>("gene_id"),
                            "gene_symbol": r.get::<_, Option<String>>("gene_symbol").map(|s| strip_markup(&s)),
                        })
                    })
                    .take(ctx.fetch as usize)
                    .collect();
                paged(items, ctx.limit)
            }
            "insertions" => {
                let r = ctx
                    .query("report/construct/insertions", &[id, &ctx.fetch])
                    .await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({
                            "id": r.get::<_, String>("id"),
                            "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                        })
                    }),
                    ctx.limit,
                )
            }
            "stocks" => stocks_section(ctx).await?,
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown construct section '{other}'"),
                });
            }
        })
    }
}
