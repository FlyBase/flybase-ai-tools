use super::{ReportType, SectionCtx, paged, rows_to_json};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Reference;

pub const DEFAULT: &[&str] = &["citation", "authors", "abstract"];
pub const OPTIONAL: &[&str] = &["genes", "alleles", "data_types"];

#[async_trait::async_trait]
impl ReportType for Reference {
    fn name(&self) -> &'static str {
        "reference"
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
            "citation" => {
                let r = ctx.query("report/reference/citation", &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{id} not found"),
                })?;
                json!({
                    "id": id,
                    "title": r.get::<_, Option<String>>("title"),
                    "year": r.get::<_, Option<String>>("year"),
                    "miniref": r.get::<_, Option<String>>("miniref"),
                    "volume": r.get::<_, Option<String>>("volume"),
                    "issue": r.get::<_, Option<String>>("issue"),
                    "pages": r.get::<_, Option<String>>("pages"),
                    "type": r.get::<_, String>("type"),
                    "pubmed_id": r.get::<_, Option<String>>("pubmed_id"),
                    "doi": r.get::<_, Option<String>>("doi"),
                })
            }
            "authors" => {
                let r = ctx.query("report/reference/authors", &[id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({
                        "rank": r.get::<_, i32>("rank"),
                        "surname": r.get::<_, Option<String>>("surname"),
                        "given_names": r.get::<_, Option<String>>("givennames"),
                    })
                }))
            }
            "abstract" => {
                let r = ctx.query("report/reference/abstract", &[id]).await?;
                json!({"text": r.first().map(|r| r.get::<_, String>("value"))})
            }
            "genes" | "alleles" => {
                let q = if name == "genes" {
                    "report/reference/genes"
                } else {
                    "report/reference/alleles"
                };
                let r = ctx.query(q, &[id, &ctx.fetch]).await?;
                paged(
                    rows_to_json(
                        &r,
                        |r| json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol")}),
                    ),
                    ctx.limit,
                )
            }
            "data_types" => {
                let r = ctx.query("report/reference/data_types", &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"type": r.get::<_, String>("name"), "count": r.get::<_, i64>("n")})
                ))
            }
            other => {
                return Err(ToolError::InvalidInput {
                    tool: t,
                    arg: "sections",
                    message: format!("unknown reference section '{other}'"),
                });
            }
        })
    }
}
