use super::{ReportType, SectionCtx, paged, reference_page, rows_to_json, strip_markup};
use crate::{error::ToolResult, output::report_link};
use serde_json::{Value, json};

pub struct PhysicalInteraction;

#[async_trait::async_trait]
impl ReportType for PhysicalInteraction {
    fn name(&self) -> &'static str {
        "physical_interaction"
    }

    fn default_sections(&self) -> &'static [&'static str] {
        &["identity", "reported_interactions"]
    }

    fn optional_sections(&self) -> &'static [&'static str] {
        &["references"]
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        Ok(match name {
            "identity" => {
                let rows = ctx
                    .query("report/physical_interaction/identity", &[&ctx.id])
                    .await?;
                let row = rows
                    .first()
                    .ok_or_else(|| crate::error::ToolError::NotFound {
                        tool: ctx.tool,
                        message: format!("{} not found", ctx.id),
                    })?;
                let mut genes: Value = row.get("genes");
                for gene in genes.as_array_mut().expect("gene array") {
                    let id = gene["id"].as_str().expect("gene ID");
                    let report_url = report_link(id);
                    let network_url = format!(
                        "https://flybase.org/cgi-bin/get_interactions.html?items={id}&mode=ppi"
                    );
                    gene["report_url"] = json!(report_url);
                    gene["network_url"] = json!(network_url);
                    strip_field(gene, "symbol");
                }
                json!({
                    "id": ctx.id,
                    "interaction_types": row.get::<_, Vec<String>>("interaction_types"),
                    "genes": genes,
                })
            }
            "reported_interactions" => {
                let rows = ctx
                    .query(
                        "report/physical_interaction/reported_interactions",
                        &[&ctx.id, &ctx.fetch],
                    )
                    .await?;
                paged(
                    rows_to_json(&rows, |row| {
                        let mut participants: Value = row.get("participants");
                        for participant in participants.as_array_mut().expect("participant array") {
                            strip_field(participant, "name");
                            for field in ["reported_as", "notes"] {
                                for value in participant[field].as_array_mut().expect("text array")
                                {
                                    if let Some(text) = value.as_str() {
                                        *value = json!(strip_markup(text));
                                    }
                                }
                            }
                        }
                        let mut comments: Value = row.get("comments");
                        for comment in comments.as_array_mut().expect("comment array") {
                            strip_field(comment, "text");
                        }
                        json!({
                            "record_id": row.get::<_, String>("uniquename"),
                            "interaction_type": row.get::<_, String>("interaction_type"),
                            "molecule_pair": row.get::<_, Option<String>>("molecule_pair"),
                            "reference_ids": row.get::<_, Vec<String>>("reference_ids"),
                            "assays": row.get::<_, Value>("assays"),
                            "participants": participants,
                            "comments": comments,
                            "cell_lines": row.get::<_, Value>("cell_lines"),
                            "collections": row.get::<_, Value>("collections"),
                            "sources": row.get::<_, Value>("sources"),
                        })
                    }),
                    ctx.limit,
                )
            }
            "references" => {
                return reference_page(ctx, "report/physical_interaction/references").await;
            }
            _ => unreachable!("validated report section"),
        })
    }
}

fn strip_field(item: &mut Value, field: &str) {
    if let Some(text) = item[field].as_str() {
        item[field] = json!(strip_markup(text));
    }
}
