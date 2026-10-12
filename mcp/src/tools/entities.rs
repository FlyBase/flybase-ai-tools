use crate::{
    backends::{chado::Chado, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, cv_term_link, report_link},
    report::cvterm::{MAX_DESCENDANTS, canonical_term_id, parse_term_id},
    server::FlyBaseServer,
    tools::search::MAX_OFFSET,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

/// The gene and allele queries scan at most this many annotation rows (`limit 200001` in the SQL); more is refused.
pub const MAX_HITS: i64 = 200_000;

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum EntityClass {
    Gene,
    Allele,
    DiseaseModel,
    GeneGroup,
}

impl EntityClass {
    fn label(self) -> &'static str {
        match self {
            Self::Gene => "gene",
            Self::Allele => "allele",
            Self::DiseaseModel => "disease model",
            Self::GeneGroup => "gene group",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntitiesParams {
    /// A vocabulary term ID (GO:0007219, FBbt:00004729, FBcv:..., DOID:162, SO:...).
    pub term_id: String,
    /// gene (feature_cvterm annotations such as GO), allele (disease annotations for DOID terms, otherwise phenotypes
    /// whose anatomy or phenotypic class is the term), disease_model, or gene_group.
    pub data_class: EntityClass,
    /// Include terms below this one (is_a and part_of), default true. Sets over 5,000 terms are refused.
    pub include_descendants: Option<bool>,
    /// Species abbreviation (genes and alleles only).
    pub species: Option<String>,
    /// Entities per page (default 25, max 100).
    pub limit: Option<u32>,
    /// Entities to skip; offset + limit may not exceed 10,000.
    pub offset: Option<u32>,
}

#[tool_router(router = tool_router_entities, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "entities_for_term",
        description = "List the genes, alleles, disease models, or gene groups annotated to a vocabulary term (GO process, anatomy FBbt phenotype, DOID disease, ...), optionally including its descendant terms, with evidence or qualifier summaries and reference counts. Use after search_vocabulary or get_term; use get_report on a hit for details."
    )]
    pub async fn entities_for_term(
        &self,
        Parameters(p): Parameters<EntitiesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "entities_for_term";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.heavy(T).await?;
            self.entities_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn entities_impl(&self, p: EntitiesParams) -> ToolResult<CallToolResult> {
        const T: &str = "entities_for_term";
        let term_id =
            canonical_term_id(p.term_id.trim()).ok_or_else(|| ToolError::InvalidInput {
                tool: T,
                arg: "term_id",
                message: format!(
                    "'{}' is not a vocabulary term ID such as GO:0007219",
                    p.term_id.trim()
                ),
            })?;
        let (db, acc) = parse_term_id(&term_id).expect("canonical");
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + limit may not exceed {MAX_OFFSET}"),
            });
        }
        let class = p.data_class;
        if p.species.is_some()
            && matches!(class, EntityClass::DiseaseModel | EntityClass::GeneGroup)
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "species",
                message: format!("species does not apply to {}", class.label()),
            });
        }
        let species = match &p.species {
            Some(s) => Some(normalize_species(&self.chado, T, s).await?),
            None => None,
        };
        let include = p.include_descendants.unwrap_or(true);
        let fetch = limit + 1;
        // Disease Ontology terms are annotated on alleles directly (feature_cvterm); every other vocabulary reaches
        // alleles through phenotype statements.
        let annotated_alleles = class == EntityClass::Allele && db == "DOID";
        let (term, rows) = self
            .chado
            .heavy(T, async |tx| {
                let t = Chado::query_tx(tx, T, "report/cvterm/term", &[&db, &acc]).await?;
                let t = t.first().ok_or_else(|| ToolError::NotFound {
                    tool: T,
                    message: format!("{term_id} is not a term in the {db} vocabulary"),
                })?;
                let cvterm_id: i32 = t.get("cvterm_id");
                let d =
                    Chado::query_tx(tx, T, "report/cvterm/descendant_count", &[&cvterm_id]).await?;
                let descendants = d.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0);
                if include && descendants > MAX_DESCENDANTS {
                    return Err(ToolError::LimitExceeded {
                        tool: T,
                        message: format!(
                            "{term_id} has {descendants} descendants (max {MAX_DESCENDANTS}); choose a narrower term or set include_descendants=false"
                        ),
                    });
                }
                let term = json!({
                    "id": term_id,
                    "name": t.get::<_, String>("name"),
                    "vocabulary": t.get::<_, String>("vocabulary"),
                    "descendants": descendants,
                    "descendants_included": include,
                });
                let paged: &[&(dyn tokio_postgres::types::ToSql + Sync)] =
                    &[&cvterm_id, &include, &species, &fetch, &offset];
                let unfiltered: &[&(dyn tokio_postgres::types::ToSql + Sync)] =
                    &[&cvterm_id, &include, &fetch, &offset];
                let rows = match class {
                    EntityClass::Gene => Chado::query_tx(tx, T, "term/genes", paged).await?,
                    EntityClass::Allele if annotated_alleles => {
                        Chado::query_tx(tx, T, "term/alleles_annotated", paged).await?
                    }
                    EntityClass::Allele => {
                        Chado::query_tx(tx, T, "term/alleles_phenotype", paged).await?
                    }
                    EntityClass::DiseaseModel => {
                        Chado::query_tx(tx, T, "term/disease_models", unfiltered).await?
                    }
                    EntityClass::GeneGroup => {
                        Chado::query_tx(tx, T, "term/gene_groups", unfiltered).await?
                    }
                };
                // The gene and allele queries cap their filtered annotation scan at MAX_HITS + 1 rows; reaching it
                // means the page would be computed from an incomplete set, so the call is refused instead. Every row
                // carries the count, and an empty page still returns one sentinel row (null id) with it, so a capped
                // term is refused even when the page itself is empty.
                let mut rows = rows;
                if matches!(class, EntityClass::Gene | EntityClass::Allele) {
                    if rows
                        .iter()
                        .any(|r| r.get::<_, i64>("hits") > MAX_HITS)
                    {
                        return Err(ToolError::LimitExceeded {
                            tool: T,
                            message: format!(
                                "{term_id} has more than {MAX_HITS} matching annotations; add a species filter, choose a narrower term, or set include_descendants=false"
                            ),
                        });
                    }
                    rows.retain(|r| r.get::<_, Option<String>>("id").is_some());
                }
                Ok((term, rows))
            })
            .await?;
        let has_more = rows.len() as i64 > limit;
        let entities: Vec<Value> = rows
            .iter()
            .take(limit as usize)
            .map(|r| {
                let id: String = r.get("id");
                let mut v = json!({"id": id, "link": report_link(&id)});
                let feature = |v: &mut Value| {
                    v["symbol"] = json!(r.get::<_, Option<String>>("symbol"));
                    v["species"] = json!(r.get::<_, Option<String>>("species"));
                    v["reference_count"] = json!(r.get::<_, i64>("reference_count"));
                };
                match class {
                    // Genes and DOID-annotated alleles share the feature_cvterm shape; the summary column is evidence
                    // codes for genes and qualifiers (model of, ameliorates, ...) for alleles.
                    EntityClass::Gene => {
                        feature(&mut v);
                        v["matching_terms"] = json!(r.get::<_, i64>("terms"));
                        v["evidence"] = json!(r.get::<_, Option<String>>("evidence"));
                    }
                    EntityClass::Allele if annotated_alleles => {
                        feature(&mut v);
                        v["matching_terms"] = json!(r.get::<_, i64>("terms"));
                        v["qualifiers"] = json!(r.get::<_, Option<String>>("qualifiers"));
                    }
                    EntityClass::Allele => {
                        feature(&mut v);
                        v["statements"] = json!(r.get::<_, i64>("statements"));
                    }
                    EntityClass::DiseaseModel => {
                        v["name"] = json!(r.get::<_, Option<String>>("name"));
                        v["matching_terms"] = json!(r.get::<_, i64>("terms"));
                    }
                    EntityClass::GeneGroup => {
                        v["symbol"] = json!(r.get::<_, Option<String>>("symbol"));
                        v["matching_terms"] = json!(r.get::<_, i64>("terms"));
                    }
                }
                v
            })
            .collect();
        let summary = format!(
            "{} {}(s) for {} ({}){}{}.",
            entities.len(),
            class.label(),
            term["id"].as_str().unwrap_or(""),
            term["name"].as_str().unwrap_or(""),
            if include {
                format!(", including {} descendant term(s)", term["descendants"])
            } else {
                String::new()
            },
            if has_more {
                "; more available with offset"
            } else {
                ""
            }
        );
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .link("term", cv_term_link(term["id"].as_str().unwrap_or("")))
            .field("term", term)
            .field("entities", json!(entities))
            .field("has_more", json!(has_more))
            .field("offset", json!(offset))
            .finish())
    }
}
