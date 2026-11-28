use super::{
    ReportType, SectionCtx, external_links_section, feature_identity, paged, reference_page,
    relationships_section, rows_to_json, strip_markup, synonyms_section,
};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct MolecularSegment;

#[async_trait::async_trait]
impl ReportType for MolecularSegment {
    fn name(&self) -> &'static str {
        "molecular_segment"
    }

    fn default_sections(&self) -> &'static [&'static str] {
        &[
            "identity",
            "molecular_details",
            "sequence_extents",
            "segments",
        ]
    }

    fn optional_sections(&self) -> &'static [&'static str] {
        &["relationships", "references", "synonyms", "external_links"]
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let args: &[&(dyn tokio_postgres::types::ToSql + Sync)] = &[&ctx.id, &ctx.fetch];
        let items = match name {
            "identity" => return feature_identity(ctx).await,
            "references" => return reference_page(ctx, "report/segment/references").await,
            "relationships" => return relationships_section(ctx).await,
            "synonyms" => return synonyms_section(ctx).await,
            "external_links" => return external_links_section(ctx).await,
            "molecular_details" => {
                rows_to_json(&ctx.query("report/segment/details", args).await?, |r| {
                    let kind: String = r.get("kind");
                    let value = super::curated_text::curated_text(r.get("value"));
                    let mut detail = json!({
                        "kind": kind,
                        "value": value,
                        "references": r.get::<_, Vec<String>>("references"),
                    });
                    if kind == "has_sequence_data" && value == ctx.id {
                        detail["url"] = json!(format!(
                            "https://flybase.org/api/sequence/id/{value}/compiled"
                        ));
                    }
                    detail
                })
            }
            "sequence_extents" => {
                rows_to_json(&ctx.query("report/segment/extents", args).await?, |r| {
                    let accession: String = r.get("accession");
                    let query = url::form_urlencoded::Serializer::new(String::new())
                        .append_pair("db", "nucleotide")
                        .append_pair("tool", "FlyBase")
                        .append_pair("val", &accession)
                        .finish();
                    json!({
                        "accession": accession,
                        "url": format!("https://www.ncbi.nlm.nih.gov/entrez/viewer.fcgi?{query}"),
                        "start": r.get::<_, Option<i32>>("start"),
                        "end": r.get::<_, Option<i32>>("end_pos"),
                        "strand": r.get::<_, Option<i16>>("strand"),
                    })
                })
            }
            "segments" => rows_to_json(&ctx.query("report/segment/components", args).await?, |r| {
                json!({
                    "id": r.get::<_, String>("id"),
                    "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                    "rank": r.get::<_, i32>("rank"),
                    "orientations": r.get::<_, Vec<String>>("orientations"),
                    "references": r.get::<_, Vec<String>>("references"),
                })
            }),
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown molecular segment section '{other}'"),
                });
            }
        };
        Ok(paged(items, ctx.limit))
    }
}
