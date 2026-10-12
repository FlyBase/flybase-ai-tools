use super::{
    ReportType, SectionCtx, external_links_section, feature_identity, feature_location,
    location_json, props_map, references_section, relationships_section, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::Value;

/// Report for feature prefixes with no dedicated page structure: clones, sequence features, natural transposons,
/// chemicals, ortholog genes, split system combinations, engineered plasmids. `name` is the class from
/// `tools::resolve::class_of`; `components` names the relationship section "components" (split system combinations
/// and plasmids, whose relationships are their parts) instead of "relationships".
pub struct GenericFeature {
    pub name: &'static str,
    pub components: bool,
}

pub const DEFAULT: &[&str] = &["identity", "location", "properties", "relationships"];
pub const DEFAULT_COMPONENTS: &[&str] = &["identity", "location", "properties", "components"];
pub const OPTIONAL: &[&str] = &["references", "synonyms", "external_links"];

#[async_trait::async_trait]
impl ReportType for GenericFeature {
    fn name(&self) -> &'static str {
        self.name
    }
    fn default_sections(&self) -> &'static [&'static str] {
        if self.components {
            DEFAULT_COMPONENTS
        } else {
            DEFAULT
        }
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    async fn location(&self, ctx: &SectionCtx) -> ToolResult<Option<(String, i64, i64)>> {
        Ok(feature_location(ctx).await?.map(|l| l.jbrowse()))
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        Ok(match name {
            "identity" => feature_identity(ctx).await?,
            "location" => location_json(&feature_location(ctx).await?),
            "properties" => Value::Object(props_map(
                &ctx.query("report/common/props_all", &[&ctx.id]).await?,
            )),
            "relationships" if !self.components => relationships_section(ctx).await?,
            "components" if self.components => relationships_section(ctx).await?,
            "references" => references_section(ctx).await?,
            "synonyms" => synonyms_section(ctx).await?,
            "external_links" => external_links_section(ctx).await?,
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown {} section '{other}'", self.name),
                });
            }
        })
    }
}
