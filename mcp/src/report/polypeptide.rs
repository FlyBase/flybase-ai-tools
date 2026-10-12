use super::{
    ReportType, SectionCtx, external_links_section, feature_identity, paged, props,
    references_section, rows_to_json, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Polypeptide;

pub const DEFAULT: &[&str] = &["identity", "gene", "transcript", "length", "domains"];
pub const OPTIONAL: &[&str] = &["external_links", "references", "synonyms"];

#[async_trait::async_trait]
impl ReportType for Polypeptide {
    fn name(&self) -> &'static str {
        "polypeptide"
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
                let mut v = feature_identity(ctx).await?;
                let p = props(
                    ctx,
                    &["derived_molecular_weight", "derived_isoelectric_point"],
                )
                .await?;
                v["molecular_weight"] = p
                    .get("derived_molecular_weight")
                    .and_then(|a| a.get(0))
                    .cloned()
                    .unwrap_or(Value::Null);
                v["isoelectric_point"] = p
                    .get("derived_isoelectric_point")
                    .and_then(|a| a.get(0))
                    .cloned()
                    .unwrap_or(Value::Null);
                v
            }
            "gene" => super::transcript::gene_section(ctx).await?,
            "transcript" => {
                let r = ctx.query("report/polypeptide/transcript", &[id]).await?;
                match r.first() {
                    None => Value::Null,
                    Some(r) => {
                        json!({"id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol"), "type": r.get::<_, String>("type"), "length": r.get::<_, Option<i32>>("length")})
                    }
                }
            }
            "length" => feature_identity(ctx).await?["length"].clone(),
            "domains" => {
                let r = ctx
                    .query("report/polypeptide/domains", &[id, &ctx.fetch])
                    .await?;
                paged(
                    rows_to_json(
                        &r,
                        |r| json!({"name": r.get::<_, Option<String>>("name"), "start": r.get::<_, Option<i32>>("fmin").map(|x| x + 1), "end": r.get::<_, Option<i32>>("fmax"), "accessions": r.get::<_, Option<String>>("accessions")}),
                    ),
                    ctx.limit,
                )
            }
            "external_links" => external_links_section(ctx).await?,
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown polypeptide section '{other}'"),
                });
            }
        })
    }
}
