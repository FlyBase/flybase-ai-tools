use super::{
    ReportType, SectionCtx, curated_text::curated_text, external_links_section, feature_identity,
    offset_page, paged, references_section, rows_to_json, strip_markup, synonym_row,
};
use crate::{
    error::{ToolError, ToolResult},
    output::{cv_term_link, report_link},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_postgres::Row;

pub struct ExperimentalTool;

const DEFAULT: &[&str] = &["identity", "description", "uses"];
const OPTIONAL: &[&str] = &[
    "related_tools",
    "constructs",
    "insertions",
    "synonyms",
    "secondary_ids",
    "external_links",
    "references",
];

#[async_trait::async_trait]
impl ReportType for ExperimentalTool {
    fn name(&self) -> &'static str {
        "experimental_tool"
    }

    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }

    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }

    fn offset_sections(&self) -> &'static [&'static str] {
        &["references", "constructs", "insertions"]
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let map: fn(&Row) -> Value = match name {
            "identity" => return feature_identity(ctx).await,
            "external_links" => return external_links_section(ctx).await,
            "references" => return references_section(ctx).await,
            "description" => |r| {
                let kind = r.get::<_, &str>("kind");
                json!({
                    "kind": if kind == "misc" { "notes" } else { kind },
                    "text": curated_text(r.get("text")),
                    "reference_ids": r.get::<_, Vec<String>>("reference_ids"),
                })
            },
            "uses" => |r| {
                let id = r.get::<_, &str>("id");
                json!({
                    "id": id,
                    "name": r.get::<_, &str>("name"),
                    "url": cv_term_link(id),
                    "reference_ids": r.get::<_, Vec<String>>("reference_ids"),
                })
            },
            "related_tools" => related_tool_row,
            "constructs" => {
                return linked_page(ctx, "report/experimental_tool/constructs", construct_row)
                    .await;
            }
            "insertions" => {
                return linked_page(ctx, "report/experimental_tool/insertions", insertion_row)
                    .await;
            }
            "synonyms" => |r| {
                let mut item = synonym_row(r);
                item["reference_ids"] = json!(r.get::<_, Vec<String>>("reference_ids"));
                item
            },
            "secondary_ids" => |r| json!(r.get::<_, &str>("id")),
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown experimental tool section '{other}'"),
                });
            }
        };

        let rows = ctx
            .query(
                &format!("report/experimental_tool/{name}"),
                &[&ctx.id, &ctx.fetch],
            )
            .await?;
        Ok(paged(rows_to_json(&rows, map), ctx.limit))
    }
}

async fn linked_page(ctx: &SectionCtx, query: &str, map: fn(&Row) -> Value) -> ToolResult<Value> {
    let rows = ctx
        .query(query, &[&ctx.id, &ctx.fetch, &ctx.offset])
        .await?;
    Ok(offset_page(rows_to_json(&rows, map), ctx.limit, ctx.offset))
}

fn related_tool_row(r: &Row) -> Value {
    let id = r.get::<_, &str>("id");
    let origin_gene = r.get::<_, Option<&str>>("origin_gene_id").map(|id| {
        json!({
            "id": id,
            "symbol": r.get::<_, Option<&str>>("origin_gene_symbol").map(strip_markup),
        })
    });
    json!({
        "id": id,
        "symbol": r.get::<_, Option<&str>>("symbol").map(strip_markup),
        "url": report_link(id),
        "category": r.get::<_, &str>("category"),
        "relationships": r.get::<_, Vec<String>>("relationships"),
        "origin_gene": origin_gene,
        "use_ids": r.get::<_, Vec<String>>("use_ids"),
        "reference_ids": r.get::<_, Vec<String>>("reference_ids"),
    })
}

fn construct_row(r: &Row) -> Value {
    json!({
        "component_id": r.get::<_, &str>("component_id"),
        "component_symbol": r.get::<_, Option<&str>>("component_symbol").map(strip_markup),
        "relationship": r.get::<_, &str>("relationship"),
        "construct_ids": r.get::<_, Vec<String>>("construct_ids"),
        "regulatory_region_ids": r.get::<_, Vec<String>>("regulatory_region_ids"),
        "encoded_product_ids": r.get::<_, Vec<String>>("encoded_product_ids"),
        "tag_ids": r.get::<_, Vec<String>>("tag_ids"),
        "carried_tool_ids": r.get::<_, Vec<String>>("carried_tool_ids"),
        "stock_count": r.get::<_, i64>("stock_count"),
        "reference_ids": r.get::<_, Vec<String>>("reference_ids"),
    })
}

fn insertion_row(r: &Row) -> Value {
    let relationships = r.get::<_, Vec<String>>("relationships");
    let references = r.get::<_, Vec<Option<String>>>("reference_ids");
    let mut annotations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (relationship, reference) in relationships.into_iter().zip(references) {
        annotations
            .entry(relationship)
            .or_default()
            .extend(reference);
    }
    let annotations: Vec<Value> = annotations
        .into_iter()
        .map(|(relationship, reference_ids)| {
            json!({ "relationship": relationship, "reference_ids": reference_ids })
        })
        .collect();
    json!({
        "id": r.get::<_, &str>("id"),
        "symbol": r.get::<_, Option<&str>>("symbol").map(strip_markup),
        "construct_id": r.get::<_, &str>("construct_id"),
        "construct_symbol": r.get::<_, Option<&str>>("construct_symbol").map(strip_markup),
        "tool_annotations": annotations,
        "associated_allele_ids": r.get::<_, Vec<String>>("associated_allele_ids"),
        "encoded_tool_ids": r.get::<_, Vec<String>>("encoded_tool_ids"),
        "tag_ids": r.get::<_, Vec<String>>("tag_ids"),
        "carried_tool_ids": r.get::<_, Vec<String>>("carried_tool_ids"),
        "stock_count": r.get::<_, i64>("stock_count"),
    })
}
