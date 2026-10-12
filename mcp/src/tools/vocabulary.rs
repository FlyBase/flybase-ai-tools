use crate::backends::cache::ResponseCache;
use crate::{
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, cv_term_link},
    report::{
        ReportType, SectionCtx,
        cvterm::{CvTerm, canonical_term_id},
        section_error,
    },
    server::FlyBaseServer,
    tools::{resolve::like_escape, search::MAX_OFFSET},
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Vocabulary {
    /// Gene Ontology: biological process, molecular function, cellular component.
    Go,
    /// FlyBase anatomy (FBbt).
    Anatomy,
    /// FlyBase developmental stages (FBdv).
    Development,
    /// Disease Ontology (DOID).
    Disease,
    /// Sequence Ontology (SO).
    Sequence,
    /// FlyBase miscellaneous CV (FBcv): phenotypic classes, experimental assays, and so on.
    Misc,
    /// PSI-MI interaction detection methods and roles.
    PsiMi,
}

impl Vocabulary {
    pub const ALL: &[Vocabulary] = &[
        Self::Go,
        Self::Anatomy,
        Self::Development,
        Self::Disease,
        Self::Sequence,
        Self::Misc,
        Self::PsiMi,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Anatomy => "anatomy",
            Self::Development => "development",
            Self::Disease => "disease",
            Self::Sequence => "sequence",
            Self::Misc => "misc",
            Self::PsiMi => "psi_mi",
        }
    }
    /// The `cv.name` values the vocabulary covers in chado.
    pub fn cv_names(self) -> &'static [&'static str] {
        match self {
            Self::Go => &[
                "biological_process",
                "molecular_function",
                "cellular_component",
            ],
            Self::Anatomy => &["FlyBase anatomy CV"],
            Self::Development => &["FlyBase development CV"],
            Self::Disease => &["disease_ontology"],
            Self::Sequence => &["SO"],
            Self::Misc => &["FlyBase miscellaneous CV"],
            Self::PsiMi => &["PSI-MI"],
        }
    }
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchVocabularyParams {
    /// Free text, at least 2 characters; matches term names and synonyms.
    pub query: String,
    /// Vocabularies to search (default all). Pass list_options=true to see them.
    pub vocabularies: Option<Vec<Vocabulary>>,
    /// Terms to return (default 25, max 100).
    pub limit: Option<u32>,
    /// If true, return only the vocabulary names and nothing else.
    pub list_options: Option<bool>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTermParams {
    /// A vocabulary term ID: GO:0007219, FBbt:00004729, FBcv:0000013, FBdv:00005289, DOID:1289, SO:0000704.
    pub term_id: String,
    /// Children to skip (the children list is paged 100 at a time); offset + 100 may not exceed 10,000.
    pub offset: Option<u32>,
}

#[tool_router(router = tool_router_vocabulary, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "search_vocabulary",
        description = "Search FlyBase controlled vocabularies (GO, anatomy FBbt, development FBdv, disease DOID, sequence SO, miscellaneous FBcv, PSI-MI) by term name or synonym. Returns term IDs for get_term and entities_for_term. Use when you need the ID of a biological concept rather than a gene or allele."
    )]
    pub async fn search_vocabulary(
        &self,
        Parameters(p): Parameters<SearchVocabularyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "search_vocabulary";
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = self.limits.light(T).await?;
                    self.search_vocabulary_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn search_vocabulary_impl(
        &self,
        p: SearchVocabularyParams,
    ) -> ToolResult<CallToolResult> {
        const T: &str = "search_vocabulary";
        if p.list_options.unwrap_or(false) {
            let vocabs: Vec<Value> = Vocabulary::ALL
                .iter()
                .map(|v| json!({"name": v.name(), "cv_names": v.cv_names()}))
                .collect();
            return Ok(Envelope::new(T, &self.release)
                .summary("Vocabularies you can pass in `vocabularies`.")
                .field("vocabularies", json!(vocabs))
                .finish());
        }
        let q = p.query.trim().to_string();
        if q.chars().count() < 2 || q.len() > 200 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "query",
                message: "must be 2 to 200 characters".into(),
            });
        }
        let limit = clamp_limit(p.limit);
        let cvs: Option<Vec<String>> = p.vocabularies.filter(|v| !v.is_empty()).map(|v| {
            v.iter()
                .flat_map(|x| x.cv_names().iter().map(|s| s.to_string()))
                .collect()
        });
        let escaped = like_escape(&q);
        // No alphanumeric character means no trigrams and a sequential scan; answer with no hits instead.
        let rows = if q.chars().any(char::is_alphanumeric) {
            self.chado
                .query(T, "vocab/search", &[&q, &escaped, &cvs, &(limit + 1)])
                .await?
        } else {
            Vec::new()
        };
        let has_more = rows.len() as i64 > limit;
        let terms: Vec<Value> = rows
            .iter()
            .take(limit as usize)
            .map(|r| {
                let id: String = r.get("term_id");
                json!({
                    "id": id,
                    "name": r.get::<_, String>("name"),
                    "vocabulary": r.get::<_, String>("vocabulary"),
                    "definition": r.get::<_, Option<String>>("definition"),
                    "synonyms": r.get::<_, Option<String>>("synonyms"),
                    "match_kind": r.get::<_, String>("match_kind"),
                    "link": cv_term_link(&id),
                })
            })
            .collect();
        let summary = match terms.first() {
            None => format!("No vocabulary terms match '{q}'."),
            Some(t) => format!(
                "{} term(s) for '{q}'{}; first: {} ({}).",
                terms.len(),
                if has_more {
                    ", more available with a narrower query"
                } else {
                    ""
                },
                t["id"].as_str().unwrap_or(""),
                t["name"].as_str().unwrap_or("")
            ),
        };
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .field("terms", json!(terms))
            .field("has_more", json!(has_more));
        if let Some(t) = terms.first() {
            env = env.link("first_term", cv_term_link(t["id"].as_str().unwrap_or("")));
        }
        Ok(env.finish())
    }

    #[tool(
        name = "get_term",
        description = "Full report for one vocabulary term ID (GO:..., FBbt:..., FBcv:..., FBdv:..., DOID:..., SO:..., MI:...): definition, synonyms, parents, children (100 per page; page with offset), and annotation counts by data class. Use search_vocabulary first if you only have a term name; use entities_for_term to list the annotated genes or alleles."
    )]
    pub async fn get_term(
        &self,
        Parameters(p): Parameters<GetTermParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_term";
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = self.limits.light(T).await?;
                    self.get_term_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    /// All five sections run concurrently on one connection; the term row is fetched once (the `OnceCell` in
    /// `CvTerm`).
    async fn get_term_impl(&self, p: GetTermParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_term";
        let input = p.term_id.trim();
        let id = canonical_term_id(input).ok_or_else(|| ToolError::InvalidInput {
            tool: T,
            arg: "term_id",
            message: format!(
                "'{input}' is not a vocabulary term ID such as GO:0007219 or FBbt:00004729"
            ),
        })?;
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + 100 > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + 100 may not exceed {MAX_OFFSET}"),
            });
        }
        let ctx = SectionCtx {
            conn: self.chado.get(T).await?,
            tool: T,
            id: id.clone(),
            limit: 100,
            fetch: 101,
            offset,
        };
        let rt = CvTerm::default();
        let names: Vec<&str> = rt
            .default_sections()
            .iter()
            .chain(rt.optional_sections())
            .copied()
            .collect();
        let results = futures::future::join_all(names.iter().map(|n| rt.section(&ctx, n))).await;
        // Same policy as get_report: NotFound (the term does not exist) aborts; any other section failure becomes an
        // error object so a slow annotation_counts cannot sink the definition.
        let mut sections = Map::new();
        let mut section_errors: Vec<String> = Vec::new();
        for (name, res) in names.iter().zip(results) {
            match res {
                Ok(v) => {
                    sections.insert(name.to_string(), v);
                }
                Err(e @ ToolError::NotFound { .. }) | Err(e @ ToolError::InvalidInput { .. }) => {
                    return Err(e);
                }
                Err(e) => {
                    tracing::warn!(tool = T, id = %id, section = name, error = %e, "section failed");
                    sections.insert(name.to_string(), section_error(&e));
                    section_errors.push(name.to_string());
                }
            }
        }
        let label = sections["definition"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let failed = if section_errors.is_empty() {
            String::new()
        } else {
            format!(" Failed sections: {}.", section_errors.join(", "))
        };
        let mut env = Envelope::new(T, &self.release)
            .summary(format!(
                "{id} ({label}): definition, synonyms, parents, children, annotation counts.{failed}"
            ))
            .link("report", cv_term_link(&id))
            .field("id", json!(id))
            .field("type", json!("cv_term"))
            .field("offset", json!(offset))
            .field("sections", Value::Object(sections));
        if !section_errors.is_empty() {
            env = env.field("section_errors", json!(section_errors));
        }
        Ok(env.finish())
    }
}
