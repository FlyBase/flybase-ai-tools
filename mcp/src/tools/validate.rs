use crate::{
    backends::{chado::Chado, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::MAX_BATCH,
    output::{Envelope, complete},
    server::FlyBaseServer,
    tools::resolve::{DataClass, class_of, looks_like_fb_id, normalize_fb_prefix, resolve_ids},
};
use deadpool_postgres::Object;
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateParams {
    /// Up to 500 inputs: FlyBase IDs (current, secondary, or obsolete), exact symbols or full names, or external IDs
    /// (UniProt, GenBank, RefSeq, Entrez Gene numbers, HGNC:1234, PubMed numbers).
    pub ids: Vec<String>,
    /// Keep only matches of this data class (for example gene when a UniProt accession also maps to polypeptides).
    pub target_class: Option<DataClass>,
    /// Species for symbol matching (default any).
    pub species: Option<String>,
}

#[derive(Default)]
struct Hit {
    ids: Vec<String>,
    /// Symbol per matched ID, so the reported symbol is the one of the first ID left after the class filter.
    symbols: BTreeMap<String, Option<String>>,
    status: &'static str,
    matched_by: &'static str,
    note: Option<String>,
}

fn push_unique(v: &mut Vec<String>, id: String) {
    if !v.contains(&id) {
        v.push(id);
    }
}

#[tool_router(router = tool_router_validate, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "validate_ids",
        description = "Validate and update a batch of up to 500 identifiers at once: FlyBase IDs (reports secondary, split, and obsolete IDs with their current replacements), exact symbols or names, and external IDs (UniProt, GenBank, RefSeq, Entrez Gene, HGNC:n, PubMed). Use for lists of exact inputs; use resolve for one fuzzy name."
    )]
    pub async fn validate_ids(
        &self,
        Parameters(p): Parameters<ValidateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "validate_ids";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.validate_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn validate_impl(&self, p: ValidateParams) -> ToolResult<CallToolResult> {
        const T: &str = "validate_ids";
        if p.ids.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "ids",
                message: "provide at least one identifier".into(),
            });
        }
        if p.ids.len() > MAX_BATCH {
            return Err(ToolError::LimitExceeded {
                tool: T,
                message: format!(
                    "{MAX_BATCH} IDs max per call ({} given); split the list",
                    p.ids.len()
                ),
            });
        }
        let species = match &p.species {
            Some(s) => Some(normalize_species(&self.chado, T, s).await?),
            None => None,
        };
        // One result per input, in input order; empty or over-long inputs are kept with status "invalid".
        let inputs: Vec<String> = p.ids.iter().map(|s| s.trim().to_string()).collect();
        let usable = |s: &String| !s.is_empty() && s.len() <= 200;
        // Partition: FlyBase-ID-shaped inputs go to resolve_ids; everything else is tried as a symbol, stock number, and external ID.
        let mut fb: Vec<String> = inputs
            .iter()
            .filter(|s| usable(s) && looks_like_fb_id(&normalize_fb_prefix(s)))
            .map(|s| normalize_fb_prefix(s).into_owned())
            .collect();
        fb.sort();
        fb.dedup();
        let mut other: Vec<String> = inputs
            .iter()
            .filter(|s| usable(s) && !looks_like_fb_id(&normalize_fb_prefix(s)))
            .cloned()
            .collect();
        other.sort();
        other.dedup();
        let mut hits: BTreeMap<String, Hit> = BTreeMap::new();
        if !fb.is_empty() {
            for st in resolve_ids(&self.chado, T, &fb).await? {
                let note = match st.status {
                    "updated" => Some("secondary ID; use the replacement".to_string()),
                    "split" => Some("split into several current IDs".to_string()),
                    "obsolete" => Some("obsolete with no replacement".to_string()),
                    _ => None,
                };
                hits.insert(
                    st.input.clone(),
                    Hit {
                        symbols: st
                            .current_ids
                            .first()
                            .map(|id| (id.clone(), st.symbol.clone()))
                            .into_iter()
                            .collect(),
                        ids: st.current_ids,
                        status: st.status,
                        matched_by: if st.status == "unknown" {
                            "none"
                        } else {
                            "flybase_id"
                        },
                        note,
                    },
                );
            }
        }
        if !other.is_empty() {
            let conn = self.chado.get(T).await?;
            self.lookup_names(&conn, T, &other, species.as_deref(), &mut hits)
                .await?;
        }
        let results: Vec<Value> = inputs
            .iter()
            .map(|input| {
                if !usable(input) {
                    return json!({"input": input, "current_ids": [], "symbol": Value::Null, "data_class": Value::Null, "status": "invalid", "matched_by": "none", "note": "empty or longer than 200 characters"});
                }
                let key = normalize_fb_prefix(input).into_owned();
                // Hits are looked up, not removed, so a duplicated input gets the same answer twice.
                let empty = Hit { status: "unknown", matched_by: "none", ..Hit::default() };
                let h = hits.get(&key).or_else(|| hits.get(input)).unwrap_or(&empty);
                let ids: Vec<&String> = h
                    .ids
                    .iter()
                    .filter(|id| p.target_class.is_none_or(|c| class_of(id) == c.as_str()))
                    .collect();
                // The class filter can empty a hit or narrow an ambiguous one to a single match; status and note follow.
                let narrowed = ids.len() < h.ids.len();
                let (status, note) = if ids.is_empty() && !h.ids.is_empty() {
                    ("unknown", Some(format!("matches exist but none of class {}", p.target_class.map(|c| c.as_str()).unwrap_or(""))))
                } else if h.status == "ambiguous" && ids.len() == 1 {
                    ("matched", Some(format!("{} entities matched; target_class kept one", h.ids.len())))
                } else if narrowed && h.status == "ambiguous" {
                    ("ambiguous", Some(format!("{} entities match after the class filter; pick one", ids.len())))
                } else {
                    (h.status, h.note.clone())
                };
                let symbol = ids.first().and_then(|id| h.symbols.get(*id).cloned().flatten());
                json!({
                    "input": input, "current_ids": ids, "symbol": symbol,
                    "data_class": ids.first().map(|id| class_of(id)), "status": status, "matched_by": if ids.is_empty() { "none" } else { h.matched_by },
                    "note": note,
                })
            })
            .collect();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &results {
            *counts
                .entry(r["status"].as_str().unwrap_or("unknown"))
                .or_default() += 1;
        }
        let summary = format!(
            "{} input(s): {}.",
            results.len(),
            counts
                .iter()
                .map(|(k, v)| format!("{v} {k}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .field("results", json!(results))
            .field("counts", json!(counts))
            .finish())
    }

    /// Symbols and names through the name index, external accessions through feature_dbxref, PubMed IDs through pub_dbxref.
    async fn lookup_names(
        &self,
        conn: &Object,
        t: &'static str,
        inputs: &[String],
        species: Option<&str>,
        hits: &mut BTreeMap<String, Hit>,
    ) -> ToolResult<()> {
        let rows = Chado::query_on(conn, t, "validate/symbols", &[&inputs, &species]).await?;
        // Tiers per input: exact-case symbol or stock number, any-case symbol, exact-case full name, any-case full name.
        // Only the best tier counts, so "N" is Notch alone even though an allele's full name is "n".
        let tier = |input: &str, matched: &str, kind: &str| -> u8 {
            match (matched == input, kind == "symbol" || kind == "stock_number") {
                (true, true) => 0,
                (false, true) => 1,
                (true, false) => 2,
                (false, false) => 3,
            }
        };
        let mut best: BTreeMap<String, u8> = BTreeMap::new();
        for r in &rows {
            let input: String = r.get("input");
            let tr = tier(
                &input,
                r.get::<_, &str>("matched"),
                r.get::<_, &str>("name_kind"),
            );
            best.entry(input)
                .and_modify(|b| *b = (*b).min(tr))
                .or_insert(tr);
        }
        for r in &rows {
            let input: String = r.get("input");
            if tier(
                &input,
                r.get::<_, &str>("matched"),
                r.get::<_, &str>("name_kind"),
            ) != best[&input]
            {
                continue;
            }
            let matched_by = if r.get::<_, &str>("name_kind") == "stock_number" {
                "stock_number"
            } else {
                "symbol"
            };
            let h = hits.entry(input).or_insert_with(|| Hit {
                status: "matched",
                matched_by,
                ..Hit::default()
            });
            let id: String = r.get("entity_id");
            h.symbols.entry(id.clone()).or_insert_with(|| {
                r.get::<_, Option<String>>("current_symbol")
                    .or_else(|| r.get("matched"))
            });
            push_unique(&mut h.ids, id);
        }
        // HGNC:7881 is looked up as accession 7881 in the HGNC db only; a bare number is an Entrez Gene ID (and, below, a PubMed ID).
        let accessions: Vec<String> = inputs
            .iter()
            .map(|s| s.strip_prefix("HGNC:").unwrap_or(s).to_string())
            .collect();
        let rows = Chado::query_on(conn, t, "validate/external", &[&accessions]).await?;
        for r in &rows {
            let acc: String = r.get("input");
            let db: String = r.get("db");
            let original = if db == "HGNC" {
                format!("HGNC:{acc}")
            } else {
                acc.clone()
            };
            if !inputs.contains(&original) {
                continue;
            }
            let h = hits.entry(original).or_insert_with(|| Hit {
                status: "matched",
                matched_by: "external_id",
                ..Hit::default()
            });
            if h.matched_by == "symbol" || h.matched_by == "stock_number" {
                continue; // an exact symbol or stock number wins over an accidental accession match
            }
            let id: String = r.get("uniquename");
            h.symbols
                .entry(id.clone())
                .or_insert_with(|| r.get("symbol"));
            push_unique(&mut h.ids, id);
            h.note = Some(format!("via {db}"));
        }
        let numbers: Vec<String> = inputs
            .iter()
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
            .cloned()
            .collect();
        if !numbers.is_empty() {
            let rows = Chado::query_on(conn, t, "validate/pubmed", &[&numbers]).await?;
            for r in &rows {
                let input: String = r.get("input");
                let h = hits.entry(input).or_insert_with(|| Hit {
                    status: "matched",
                    matched_by: "pubmed_id",
                    ..Hit::default()
                });
                if h.matched_by == "pubmed_id" {
                    let id: String = r.get("uniquename");
                    h.symbols.entry(id.clone()).or_insert(None);
                    push_unique(&mut h.ids, id);
                    h.note = r.get::<_, Option<String>>("miniref");
                }
            }
        }
        for h in hits.values_mut() {
            if h.status == "matched" && h.ids.len() > 1 {
                h.status = "ambiguous";
                h.note = Some(format!("{} entities match; pick one", h.ids.len()));
            }
        }
        Ok(())
    }
}
