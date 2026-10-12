use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    report::strip_markup,
    server::FlyBaseServer,
    tools::{
        resolve::{normalize_fb_prefix, resolve_ids},
        search::MAX_OFFSET,
    },
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Physical,
    Genetic,
    #[default]
    Both,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InteractionsParams {
    /// A gene (FBgn) or allele (FBal). Physical interactions are at gene level; an allele gives allele-level genetic interactions.
    pub id: String,
    /// physical, genetic, or both (default both).
    pub kind: Option<Kind>,
    /// Items per page for each kind (default 25, max 100).
    pub limit: Option<u32>,
    /// Items to skip in each list.
    pub offset: Option<u32>,
}

fn opt(r: &tokio_postgres::Row, col: &str) -> Value {
    json!(r.get::<_, Option<String>>(col).map(|s| strip_markup(&s)))
}

#[tool_router(router = tool_router_interactions, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_interactions",
        description = "Physical (protein-protein) and genetic interactions for a gene, or allele-level genetic interactions for an allele, with partners, roles, detection methods, outcomes, and references. A self-interaction lists the gene as its own partner; allele comparisons give genotype (whose phenotype is described), compared_with, and side (which of the two carries the allele). One hop only; call again on a partner to go further. Use get_report sections for everything else about the gene."
    )]
    pub async fn get_interactions(
        &self,
        Parameters(p): Parameters<InteractionsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_interactions";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.interactions_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn interactions_impl(&self, p: InteractionsParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_interactions";
        let input = normalize_fb_prefix(p.id.trim()).into_owned();
        if !(input.starts_with("FBgn") || input.starts_with("FBal")) || input.len() != 11 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: format!("'{input}' must be a gene (FBgn) or allele (FBal) ID"),
            });
        }
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + limit may not exceed {MAX_OFFSET}"),
            });
        }
        let kind = p.kind.unwrap_or_default();
        let st = resolve_ids(&self.chado, T, std::slice::from_ref(&input))
            .await?
            .remove(0);
        let id = match st.status {
            "current" | "updated" => st.current_ids[0].clone(),
            _ => {
                return Err(ToolError::NotFound {
                    tool: T,
                    message: format!("{input} is not a current gene or allele ID; use resolve"),
                });
            }
        };
        let is_allele = id.starts_with("FBal");
        let fetch = limit + 1;
        let conn = self.chado.get(T).await?;
        // The gene: the input itself, or the allele's gene (physical interactions are gene level).
        let (gene_id, gene_symbol, allele_symbol) = if is_allele {
            let r = Chado::query_on(&conn, T, "report/allele/gene", &[&id]).await?;
            let r = r.first().ok_or_else(|| ToolError::NotFound {
                tool: T,
                message: format!("{id} has no gene"),
            })?;
            (
                r.get::<_, String>("id"),
                r.get::<_, Option<String>>("symbol"),
                st.symbol.clone(),
            )
        } else {
            (id.clone(), st.symbol.clone(), None)
        };
        let mut env = Envelope::new(T, &self.release)
            .link("report", report_link(&id))
            .field("id", json!(id))
            .field("gene", json!({"id": gene_id, "symbol": gene_symbol}))
            .field("offset", json!(offset));
        let mut parts: Vec<String> = Vec::new();
        if matches!(kind, Kind::Physical | Kind::Both) {
            let rows = Chado::query_on(
                &conn,
                T,
                "interactions/physical",
                &[&gene_id, &fetch, &offset],
            )
            .await?;
            let has_more = rows.len() as i64 > limit;
            let items: Vec<Value> = rows
                .iter()
                .take(limit as usize)
                .map(|r| {
                    json!({"interaction": r.get::<_, String>("interaction"), "type": r.get::<_, String>("type"), "partner_id": r.get::<_, String>("partner_id"),
                           "partner_symbol": opt(r, "partner_symbol"), "role": opt(r, "role"), "partner_role": opt(r, "partner_role"), "detection": opt(r, "detection"),
                           "references": opt(r, "refs"), "comment": opt(r, "comment")})
                })
                .collect();
            parts.push(format!(
                "{} physical interaction(s){}",
                items.len(),
                if has_more { " (more with offset)" } else { "" }
            ));
            env = env.field("physical", json!({"items": items, "has_more": has_more}));
            if is_allele {
                env = env.field(
                    "note",
                    json!(format!(
                        "physical interactions are recorded at gene level; showing {gene_id}"
                    )),
                );
            }
        }
        if matches!(kind, Kind::Genetic | Kind::Both) {
            let (query, label) = if is_allele {
                (
                    "interactions/genetic_allele",
                    "allele-level genetic comparison(s)",
                )
            } else {
                ("interactions/genetic_gene", "genetic interaction(s)")
            };
            let rows = Chado::query_on(&conn, T, query, &[&id, &fetch, &offset]).await?;
            let has_more = rows.len() as i64 > limit;
            let items: Vec<Value> = rows
                .iter()
                .take(limit as usize)
                .map(|r| {
                    if is_allele {
                        json!({"genotype": opt(r, "genotype"), "compared_with": opt(r, "compared_with"), "side": r.get::<_, String>("side"), "phenotype": opt(r, "phenotype"), "outcome": opt(r, "outcome"),
                               "reference": r.get::<_, String>("reference"), "citation": r.get::<_, Option<String>>("miniref")})
                    } else {
                        json!({"partner_id": r.get::<_, String>("partner_id"), "partner_symbol": opt(r, "partner_symbol"), "direction": r.get::<_, String>("direction"),
                               "allele": opt(r, "allele"), "references": opt(r, "refs")})
                    }
                })
                .collect();
            parts.push(format!(
                "{} {label}{}",
                items.len(),
                if has_more { " (more with offset)" } else { "" }
            ));
            env = env.field("genetic", json!({"items": items, "has_more": has_more}));
        }
        let label = allele_symbol
            .or(st.symbol)
            .map(|s| format!(" ({})", strip_markup(&s)))
            .unwrap_or_default();
        Ok(env
            .summary(format!("{id}{label}: {}.", parts.join("; ")))
            .finish())
    }
}
