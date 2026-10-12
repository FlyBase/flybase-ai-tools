use crate::{
    backends::{chado::Chado, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    server::FlyBaseServer,
    tools::search::MAX_OFFSET,
};
use deadpool_postgres::Object;
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_CONDITIONS: usize = 10;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

impl Operator {
    fn as_str(self) -> &'static str {
        match self {
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Eq => "eq",
        }
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// Dataset name from list_options (FlyAtlas2, modENCODE_development, modENCODE_tissues, modENCODE_treatments, modENCODE_cell_lines).
    pub dataset: String,
    /// Sample name within the dataset, from list_options (for example Adult_Male_Testis).
    pub sample: String,
    /// Comparison of the gene's value in that sample.
    pub operator: Operator,
    /// RPKM threshold.
    pub value: f32,
}

#[derive(Deserialize, Serialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileSearchParams {
    /// One to ten conditions, all in the same dataset; a gene must satisfy every one.
    pub conditions: Option<Vec<Condition>>,
    /// Only Dmel has RNA-seq profiles (default Dmel).
    pub species: Option<String>,
    /// Genes per page (default 25, max 100).
    pub limit: Option<u32>,
    /// Genes to skip; offset + limit may not exceed 10,000.
    pub offset: Option<u32>,
    /// If true, return only the datasets with their sample names.
    pub list_options: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    pub dataset: String,
    pub project_id: String,
    pub project_name: String,
    pub samples: Vec<String>,
    pub gene_count: i32,
    pub value_type: String,
}

/// The dataset catalog (five rows) on the caller's connection.
pub async fn datasets(conn: &Object, tool: &'static str) -> ToolResult<Vec<Dataset>> {
    let rows = Chado::query_on(conn, tool, "rnaseq/datasets", &[]).await?;
    Ok(rows
        .iter()
        .map(|r| Dataset {
            dataset: r.get("dataset"),
            project_id: r.get("project_id"),
            project_name: r.get("project_name"),
            samples: r.get("samples"),
            gene_count: r.get("gene_count"),
            value_type: r.get("value_type"),
        })
        .collect())
}

pub fn dataset_json(d: &Dataset) -> Value {
    json!({"dataset": d.dataset, "project_id": d.project_id, "project_name": d.project_name, "samples": d.samples,
           "gene_count": d.gene_count, "value_type": d.value_type, "link": report_link(&d.project_id)})
}

/// Find a dataset by name (case-insensitive) or explain which exist.
pub fn find_dataset<'a>(
    all: &'a [Dataset],
    name: &str,
    tool: &'static str,
) -> ToolResult<&'a Dataset> {
    all.iter()
        .find(|d| d.dataset.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| ToolError::InvalidInput {
            tool,
            arg: "dataset",
            message: format!(
                "unknown dataset '{name}'; datasets: {} (call with list_options=true for samples)",
                all.iter()
                    .map(|d| d.dataset.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
}

/// Only Dmel has profiles; anything else is invalid input, and an absent species means Dmel.
async fn require_dmel(chado: &Chado, tool: &'static str, species: Option<&str>) -> ToolResult<()> {
    if let Some(s) = species {
        let canon = normalize_species(chado, tool, s).await?;
        if canon != "Dmel" {
            return Err(ToolError::InvalidInput {
                tool,
                arg: "species",
                message: format!("RNA-seq profiles exist for Dmel only, not {canon}"),
            });
        }
    }
    Ok(())
}

#[tool_router(router = tool_router_rnaseq, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "rnaseq_profile_search",
        description = "Find D. melanogaster genes whose RNA-seq expression (RPKM) meets every condition you give, such as high in adult testis and low in ovary, over the FlyAtlas2 tissue atlas or the modENCODE developmental, tissue, treatment, and cell-line series. Use list_options=true first to see dataset and sample names. Use rnaseq_similar_genes to rank genes by profile similarity instead."
    )]
    pub async fn rnaseq_profile_search(
        &self,
        Parameters(p): Parameters<ProfileSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "rnaseq_profile_search";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.rnaseq_profile_search_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn rnaseq_profile_search_impl(
        &self,
        p: ProfileSearchParams,
    ) -> ToolResult<CallToolResult> {
        const T: &str = "rnaseq_profile_search";
        require_dmel(&self.chado, T, p.species.as_deref()).await?;
        let conn = self.chado.get(T).await?;
        let all = datasets(&conn, T).await?;
        if p.list_options.unwrap_or(false) {
            return Ok(Envelope::new(T, &self.release)
                .summary(format!("{} RNA-seq datasets: {}. Each condition names a dataset and one of its samples.", all.len(), all.iter().map(|d| d.dataset.as_str()).collect::<Vec<_>>().join(", ")))
                .field("datasets", json!(all.iter().map(dataset_json).collect::<Vec<_>>()))
                .finish());
        }
        let conds = p.conditions.unwrap_or_default();
        if conds.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "conditions",
                message:
                    "give at least one condition, or list_options=true to see datasets and samples"
                        .into(),
            });
        }
        if conds.len() > MAX_CONDITIONS {
            return Err(ToolError::LimitExceeded {
                tool: T,
                message: format!("{MAX_CONDITIONS} conditions max per call"),
            });
        }
        let ds = find_dataset(&all, &conds[0].dataset, T)?;
        if conds
            .iter()
            .any(|c| !c.dataset.eq_ignore_ascii_case(&ds.dataset))
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "conditions",
                message: "all conditions must use the same dataset; run one call per dataset"
                    .into(),
            });
        }
        let mut samples = Vec::with_capacity(conds.len());
        for c in &conds {
            let s = ds.samples.iter().find(|s| s.eq_ignore_ascii_case(c.sample.trim())).ok_or_else(|| ToolError::InvalidInput {
                tool: T,
                arg: "conditions",
                message: format!("'{}' is not a sample of {}; call with list_options=true for the {} sample names", c.sample, ds.dataset, ds.samples.len()),
            })?;
            if !c.value.is_finite() || c.value < 0.0 {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "conditions",
                    message: "value must be a non-negative number".into(),
                });
            }
            samples.push(s.clone());
        }
        let ops: Vec<String> = conds
            .iter()
            .map(|c| c.operator.as_str().to_string())
            .collect();
        let values: Vec<f32> = conds.iter().map(|c| c.value).collect();
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + limit may not exceed {MAX_OFFSET}"),
            });
        }
        let fetch = limit + 1;
        let dataset = ds.dataset.clone();
        let rows = Chado::query_on(
            &conn,
            T,
            "rnaseq/search",
            &[&dataset, &samples, &ops, &values, &fetch, &offset],
        )
        .await?;
        let total = rows.first().map(|r| r.get::<_, i64>("total")).unwrap_or(0);
        let has_more = rows.len() as i64 > limit;
        let genes: Vec<Value> = rows
            .iter()
            .take(limit as usize)
            .map(|r| {
                let id: String = r.get("gene_id");
                let values: Value = r.get::<_, Option<String>>("values").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                json!({"id": id, "symbol": r.get::<_, Option<String>>("symbol"), "values": values, "link": report_link(&id)})
            })
            .collect();
        let echo: Vec<Value> = conds.iter().zip(&samples).map(|(c, s)| json!({"dataset": &dataset, "sample": s, "operator": c.operator.as_str(), "value": c.value})).collect();
        let summary = if genes.is_empty() {
            format!("No {dataset} genes meet all {} condition(s).", conds.len())
        } else {
            format!(
                "{} of {total} {dataset} gene(s) meeting all {} condition(s){}; first: {} ({}).",
                genes.len(),
                conds.len(),
                if has_more { ", more with offset" } else { "" },
                genes[0]["id"].as_str().unwrap_or(""),
                genes[0]["symbol"].as_str().unwrap_or("")
            )
        };
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .field("dataset", json!(dataset))
            .field("conditions", json!(echo))
            .field("genes", json!(genes))
            .field("total", json!(total))
            .field("has_more", json!(has_more))
            .field("offset", json!(offset))
            .link("dataset_report", report_link(&ds.project_id));
        if let Some(first) = genes.first().and_then(|g| g["link"].as_str()) {
            env = env.link("first_report", first.to_string());
        }
        Ok(env.finish())
    }
}
