use crate::backends::cache::ResponseCache;
use crate::{
    backends::{chado::Chado, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::{MAX_BATCH, MAX_JSON_BYTES},
    output::{Envelope, complete},
    server::FlyBaseServer,
    tools::resolve::{DataClass, class_of, looks_like_fb_id, normalize_fb_prefix, resolve_ids},
};
use deadpool_postgres::Object;
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, btree_map::Entry};
use tokio_postgres::{Row, types::ToSql};

const MAX_VALIDATE_IDS: usize = 50_000;
const MAX_PAGE_ROWS: usize = 1_000;
const MAX_CANDIDATE_ROWS: i64 = 25_000;

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateParams {
    /// Up to 50,000 inputs in output order. Resend the same list for each page (the HTTP request limit is 1 MiB).
    /// Supports FlyBase IDs, exact symbols or full names, and external IDs.
    #[schemars(length(min = 1, max = 50000))]
    pub ids: Vec<String>,
    /// Keep only matches of this data class (for example gene when a UniProt accession also maps to polypeptides).
    pub target_class: Option<DataClass>,
    /// Species for symbol matching (default any).
    pub species: Option<String>,
    /// Zero-based index of the first input to return. Use next_offset from the previous response.
    pub offset: Option<usize>,
    /// Maximum results per page (1 to 1,000). Large results can reduce the number that fit.
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
struct ValidatePage<'a> {
    inputs: &'a [String],
    target_class: Option<DataClass>,
    species: Option<&'a str>,
    offset: usize,
    total: usize,
    paged: bool,
}

impl ValidateParams {
    fn page(&self, tool: &'static str) -> ToolResult<ValidatePage<'_>> {
        let total = self.ids.len();
        if total == 0 {
            return Err(ToolError::InvalidInput {
                tool,
                arg: "ids",
                message: "provide at least one identifier".into(),
            });
        }
        if total > MAX_VALIDATE_IDS {
            return Err(ToolError::LimitExceeded {
                tool,
                message: format!(
                    "{MAX_VALIDATE_IDS} IDs max per call ({total} given); split the list"
                ),
            });
        }
        let offset = self.offset.unwrap_or(0);
        let limit = self.limit.unwrap_or(MAX_PAGE_ROWS);
        if offset >= total {
            return Err(ToolError::InvalidInput {
                tool,
                arg: "offset",
                message: "must identify a row in the input list".into(),
            });
        }
        if !(1..=MAX_PAGE_ROWS).contains(&limit) {
            return Err(ToolError::InvalidInput {
                tool,
                arg: "limit",
                message: format!("must be between 1 and {MAX_PAGE_ROWS}"),
            });
        }
        Ok(ValidatePage {
            inputs: &self.ids[offset..(offset + limit).min(total)],
            target_class: self.target_class,
            species: self.species.as_deref(),
            offset,
            total,
            paged: total > MAX_BATCH || self.offset.is_some() || self.limit.is_some(),
        })
    }
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

impl Hit {
    fn push(&mut self, id: String, symbol: impl FnOnce() -> Option<String>) {
        if let Entry::Vacant(entry) = self.symbols.entry(id.clone()) {
            entry.insert(symbol());
            self.ids.push(id);
        }
    }
}

async fn lookup_candidates(
    conn: &Object,
    tool: &'static str,
    query: &str,
    params: &[&(dyn ToSql + Sync)],
) -> ToolResult<Vec<Row>> {
    let fetch = MAX_CANDIDATE_ROWS + 1;
    let mut params = params.to_vec();
    params.push(&fetch);
    let rows = Chado::query_on(conn, tool, query, &params).await?;
    if rows.len() as i64 > MAX_CANDIDATE_ROWS {
        return Err(ToolError::LimitExceeded {
            tool,
            message: "too many candidate matches for this page; retry at the same offset with a smaller limit".into(),
        });
    }
    Ok(rows)
}

#[tool_router(router = tool_router_validate, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "validate_ids",
        description = "Validate and update up to 50,000 identifiers: FlyBase IDs (including secondary, split, and obsolete IDs), exact symbols or names, and external IDs (UniProt, GenBank, RefSeq, Entrez Gene, HGNC:n, PubMed). Secondary IDs are known for feature classes such as FBgn and FBal only, not FBgg or FBhh. Returns one result per input in order, including duplicates. Lists over 500 inputs are paged. Repeat with the same arguments and offset=next_offset until next_offset is null. Counts cover the returned page. Use resolve for one fuzzy name."
    )]
    pub async fn validate_ids(
        &self,
        Parameters(p): Parameters<ValidateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "validate_ids";
        let started = std::time::Instant::now();
        let res = async {
            let page = p.page(T)?;
            let key = ResponseCache::key(T, &page);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = self.limits.light(T).await?;
                    self.validate_impl(page).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn validate_impl(&self, page: ValidatePage<'_>) -> ToolResult<CallToolResult> {
        const T: &str = "validate_ids";
        let species = match page.species {
            Some(s) => Some(normalize_species(&self.chado, T, s).await?),
            None => None,
        };
        // One result per input, in input order; empty or over-long inputs are kept with status "invalid".
        let inputs: Vec<String> = page.inputs.iter().map(|s| s.trim().to_string()).collect();
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
        let rows = inputs
            .iter()
            .map(|input| {
                if !usable(input) {
                    return ("invalid", json!({"input": input, "current_ids": [], "symbol": Value::Null, "data_class": Value::Null, "status": "invalid", "matched_by": "none", "note": "empty or longer than 200 characters"}));
                }
                let key = normalize_fb_prefix(input).into_owned();
                // Hits are looked up, not removed, so a duplicated input gets the same answer twice.
                let empty = Hit { status: "unknown", matched_by: "none", ..Hit::default() };
                let h = hits.get(&key).or_else(|| hits.get(input)).unwrap_or(&empty);
                let ids: Vec<&String> = h
                    .ids
                    .iter()
                    .filter(|id| page.target_class.is_none_or(|c| class_of(id) == c.as_str()))
                    .collect();
                // The class filter can empty a hit or narrow an ambiguous one to a single match; status and note follow.
                let narrowed = ids.len() < h.ids.len();
                let (status, note) = if ids.is_empty() && !h.ids.is_empty() {
                    ("unknown", Some(format!("matches exist but none of class {}", page.target_class.map(|c| c.as_str()).unwrap_or(""))))
                } else if h.status == "ambiguous" && ids.len() == 1 {
                    ("matched", Some(format!("{} entities matched; target_class kept one", h.ids.len())))
                } else if narrowed && h.status == "ambiguous" {
                    ("ambiguous", Some(format!("{} entities match after the class filter; pick one", ids.len())))
                } else {
                    (h.status, h.note.clone())
                };
                let symbol = ids.first().and_then(|id| h.symbols.get(*id).cloned().flatten());
                let row = json!({
                    "input": input, "current_ids": ids, "symbol": symbol,
                    "data_class": ids.first().map(|id| class_of(id)), "status": status, "matched_by": if ids.is_empty() { "none" } else { h.matched_by },
                    "note": note,
                });
                (status, row)
            });
        // Leave room for envelope and page metadata. A result always stays together with all its candidates.
        let mut bytes = 1_024 + json!(self.release).to_string().len();
        let mut results = Vec::with_capacity(inputs.len());
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for (status, row) in rows {
            bytes += row.to_string().len() + 1;
            if bytes > MAX_JSON_BYTES {
                if results.is_empty() {
                    return Err(ToolError::LimitExceeded {
                        tool: T,
                        message: format!(
                            "the result at offset {} exceeds the response size limit",
                            page.offset
                        ),
                    });
                }
                break;
            }
            results.push(row);
            *counts.entry(status).or_default() += 1;
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
        let next = page.offset + results.len();
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .field("results", Value::Array(results))
            .field("counts", json!(counts));
        if page.paged || next < page.total {
            env = env
                .field("total", json!(page.total))
                .field("offset", json!(page.offset))
                .field("next_offset", json!((next < page.total).then_some(next)));
        }
        Ok(env.finish())
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
        let rows = lookup_candidates(conn, t, "validate/symbols", &[&inputs, &species]).await?;
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
        for r in rows {
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
            h.push(r.get("entity_id"), || {
                r.get::<_, Option<String>>("current_symbol")
                    .or_else(|| r.get("matched"))
            });
        }
        // Symbols and stock numbers win over accessions, which win over PubMed IDs.
        let accessions: Vec<&str> = inputs
            .iter()
            .filter(|s| !hits.contains_key(*s))
            .map(String::as_str)
            .collect();
        if !accessions.is_empty() {
            let rows = lookup_candidates(conn, t, "validate/external", &[&accessions]).await?;
            for r in &rows {
                let h = hits.entry(r.get("input")).or_insert_with(|| Hit {
                    status: "matched",
                    matched_by: "external_id",
                    ..Hit::default()
                });
                h.push(r.get("uniquename"), || r.get("symbol"));
                let db: &str = r.get("db");
                h.note = Some(format!("via {db}"));
            }
        }
        let numbers: Vec<&str> = inputs
            .iter()
            .filter(|s| !hits.contains_key(*s) && s.bytes().all(|b| b.is_ascii_digit()))
            .map(String::as_str)
            .collect();
        if !numbers.is_empty() {
            let rows = lookup_candidates(conn, t, "validate/pubmed", &[&numbers]).await?;
            for r in &rows {
                let input: String = r.get("input");
                let h = hits.entry(input).or_insert_with(|| Hit {
                    status: "matched",
                    matched_by: "pubmed_id",
                    ..Hit::default()
                });
                h.push(r.get("uniquename"), || None);
                h.note = r.get("miniref");
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
