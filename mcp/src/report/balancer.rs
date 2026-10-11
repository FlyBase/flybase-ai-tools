use super::{
    ReportType, SectionCtx, feature_identity, phenotype_section, references_section, rows_to_json,
    stock_count_section, stocks_section, strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Balancer;

pub const DEFAULT: &[&str] = &["identity", "components", "stock_count"];
pub const OPTIONAL: &[&str] = &["stocks", "phenotypes", "references", "synonyms"];

#[async_trait::async_trait]
impl ReportType for Balancer {
    fn name(&self) -> &'static str {
        "balancer"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        Ok(match name {
            "identity" => feature_identity(ctx).await?,
            "components" => {
                let r = ctx.query("report/balancer/components", &[&ctx.id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({"relationship": r.get::<_, String>("relationship"), "id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)), "type": r.get::<_, String>("type")})
                }))
            }
            "stock_count" => stock_count_section(ctx).await?,
            "stocks" => stocks_section(ctx).await?,
            "phenotypes" => phenotype_section(ctx, name).await?,
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown balancer section '{other}'"),
                });
            }
        })
    }
}
