use crate::{
    backends::{chado::Chado, diopt::parse_detail, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::MAX_LIMIT,
    output::{Envelope, complete, report_link},
    server::FlyBaseServer,
    tools::resolve::{looks_like_fb_id, normalize_fb_prefix, resolve_ids},
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrthologsParams {
    /// A fly gene (FBgn), or a human gene as a symbol (NOTCH1) or HGNC ID (HGNC:7881).
    pub id: String,
    /// Species abbreviations to keep (Hsap, Mmus, Drer, Cele, Scer, Dsim, ...). Default all.
    pub species: Option<Vec<String>>,
    /// Keep orthologs with a DIOPT score of at least this (number of supporting algorithms).
    pub min_score: Option<u32>,
    /// Keep only best matches (best score in either direction).
    pub best_only: Option<bool>,
    /// Max orthologs to return (default 100, max 1000).
    pub limit: Option<u32>,
}

#[derive(Default)]
struct Ortholog {
    id: String,
    symbol: Option<String>,
    species: String,
    detail: Option<String>,
    hgnc: Option<String>,
    human_score: Option<i32>,
    omim: Option<String>,
}

impl Ortholog {
    fn json(&self) -> Value {
        let d = parse_detail(self.detail.as_deref());
        let score = self
            .human_score
            .map(i64::from)
            .or_else(|| (!d.methods.is_empty()).then_some(d.methods.len() as i64));
        json!({
            "id": self.id, "symbol": self.symbol, "species": self.species, "score": score,
            "best_score": d.best_score, "best_score_rev": d.best_score_rev, "methods": d.methods,
            "hgnc_id": self.hgnc.as_ref().map(|h| format!("HGNC:{h}")), "omim_phenotypes": self.omim, "link": report_link(&self.id),
        })
    }
}

#[tool_router(router = tool_router_orthologs, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_orthologs",
        description = "Orthologs of a fly gene in human, mouse, rat, zebrafish, frog, worm, mosquito, yeast, and other Drosophila species, with DIOPT scores, best-match flags, and supporting algorithms; or the fly orthologs of a human gene (symbol or HGNC ID) with OMIM phenotypes. Use for cross-species questions; use get_report for the fly gene itself."
    )]
    pub async fn get_orthologs(
        &self,
        Parameters(p): Parameters<OrthologsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_orthologs";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.orthologs_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn orthologs_impl(&self, p: OrthologsParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_orthologs";
        let limit = p.limit.unwrap_or(MAX_LIMIT).clamp(1, 1000) as usize;
        let mut species: Vec<String> = Vec::new();
        for s in p.species.unwrap_or_default() {
            species.push(normalize_species(&self.chado, T, &s).await?);
        }
        let species_filter: Option<Vec<String>> = (!species.is_empty()).then_some(species.clone());
        let input = normalize_fb_prefix(p.id.trim()).into_owned();
        if input.is_empty() || input.len() > 100 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: "must be 1 to 100 characters".into(),
            });
        }
        let conn = self.chado.get(T).await?;
        let file_loaded = Chado::query_on(&conn, T, "orthologs/file_loaded", &[])
            .await?
            .first()
            .is_some_and(|r| r.get::<_, bool>("loaded"));
        let (query, rows): (Value, Vec<Ortholog>) = if looks_like_fb_id(&input) {
            if !input.starts_with("FBgn") {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "id",
                    message: format!("'{input}' is not a gene ID (FBgn) or human gene"),
                });
            }
            drop(conn);
            let st = resolve_ids(&self.chado, T, std::slice::from_ref(&input))
                .await?
                .remove(0);
            let id = match st.status {
                "current" | "updated" => st.current_ids[0].clone(),
                _ => {
                    return Err(ToolError::NotFound {
                        tool: T,
                        message: format!("{input} is not a current gene ID; use resolve"),
                    });
                }
            };
            let conn = self.chado.get(T).await?;
            let rows =
                Chado::query_on(&conn, T, "orthologs/by_gene", &[&id, &species_filter]).await?;
            let list = rows
                .iter()
                .map(|r| Ortholog {
                    id: r.get("ortholog_id"),
                    symbol: r.get("symbol"),
                    species: r.get("species"),
                    detail: r.get("detail"),
                    hgnc: r.get("hgnc"),
                    human_score: r.get("human_score"),
                    omim: r.get("omim_phenotypes"),
                })
                .collect();
            (
                json!({"id": id, "symbol": st.symbol, "species": "Dmel"}),
                list,
            )
        } else {
            let acc = input.strip_prefix("HGNC:").unwrap_or(&input).to_string();
            let h = Chado::query_on(&conn, T, "orthologs/human_gene", &[&acc]).await?;
            let h = h.first().ok_or_else(|| ToolError::NotFound {
                tool: T,
                message: format!(
                    "no human gene named '{input}' in FlyBase; use a symbol such as NOTCH1 or an HGNC ID such as HGNC:7881"
                ),
            })?;
            let fbog: String = h.get("uniquename");
            if species_filter
                .as_ref()
                .is_some_and(|s| !s.iter().any(|x| x == "Dmel"))
            {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "species",
                    message:
                        "a human query returns D. melanogaster genes; omit species or pass Dmel"
                            .into(),
                });
            }
            let rows = Chado::query_on(&conn, T, "orthologs/by_human", &[&fbog]).await?;
            let list = rows
                .iter()
                .map(|r| Ortholog {
                    id: r.get("gene_id"),
                    symbol: r.get("symbol"),
                    species: "Dmel".into(),
                    detail: r.get("detail"),
                    hgnc: input.starts_with("HGNC:").then_some(acc.clone()),
                    human_score: r.get("human_score"),
                    omim: r.get("omim_phenotypes"),
                })
                .collect();
            (
                json!({"id": fbog, "symbol": h.get::<_, Option<String>>("symbol"), "species": "Hsap"}),
                list,
            )
        };
        let min_score = p.min_score.map(i64::from);
        let best_only = p.best_only.unwrap_or(false);
        let mut orthologs: Vec<Value> = rows.iter().map(Ortholog::json).collect();
        orthologs.retain(|o| {
            let score_ok = min_score.is_none_or(|m| o["score"].as_i64().is_some_and(|s| s >= m));
            let best_ok = !best_only || o["best_score"] == true || o["best_score_rev"] == true;
            score_ok && best_ok
        });
        orthologs.sort_by(|a, b| {
            b["score"]
                .as_i64()
                .unwrap_or(-1)
                .cmp(&a["score"].as_i64().unwrap_or(-1))
                .then_with(|| a["species"].as_str().cmp(&b["species"].as_str()))
                .then_with(|| a["symbol"].as_str().cmp(&b["symbol"].as_str()))
        });
        let count = orthologs.len();
        orthologs.truncate(limit);
        let summary = format!(
            "{} ortholog(s) of {} ({}){}; showing {} by DIOPT score. Scores count supporting algorithms; Drosophila pairs from OrthoDB have no score.",
            count,
            query["id"].as_str().unwrap_or(""),
            query["symbol"].as_str().unwrap_or(""),
            match &species_filter {
                Some(s) => format!(" in {}", s.join(", ")),
                None => String::new(),
            },
            orthologs.len()
        );
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .link("report", report_link(query["id"].as_str().unwrap_or("")))
            .field("query", query)
            .field("orthologs", json!(orthologs))
            .field("count", json!(count))
            .field("diopt_file_loaded", json!(file_loaded))
            .finish())
    }
}
