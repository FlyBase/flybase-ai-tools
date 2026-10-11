use crate::{
    backends::species::normalize_species,
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, cv_term_link, report_link},
    server::FlyBaseServer,
    tools::resolve::like_escape,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchClass {
    Gene,
    Allele,
    Insertion,
    Construct,
    Aberration,
    Balancer,
    GeneGroup,
    DiseaseModel,
    Stock,
    Ortholog,
    SplitSystemCombination,
    CellLine,
    Library,
    Strain,
    Reference,
    CvTerm,
}

/// Deepest page a caller may reach: offset + limit must not exceed this.
pub const MAX_OFFSET: i64 = 10_000;

impl SearchClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Gene => "gene",
            Self::Allele => "allele",
            Self::Insertion => "insertion",
            Self::Construct => "construct",
            Self::Aberration => "aberration",
            Self::Balancer => "balancer",
            Self::GeneGroup => "gene_group",
            Self::DiseaseModel => "disease_model",
            Self::Stock => "stock",
            Self::Ortholog => "ortholog",
            Self::SplitSystemCombination => "split_system_combination",
            Self::CellLine => "cell_line",
            Self::Library => "library",
            Self::Strain => "strain",
            Self::Reference => "reference",
            Self::CvTerm => "cv_term",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    /// Free text, at least 2 characters. Matches symbols, names, synonyms, reference titles, and vocabulary terms.
    pub query: String,
    /// Restrict to one class. Omit to search genes, alleles, and other entities together (references and CV terms only when asked for).
    pub data_class: Option<SearchClass>,
    /// FlyBase species abbreviation such as Dmel. Default: any.
    pub species: Option<String>,
    /// Reference search only: author surname.
    pub author: Option<String>,
    /// Reference search only: publication year.
    pub year: Option<u32>,
    /// Hits per page (default 25, max 100).
    pub limit: Option<u32>,
    /// Hits to skip; offset + limit may not exceed 10,000.
    pub offset: Option<u32>,
}

/// Where a hit's page lives: the CV report for vocabulary terms, the report page for FlyBase IDs.
fn hit_link(id: &str, data_class: &str) -> Option<String> {
    if data_class == "cv_term" {
        Some(cv_term_link(id))
    } else if id.starts_with("FB") {
        Some(report_link(id))
    } else {
        None
    }
}

#[tool_router(router = tool_router_search, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "search",
        description = "Free-text search over FlyBase: symbols, names, and synonyms of genes, alleles, insertions, constructs, stocks (including stock numbers), cell lines, libraries, strains, gene groups, and disease models; reference titles with author and year filters; and vocabulary terms. Returns a paged hit list. Use resolve when you already know the exact symbol or ID."
    )]
    pub async fn search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "search";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.search_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn search_impl(&self, p: SearchParams) -> ToolResult<CallToolResult> {
        const T: &str = "search";
        let q = p.query.trim().to_string();
        let n_chars = q.chars().count();
        if n_chars < 2 || q.len() > 200 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "query",
                message: "must be 2 to 200 characters".into(),
            });
        }
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!(
                    "offset + limit may not exceed {MAX_OFFSET}; narrow the query instead"
                ),
            });
        }
        let species = match &p.species {
            Some(s) => Some(normalize_species(&self.chado, T, s).await?),
            None => None,
        };
        let fetch = limit + 1;
        let year = p.year.map(|y| y.to_string());
        let escaped = like_escape(&q);
        // A query with no letters or digits has no trigrams, so it cannot use the trigram indexes and would
        // force a sequential scan; it also cannot match anything useful. Answer with no hits instead.
        let searchable = q.chars().any(char::is_alphanumeric);
        let hits: Vec<Value> = if !searchable {
            Vec::new()
        } else {
            match p.data_class {
                Some(SearchClass::Reference) => {
                    let author = p.author.as_deref().map(like_escape);
                    let rows = self
                        .chado
                        .query(
                            T,
                            "search/references",
                            &[&escaped, &author, &year, &fetch, &offset],
                        )
                        .await?;
                    rows.iter()
                        .map(|r| {
                            json!({"id": r.get::<_, String>("uniquename"), "name": r.get::<_, Option<String>>("title"), "symbol": r.get::<_, Option<String>>("miniref"),
                                   "year": r.get::<_, Option<String>>("pyear"), "authors": r.get::<_, Option<String>>("authors"), "data_class": "reference", "species": Value::Null, "match_kind": "substring"})
                        })
                        .collect()
                }
                Some(SearchClass::CvTerm) => {
                    let rows = self
                        .chado
                        .query(T, "search/terms", &[&q, &fetch, &offset, &escaped])
                        .await?;
                    rows.iter()
                        .map(|r| {
                            json!({"id": format!("{}:{}", r.get::<_, String>("db"), r.get::<_, String>("accession")), "name": r.get::<_, String>("name"), "symbol": Value::Null,
                                   "vocabulary": r.get::<_, String>("cv"), "definition": r.get::<_, Option<String>>("definition"), "data_class": "cv_term", "species": Value::Null, "match_kind": r.get::<_, String>("match_kind")})
                        })
                        .collect()
                }
                other => {
                    let class = other.map(SearchClass::as_str);
                    let prefix_only = n_chars < 3;
                    let rows = self
                        .chado
                        .query(
                            T,
                            "search/names",
                            &[
                                &q,
                                &species,
                                &class,
                                &fetch,
                                &offset,
                                &prefix_only,
                                &escaped,
                            ],
                        )
                        .await?;
                    rows.iter()
                        .map(|r| {
                            json!({"id": r.get::<_, String>("entity_id"), "symbol": r.get::<_, Option<String>>("current_symbol"), "name": r.get::<_, String>("matched"),
                                   "data_class": r.get::<_, String>("data_class"), "species": r.get::<_, Option<String>>("species"), "match_kind": r.get::<_, String>("match_kind")})
                        })
                        .collect()
                }
            }
        };
        let has_more = hits.len() as i64 > limit;
        let hits: Vec<Value> = hits
            .into_iter()
            .take(limit as usize)
            .map(|mut h| {
                let link = hit_link(
                    h["id"].as_str().unwrap_or(""),
                    h["data_class"].as_str().unwrap_or(""),
                );
                h["link"] = json!(link);
                h
            })
            .collect();
        let summary = if hits.is_empty() {
            format!("No hits for '{q}'.")
        } else {
            let more = if has_more {
                ", more available with offset"
            } else {
                ""
            };
            format!(
                "{} hit(s) for '{q}'{more}; first: {} ({}).",
                hits.len(),
                hits[0]["id"].as_str().unwrap_or(""),
                hits[0]["name"].as_str().unwrap_or("")
            )
        };
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .field("hits", json!(hits))
            .field("has_more", json!(has_more))
            .field("offset", json!(offset));
        if let Some(link) = hits.first().and_then(|h| h["link"].as_str()) {
            env = env.link("first_report", link.to_string());
        }
        Ok(env.finish())
    }
}
