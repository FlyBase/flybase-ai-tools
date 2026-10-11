use super::{ReportType, SectionCtx, rows_to_json, strip_markup};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Stock;

/// No `references` section: `stock_pub` is empty in public chado.
pub const DEFAULT: &[&str] = &["identity", "components"];
pub const OPTIONAL: &[&str] = &[];

#[async_trait::async_trait]
impl ReportType for Stock {
    fn name(&self) -> &'static str {
        "stock"
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
                let r = ctx.query("report/stock/identity", &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{id} not found"),
                })?;
                json!({
                    "id": id,
                    "stock_number": r.get::<_, Option<String>>("stock_number"),
                    "provider": r.get::<_, Option<String>>("provider"),
                    "genotype": r.get::<_, Option<String>>("genotype").map(|s| strip_markup(&s)),
                    "description": r.get::<_, Option<String>>("description"),
                    "stock_type": r.get::<_, String>("stock_type"),
                })
            }
            "components" => {
                let r = ctx.query("report/stock/components", &[id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({
                        "id": r.get::<_, String>("id"),
                        "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                        "type": r.get::<_, String>("type"),
                    })
                }))
            }
            other => {
                return Err(ToolError::InvalidInput {
                    tool: t,
                    arg: "sections",
                    message: format!("unknown stock section '{other}'"),
                });
            }
        })
    }
}
