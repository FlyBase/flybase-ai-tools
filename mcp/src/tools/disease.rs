use crate::backends::cache::ResponseCache;
use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, cv_term_link, report_link},
    report::{
        ReportType, SectionCtx,
        cvterm::{MAX_DESCENDANTS, parse_term_id},
        disease::{Disease, variant_row},
        paged, rows_to_json, strip_markup,
    },
    server::FlyBaseServer,
    tools::{
        entities::MAX_HITS,
        resolve::{normalize_fb_prefix, resolve_ids},
        search::MAX_OFFSET,
    },
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiseaseParams {
    /// A gene (FBgn), allele (FBal), human disease model entry (FBhh), or Disease Ontology term (DOID:162).
    pub id: String,
    /// Items per paged list (default 25, max 100).
    pub limit: Option<u32>,
    /// Items to skip in paged lists.
    pub offset: Option<u32>,
}

/// One annotation (disease/allele_annotations.sql) in the shape shared with the DOID branch: one item per
/// annotation, so `reference` is set and `reference_count` is 1; a negated annotation's qualifiers start with `NOT`.
fn annotation_row(r: &tokio_postgres::Row) -> Value {
    let id: String = r.get("allele_id");
    let qualifiers = match (
        r.get::<_, bool>("is_not"),
        r.get::<_, Option<String>>("qualifier"),
    ) {
        (true, Some(q)) => Some(format!("NOT {q}")),
        (true, None) => Some("NOT".to_string()),
        (false, q) => q,
    };
    json!({
        "allele_id": id, "allele_symbol": r.get::<_, Option<String>>("allele_symbol").map(|s| strip_markup(&s)),
        "term_id": r.get::<_, String>("term_id"), "disease": r.get::<_, String>("disease"), "qualifiers": qualifiers,
        "evidence": r.get::<_, Option<String>>("evidence").map(|s| strip_markup(&s)),
        "reference": r.get::<_, String>("reference"), "reference_count": 1, "link": report_link(&id),
    })
}

#[tool_router(router = tool_router_disease, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_disease_models",
        description = "Human disease models: for a gene or allele, its Disease Ontology annotations with qualifiers (model of, ameliorates, exacerbates), evidence, and references, plus disease-variant alleles (first page only) and the FlyBase disease model entries (FBhh) that cite it; for an FBhh entry, its human gene, DO terms, fly models, and variants; for a DOID term, the entries and alleles annotated to it or its descendants. Use for disease questions; use get_orthologs for the human gene itself."
    )]
    pub async fn get_disease_models(
        &self,
        Parameters(p): Parameters<DiseaseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_disease_models";
        let started = std::time::Instant::now();
        // A DOID query walks the term's descendants like entities_for_term and runs under the heavy permit; everything
        // else (including a non-DOID term ID, which is refused) is a few indexed lookups under the light permit.
        let is_term =
            parse_term_id(&normalize_fb_prefix(p.id.trim())).is_some_and(|(db, _)| db == "DOID");
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = if is_term {
                        self.limits.heavy(T).await?
                    } else {
                        self.limits.light(T).await?
                    };
                    self.disease_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn disease_impl(&self, p: DiseaseParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_disease_models";
        let input = normalize_fb_prefix(p.id.trim()).into_owned();
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
        if let Some((db, acc)) = parse_term_id(&input) {
            if db != "DOID" {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "id",
                    message: format!("'{input}' is a {db} term; disease terms are DOID:n"),
                });
            }
            return self.disease_term(T, db, acc, limit, fetch, offset).await;
        }
        if !(input.starts_with("FBgn") || input.starts_with("FBal") || input.starts_with("FBhh"))
            || input.len() != 11
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: format!("'{input}' must be an FBgn, FBal, FBhh, or DOID:n"),
            });
        }
        let st = resolve_ids(&self.chado, T, std::slice::from_ref(&input))
            .await?
            .remove(0);
        let id = match st.status {
            "current" | "updated" => st.current_ids[0].clone(),
            _ => {
                return Err(ToolError::NotFound {
                    tool: T,
                    message: format!("{input} is not a current ID; use resolve"),
                });
            }
        };
        let ctx = SectionCtx {
            conn: self.chado.get(T).await?,
            tool: T,
            id: id.clone(),
            limit,
            fetch,
            offset: 0,
        };
        let symbol = st
            .symbol
            .clone()
            .map(|s| strip_markup(&s))
            .unwrap_or_default();
        if id.starts_with("FBhh") {
            let rt = Disease;
            let mut sections = Map::new();
            for name in [
                "identity",
                "human_genes",
                "do_terms",
                "fly_models",
                "variants",
            ] {
                sections.insert(name.to_string(), rt.section(&ctx, name).await?);
            }
            return Ok(Envelope::new(T, &self.release)
                .summary(format!(
                    "{id} ({}): {} fly model gene(s), {} variant allele(s) shown.",
                    sections["identity"]["name"].as_str().unwrap_or(""),
                    sections["fly_models"].as_array().map_or(0, Vec::len),
                    sections["variants"]["count_shown"]
                ))
                .link("report", report_link(&id))
                .field("id", json!(id))
                .field("type", json!("disease_model"))
                .field("sections", Value::Object(sections))
                .finish());
        }
        let is_gene = id.starts_with("FBgn");
        let mut env = Envelope::new(T, &self.release)
            .link("report", report_link(&id))
            .field("id", json!(id))
            .field("symbol", json!(symbol))
            .field("offset", json!(offset));
        let mut parts: Vec<String> = Vec::new();
        if is_gene {
            let r = ctx.query("disease/gene_models", &[&id]).await?;
            let assoc = rows_to_json(&r, |r| {
                json!({
                    "term_id": r.get::<_, String>("term_id"), "disease": r.get::<_, Option<String>>("disease"),
                    "is_experimental": r.get::<_, Option<bool>>("is_experimental"), "qualifier": r.get::<_, Option<String>>("qualifier"),
                })
            });
            parts.push(format!("{} disease association(s)", assoc.len()));
            env = env.field("disease_associations", json!(assoc));
        }
        let r = ctx
            .query("disease/allele_annotations", &[&id, &fetch, &offset])
            .await?;
        let ann = paged(rows_to_json(&r, annotation_row), limit);
        parts.push(format!(
            "{} allele annotation(s){}",
            ann["count_shown"],
            if ann["has_more"] == true {
                " (more with offset)"
            } else {
                ""
            }
        ));
        env = env.field("allele_annotations", ann);
        // disease/variants.sql takes a fetch count without an offset: the list is the first page (limit items,
        // has_more when there are more), as in the FBhh report's `variants` section.
        let r = ctx.query("disease/variants", &[&id, &fetch]).await?;
        let variants = paged(
            r.iter()
                .map(|r| variant_row(T, r))
                .collect::<ToolResult<Vec<_>>>()?,
            limit,
        );
        parts.push(format!(
            "{} disease variant allele(s){}",
            variants["count_shown"],
            if variants["has_more"] == true {
                " (first page)"
            } else {
                ""
            }
        ));
        env = env.field("disease_variants", variants);
        let r = ctx.query("disease/gene_fbhh", &[&id]).await?;
        let entries = rows_to_json(&r, |r| {
            let hid: String = r.get("id");
            json!({"id": hid, "name": r.get::<_, Option<String>>("name"), "role": r.get::<_, Option<String>>("role"), "link": report_link(&hid)})
        });
        parts.push(format!("{} disease model entr(ies)", entries.len()));
        env = env.field("disease_model_entries", json!(entries));
        Ok(env
            .summary(format!("{id} ({symbol}): {}.", parts.join(", ")))
            .finish())
    }

    /// DOID branch: the term, the FBhh entries annotated to it or its descendants, and the alleles annotated to it
    /// or its descendants (term/alleles_annotated.sql), inside one heavy transaction with the descendant guard.
    async fn disease_term(
        &self,
        t: &'static str,
        db: &'static str,
        acc: &str,
        limit: i64,
        fetch: i64,
        offset: i64,
    ) -> ToolResult<CallToolResult> {
        let term_id = format!("DOID:{acc}");
        let (term_json, entries, alleles) = self
            .chado
            .heavy(t, async |tx| {
                let term = Chado::query_tx(tx, t, "report/cvterm/term", &[&db, &acc]).await?;
                let term = term.first().ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{term_id} is not a Disease Ontology term"),
                })?;
                let cvterm_id: i32 = term.get("cvterm_id");
                let d =
                    Chado::query_tx(tx, t, "report/cvterm/descendant_count", &[&cvterm_id]).await?;
                let descendants = d.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0);
                if descendants > MAX_DESCENDANTS {
                    return Err(ToolError::LimitExceeded {
                        tool: t,
                        message: format!(
                            "{term_id} has {descendants} descendant terms (max {MAX_DESCENDANTS}); choose a narrower disease term"
                        ),
                    });
                }
                let entries =
                    Chado::query_tx(tx, t, "disease/term_models", &[&acc, &fetch, &offset]).await?;
                // term/alleles_annotated caps its scan at MAX_HITS + 1 rows and returns a null-id sentinel row for
                // an empty page (see entities_for_term).
                let mut alleles = Chado::query_tx(
                    tx,
                    t,
                    "term/alleles_annotated",
                    &[&cvterm_id, &true, &None::<String>, &fetch, &offset],
                )
                .await?;
                if alleles
                    .iter()
                    .any(|r| r.get::<_, i64>("hits") > MAX_HITS)
                {
                    return Err(ToolError::LimitExceeded {
                        tool: t,
                        message: format!(
                            "{term_id} has more than {MAX_HITS} allele annotations; choose a narrower disease term"
                        ),
                    });
                }
                alleles.retain(|r| r.get::<_, Option<String>>("id").is_some());
                let term_json = json!({
                    "id": term_id, "name": term.get::<_, String>("name"),
                    "definition": term.get::<_, Option<String>>("definition"), "descendants": descendants,
                });
                Ok((term_json, entries, alleles))
            })
            .await?;
        let entries_json = paged(
            rows_to_json(&entries, |r| {
                let id: String = r.get("id");
                json!({"id": id, "name": r.get::<_, Option<String>>("name"), "link": report_link(&id)})
            }),
            limit,
        );
        let alleles_json = paged(
            rows_to_json(&alleles, |r| {
                let id: String = r.get("id");
                json!({
                    "allele_id": id, "allele_symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                    "term_id": term_json["id"], "disease": term_json["name"],
                    "qualifiers": r.get::<_, Option<String>>("qualifiers"),
                    "evidence": r.get::<_, Option<String>>("evidence").map(|s| strip_markup(&s)),
                    "reference": Value::Null, "reference_count": r.get::<_, i64>("reference_count"), "link": report_link(&id),
                })
            }),
            limit,
        );
        let term_label = term_json["id"].as_str().unwrap_or("").to_string();
        Ok(Envelope::new(t, &self.release)
            .summary(format!(
                "{} ({}): {} disease model entr(ies) and {} annotated allele(s) shown (term and {} descendants).",
                term_label,
                term_json["name"].as_str().unwrap_or(""),
                entries_json["count_shown"],
                alleles_json["count_shown"],
                term_json["descendants"]
            ))
            .link("term", cv_term_link(&term_label))
            .field("term", term_json)
            .field("disease_model_entries", entries_json["items"].clone())
            .field(
                "disease_model_entries_has_more",
                entries_json["has_more"].clone(),
            )
            .field("allele_annotations", alleles_json)
            .field("offset", json!(offset))
            .finish())
    }
}
