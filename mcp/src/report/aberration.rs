use super::{
    ReportType, SectionCtx, feature_identity, paged, phenotype_section, props, references_section,
    rows_to_json, stocks_section, strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Map, Value, json};

pub struct Aberration;

pub const DEFAULT: &[&str] = &["identity", "breakpoints", "cytology", "class"];
pub const OPTIONAL: &[&str] = &[
    "genes_affected",
    "stocks",
    "phenotype_summary",
    "phenotypes",
    "references",
    "synonyms",
];

fn list(p: &Map<String, Value>, key: &str) -> Value {
    p.get(key).cloned().unwrap_or_else(|| json!([]))
}

#[async_trait::async_trait]
impl ReportType for Aberration {
    fn name(&self) -> &'static str {
        "aberration"
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
            "breakpoints" => {
                let p = props(
                    ctx,
                    &[
                        "derived_computed_breakpoint",
                        "derived_attributed_breakpoint",
                    ],
                )
                .await?;
                json!({"computed": list(&p, "derived_computed_breakpoint"), "attributed": list(&p, "derived_attributed_breakpoint")})
            }
            "cytology" => {
                let p = props(
                    ctx,
                    &[
                        "deleted_segment",
                        "duplicated_segment",
                        "new_order",
                        "cyto_loc_comment",
                    ],
                )
                .await?;
                json!({"deleted_segments": list(&p, "deleted_segment"), "duplicated_segments": list(&p, "duplicated_segment"), "new_order": list(&p, "new_order"), "comments": list(&p, "cyto_loc_comment")})
            }
            "class" => {
                let r = ctx.query("report/common/cvterms", &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"vocabulary": r.get::<_, String>("vocabulary"), "term": r.get::<_, String>("term"), "term_id": r.get::<_, Option<String>>("term_id")})
                ))
            }
            "genes_affected" => {
                let r = ctx
                    .query("report/aberration/genes", &[id, &ctx.fetch])
                    .await?;
                paged(
                    rows_to_json(
                        &r,
                        |r| json!({"relationship": r.get::<_, String>("relationship"), "id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s))}),
                    ),
                    ctx.limit,
                )
            }
            "stocks" => stocks_section(ctx).await?,
            "phenotype_summary" | "phenotypes" => phenotype_section(ctx, name).await?,
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown aberration section '{other}'"),
                });
            }
        })
    }
}
