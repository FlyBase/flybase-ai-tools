use super::{
    ReportType, SectionCtx, feature_identity, feature_location, location_json, paged, props,
    references_section, rows_to_json, stock_count_section, stocks_section, strip_markup,
    synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Insertion;

pub const DEFAULT: &[&str] = &["identity", "location", "genes", "construct", "stock_count"];
pub const OPTIONAL: &[&str] = &["alleles", "stocks", "references", "synonyms"];
/// OPTIONAL plus the test-only probe, advertised only when FLYBASE_MCP_TEST_SECTIONS=1.
const OPTIONAL_WITH_PROBE: &[&str] = &[
    "alleles",
    "stocks",
    "references",
    "synonyms",
    "__timeout_probe",
];

#[async_trait::async_trait]
impl ReportType for Insertion {
    fn name(&self) -> &'static str {
        "insertion"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        if super::test_sections_enabled() {
            OPTIONAL_WITH_PROBE
        } else {
            OPTIONAL
        }
    }

    async fn location(&self, ctx: &SectionCtx) -> ToolResult<Option<(String, i64, i64)>> {
        Ok(feature_location(ctx).await?.map(|l| l.jbrowse()))
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let id = &ctx.id;
        Ok(match name {
            "identity" => {
                let mut v = feature_identity(ctx).await?;
                let p = props(
                    ctx,
                    &[
                        "derived_genomic_cyto_location",
                        "curated_cytological_location",
                    ],
                )
                .await?;
                let cyto = p
                    .get("derived_genomic_cyto_location")
                    .or_else(|| p.get("curated_cytological_location"))
                    .and_then(|a| a.get(0))
                    .cloned();
                v["cytology"] = cyto.unwrap_or(Value::Null);
                v
            }
            "location" => location_json(&feature_location(ctx).await?),
            "genes" => {
                let r = ctx.query("report/insertion/genes", &[id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({
                        "id": r.get::<_, String>("id"),
                        "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                    })
                }))
            }
            "construct" => {
                let r = ctx.query("report/insertion/construct", &[id]).await?;
                match r.first() {
                    None => Value::Null,
                    Some(r) => json!({
                        "id": r.get::<_, String>("id"),
                        "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                    }),
                }
            }
            "stock_count" => stock_count_section(ctx).await?,
            "alleles" => {
                let r = ctx
                    .query("report/insertion/alleles", &[id, &ctx.fetch])
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
            "__timeout_probe" if super::test_sections_enabled() => {
                super::timeout_probe(ctx).await?
            }
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown insertion section '{other}'"),
                });
            }
        })
    }
}
