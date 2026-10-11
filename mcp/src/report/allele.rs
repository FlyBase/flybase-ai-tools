use super::{
    ReportType, SectionCtx, paged, phenotype_section, references_section, rows_to_json,
    strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Allele;

pub const DEFAULT: &[&str] = &[
    "identity",
    "gene",
    "classes",
    "mutagens",
    "phenotype_summary",
    "stock_count",
];
pub const OPTIONAL: &[&str] = &[
    "phenotypes",
    "transgenic_product_class",
    "insertions",
    "constructs",
    "stocks",
    "references",
    "synonyms",
];

#[async_trait::async_trait]
impl ReportType for Allele {
    fn name(&self) -> &'static str {
        "allele"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let id = &ctx.id;
        let t = ctx.tool;
        Ok(match name {
            "identity" => {
                let r = ctx.query("report/allele/identity", &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{id} not found"),
                })?;
                json!({
                    "id": id,
                    "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                    "name": r.get::<_, Option<String>>("fullname").map(|s| strip_markup(&s)),
                    "species": r.get::<_, Option<String>>("species"),
                    "is_construct": r.get::<_, Option<bool>>("is_construct"),
                    "known_lesion": r.get::<_, Option<bool>>("known_lesion"),
                    "stock_count": r.get::<_, Option<i64>>("stocks_count"),
                    "paper_count": r.get::<_, Option<i64>>("paper_count"),
                })
            }
            "gene" => {
                let r = ctx.query("report/allele/gene", &[id]).await?;
                match r.first() {
                    None => Value::Null,
                    Some(r) => {
                        json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol")})
                    }
                }
            }
            "classes" | "mutagens" => {
                let q = if name == "classes" {
                    "report/allele/classes"
                } else {
                    "report/allele/mutagens"
                };
                let r = ctx.query(q, &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"name": r.get::<_, Option<String>>("name"), "term_id": r.get::<_, Option<String>>("fbcv_id")})
                ))
            }
            "phenotype_summary" | "phenotypes" => phenotype_section(ctx, name).await?,
            "transgenic_product_class" => {
                let r = ctx
                    .query("report/allele/transgenic_product_class", &[id])
                    .await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"name": r.get::<_, Option<String>>("name"), "term_id": r.get::<_, Option<String>>("so_id")})
                ))
            }
            "insertions" | "constructs" => {
                let q = if name == "insertions" {
                    "report/allele/insertions"
                } else {
                    "report/allele/constructs"
                };
                let r = ctx.query(q, &[id, &ctx.fetch]).await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({
                            "id": r.get::<_, Option<String>>("id"),
                            "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                        })
                    }),
                    ctx.limit,
                )
            }
            "stocks" => {
                let r = ctx.query("report/allele/stocks", &[id, &ctx.fetch]).await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({
                            "id": r.get::<_, Option<String>>("id"),
                            "provider": r.get::<_, Option<String>>("provider"),
                            "stock_number": r.get::<_, Option<String>>("stock_number"),
                            "genotype": r.get::<_, Option<String>>("genotype").map(|s| strip_markup(&s)),
                        })
                    }),
                    ctx.limit,
                )
            }
            "stock_count" => {
                let r = ctx.query("report/allele/stock_count", &[id]).await?;
                json!(r.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0))
            }
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: t,
                    arg: "sections",
                    message: format!("unknown allele section '{other}'"),
                });
            }
        })
    }
}
