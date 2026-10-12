use super::{
    ReportType, SectionCtx, paged, props_map, pub_row, rows_to_json, strip_markup, synonym_row,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

/// Cell lines, strains, and libraries share one table shape (record, props, pub, synonym, and for two of them
/// cvterm and feature link tables), so one type serves all three; `kind` selects the SQL directory.
pub struct Record {
    pub kind: &'static str,
}

pub const DEFAULT: &[&str] = &["identity", "properties", "references"];
const OPTIONAL_BASIC: &[&str] = &["synonyms"];
const OPTIONAL_LINKED: &[&str] = &["synonyms", "cvterms", "features"];

#[async_trait::async_trait]
impl ReportType for Record {
    fn name(&self) -> &'static str {
        self.kind
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        if self.kind == "strain" {
            OPTIONAL_BASIC
        } else {
            OPTIONAL_LINKED
        }
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let id = &ctx.id;
        let q = |section: &str| format!("report/{}/{section}", self.kind);
        Ok(match name {
            "identity" => {
                let r = ctx.query(&q("identity"), &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: ctx.tool,
                    message: format!("{id} not found"),
                })?;
                json!({"id": id, "name": r.get::<_, Option<String>>("name"), "species": r.get::<_, Option<String>>("species"), "type": r.get::<_, Option<String>>("type")})
            }
            "properties" => Value::Object(props_map(&ctx.query(&q("props"), &[id]).await?)),
            "references" => {
                let r = ctx.query(&q("references"), &[id, &ctx.fetch]).await?;
                paged(rows_to_json(&r, pub_row), ctx.limit)
            }
            "synonyms" => json!(rows_to_json(
                &ctx.query(&q("synonyms"), &[id]).await?,
                synonym_row
            )),
            "cvterms" if self.kind != "strain" => {
                let r = ctx.query(&q("cvterms"), &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"vocabulary": r.get::<_, String>("vocabulary"), "term": r.get::<_, String>("term"), "term_id": r.get::<_, String>("term_id")})
                ))
            }
            "features" if self.kind != "strain" => {
                let r = ctx.query(&q("features"), &[id, &ctx.fetch, &0i64]).await?;
                let mut v = paged(
                    rows_to_json(
                        &r,
                        |r| json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)), "type": r.get::<_, String>("type")}),
                    ),
                    ctx.limit,
                );
                if self.kind == "library" {
                    let n = ctx.query("report/library/feature_count", &[id]).await?;
                    v["total"] = json!(n.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0));
                }
                v
            }
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown {} section '{other}'", self.kind),
                });
            }
        })
    }
}
