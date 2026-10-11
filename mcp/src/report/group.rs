use super::{ReportType, SectionCtx, paged, pub_row, rows_to_json, strip_markup, synonym_row};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Group;

pub const DEFAULT: &[&str] = &["identity", "members", "hierarchy"];
pub const OPTIONAL: &[&str] = &["cvterms", "references", "synonyms"];

#[async_trait::async_trait]
impl ReportType for Group {
    fn name(&self) -> &'static str {
        "gene_group"
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
            "identity" => {
                let r = ctx.query("report/group/identity", &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: ctx.tool,
                    message: format!("{id} not found"),
                })?;
                json!({
                    "id": id, "symbol": r.get::<_, Option<String>>("symbol"), "name": r.get::<_, Option<String>>("fullname"),
                    "type": r.get::<_, Option<String>>("type"), "description": r.get::<_, Option<String>>("description").map(|s| strip_markup(&s)),
                    "review_date": r.get::<_, Option<String>>("review_date"), "is_pathway": r.get::<_, Option<bool>>("is_pathway").unwrap_or(false),
                    "category": r.get::<_, Option<String>>("category"),
                })
            }
            "members" => {
                let r = ctx.query("report/group/members", &[id, &ctx.fetch]).await?;
                paged(
                    rows_to_json(&r, |r| {
                        json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol"), "membership": r.get::<_, String>("membership"),
                               "label": r.get::<_, Option<String>>("label").map(|s| strip_markup(&s))})
                    }),
                    ctx.limit,
                )
            }
            "hierarchy" => {
                let r = ctx.query("report/group/hierarchy", &[id]).await?;
                let row = |r: &tokio_postgres::Row| json!({"relationship": r.get::<_, String>("relationship"), "id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol"), "name": r.get::<_, Option<String>>("name")});
                let parents: Vec<Value> = r
                    .iter()
                    .filter(|r| r.get::<_, String>("direction") == "parent")
                    .map(row)
                    .collect();
                let children: Vec<Value> = r
                    .iter()
                    .filter(|r| r.get::<_, String>("direction") == "child")
                    .map(row)
                    .collect();
                json!({"parents": parents, "children": children})
            }
            "cvterms" => {
                let r = ctx.query("report/group/cvterms", &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"vocabulary": r.get::<_, String>("vocabulary"), "term": r.get::<_, String>("term"), "term_id": r.get::<_, String>("term_id")})
                ))
            }
            "references" => {
                let r = ctx
                    .query("report/group/references", &[id, &ctx.fetch])
                    .await?;
                paged(rows_to_json(&r, pub_row), ctx.limit)
            }
            "synonyms" => json!(rows_to_json(
                &ctx.query("report/group/synonyms", &[id]).await?,
                synonym_row
            )),
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown gene_group section '{other}'"),
                });
            }
        })
    }
}
