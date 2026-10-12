use super::{ReportType, SectionCtx, paged, pub_row, rows_to_json, strip_markup, synonym_row};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};

pub struct Disease;

/// `human_genes` lists the Hsap genes linked to the disease model. `fly_models` lists every non-human model gene
/// (not only Dmel), each with its `species` and `role`. Both carry only `gene` features (FBgn IDs).
pub const DEFAULT: &[&str] = &["identity", "human_genes", "do_terms", "fly_models"];
pub const OPTIONAL: &[&str] = &[
    "descriptions",
    "variants",
    "related",
    "references",
    "synonyms",
];

const DESCRIPTION_PROPS: &[(&str, &str)] = &[
    ("overview", "dros_model_overview"),
    ("genetics", "genetics_description"),
    ("molecular", "molecular_description"),
    ("phenotype", "phenotype_description"),
    ("cellular", "cellular_description"),
];

/// Rows of disease/genes.sql as `{id, symbol, species, role, ortholog_comment}`.
pub fn gene_row(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, String>("id"), "symbol": r.get::<_, Option<String>>("symbol"), "species": r.get::<_, Option<String>>("species"),
        "role": r.get::<_, Option<String>>("role"), "ortholog_comment": r.get::<_, Option<String>>("ortholog_comment").map(|s| strip_markup(&s)),
    })
}

/// Rows of disease/variants.sql. The jsonb columns arrive as text; malformed JSON is a data error worth surfacing,
/// not a null, so it is logged and reported as a backend error naming the calling `tool`.
pub fn variant_row(tool: &'static str, r: &tokio_postgres::Row) -> ToolResult<Value> {
    let parse = |col: &str| -> ToolResult<Value> {
        match r.get::<_, Option<String>>(col) {
            None => Ok(Value::Null),
            Some(s) => serde_json::from_str(&s).map_err(|e| {
                tracing::error!(column = col, error = %e, "malformed JSON in humanhealth.allele_disease_variant");
                ToolError::BackendUnavailable { tool, message: format!("malformed {col} data for a disease variant; report this to FlyBase") }
            }),
        }
    };
    Ok(json!({
        "disease_model_id": r.get::<_, Option<String>>("fbhh_id"), "disease_model": r.get::<_, Option<String>>("fbhh_symbol"),
        "allele_id": r.get::<_, Option<String>>("fbal_id"), "allele_symbol": r.get::<_, Option<String>>("fbal_symbol").map(|s| strip_markup(&s)),
        "gene_id": r.get::<_, Option<String>>("fbgn_id"), "gene_symbol": r.get::<_, Option<String>>("fbgn_symbol"),
        "designation": r.get::<_, Option<String>>("div_designation"),
        "dbxrefs": parse("dbxrefs")?, "pubs": parse("pubs")?,
    }))
}

#[async_trait::async_trait]
impl ReportType for Disease {
    fn name(&self) -> &'static str {
        "disease_model"
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
                let r = ctx.query("report/disease/identity", &[id]).await?;
                let r = r.first().ok_or_else(|| ToolError::NotFound {
                    tool: ctx.tool,
                    message: format!("{id} not found"),
                })?;
                json!({
                    "id": id, "name": r.get::<_, Option<String>>("name"), "species": r.get::<_, Option<String>>("species"),
                    "category": r.get::<_, Option<String>>("category"), "subtype": r.get::<_, Option<String>>("subtype"),
                    "omim_phenotype_ids": r.get::<_, Option<String>>("omim_phenotype_ids"),
                })
            }
            "human_genes" | "fly_models" => {
                let r = ctx.query("report/disease/genes", &[id]).await?;
                let human = name == "human_genes";
                json!(
                    r.iter()
                        .filter(|r| (r.get::<_, Option<String>>("species").as_deref()
                            == Some("Hsap"))
                            == human)
                        .map(gene_row)
                        .collect::<Vec<_>>()
                )
            }
            "do_terms" => {
                let r = ctx.query("report/disease/do_terms", &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"term_id": r.get::<_, String>("term_id"), "term": r.get::<_, String>("term")})
                ))
            }
            "descriptions" => {
                let types: Vec<String> = DESCRIPTION_PROPS
                    .iter()
                    .map(|(_, t)| t.to_string())
                    .collect();
                let r = ctx.query("report/disease/props", &[id, &types]).await?;
                let mut out = serde_json::Map::new();
                for (key, prop) in DESCRIPTION_PROPS {
                    let text: Vec<String> = r
                        .iter()
                        .filter(|r| r.get::<_, String>("type") == *prop)
                        .filter_map(|r| r.get::<_, Option<String>>("value"))
                        .map(|s| strip_markup(&s))
                        .collect();
                    out.insert(
                        key.to_string(),
                        if text.is_empty() {
                            Value::Null
                        } else {
                            json!(text.join("\n\n"))
                        },
                    );
                }
                Value::Object(out)
            }
            "variants" => {
                let r = ctx.query("disease/variants", &[id, &ctx.fetch]).await?;
                paged(
                    r.iter()
                        .map(|r| variant_row(ctx.tool, r))
                        .collect::<ToolResult<Vec<_>>>()?,
                    ctx.limit,
                )
            }
            "related" => {
                let r = ctx.query("report/disease/related", &[id]).await?;
                json!(rows_to_json(
                    &r,
                    |r| json!({"relationship": r.get::<_, String>("relationship"), "direction": r.get::<_, String>("direction"), "id": r.get::<_, String>("id"), "name": r.get::<_, Option<String>>("name")})
                ))
            }
            "references" => {
                let r = ctx
                    .query("report/disease/references", &[id, &ctx.fetch])
                    .await?;
                paged(rows_to_json(&r, pub_row), ctx.limit)
            }
            "synonyms" => json!(rows_to_json(
                &ctx.query("report/disease/synonyms", &[id]).await?,
                synonym_row
            )),
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown disease_model section '{other}'"),
                });
            }
        })
    }
}
