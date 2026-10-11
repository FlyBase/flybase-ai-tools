use crate::{
    backends::{chado::Chado, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::collections::HashSet;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
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
}

impl DataClass {
    pub fn as_str(self) -> &'static str {
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
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveParams {
    /// A symbol (N, w[1118]), full name (Notch), synonym, or any FlyBase ID (FBgn0004647, including obsolete or secondary IDs).
    pub query: String,
    /// FlyBase species abbreviation such as Dmel or Hsap. Default: any species.
    pub species: Option<String>,
    /// Restrict to one data class.
    pub data_class: Option<DataClass>,
    /// Max matches to return (default 25, max 100).
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: String,
    pub data_class: String,
    pub species: String,
    pub symbol: Option<String>,
    pub matched: String,
    pub name_kind: String,
    pub is_current: bool,
    pub sim: f32,
    pub score: i32,
}

#[derive(Debug, Serialize)]
pub struct IdStatus {
    pub input: String,
    pub current_ids: Vec<String>,
    pub symbol: Option<String>,
    pub status: &'static str,
}

/// FBxx + 7 digits (most classes) or FBog + 10 digits (ortholog features). ASCII only.
pub fn looks_like_fb_id(q: &str) -> bool {
    let b = q.as_bytes();
    (b.len() == 11 || (b.len() == 14 && q.starts_with("FBog")))
        && q.starts_with("FB")
        && b[2..4].iter().all(u8::is_ascii_lowercase)
        && b[4..].iter().all(u8::is_ascii_digit)
}

/// Canonicalize the prefix of FB-ID-shaped input ("fbgn0004647", "FBGN0004647" -> "FBgn0004647").
/// Anything that is not shaped like an ID is returned unchanged.
pub fn normalize_fb_prefix(q: &str) -> Cow<'_, str> {
    let b = q.as_bytes();
    let shaped = (b.len() == 11 || b.len() == 14)
        && b[..2].eq_ignore_ascii_case(b"FB")
        && b[2..4].iter().all(u8::is_ascii_alphabetic)
        && b[4..].iter().all(u8::is_ascii_digit);
    if !shaped {
        return Cow::Borrowed(q);
    }
    let canonical = format!("FB{}{}", q[2..4].to_ascii_lowercase(), &q[4..]);
    if canonical == q {
        Cow::Borrowed(q)
    } else {
        Cow::Owned(canonical)
    }
}

/// Escape LIKE wildcards (backslash, percent, underscore) so the query matches literally with `escape '\'`.
pub(crate) fn like_escape(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for ch in q.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub fn class_of(id: &str) -> &'static str {
    match id.get(..4).unwrap_or("") {
        "FBgn" => "gene",
        "FBal" => "allele",
        "FBti" => "insertion",
        "FBtp" => "construct",
        "FBab" => "aberration",
        "FBba" => "balancer",
        "FBst" => "stock",
        "FBrf" => "reference",
        "FBgg" => "gene_group",
        "FBhh" => "disease_model",
        "FBtr" => "transcript",
        "FBpp" => "polypeptide",
        "FBog" => "ortholog",
        "FBtc" => "cell_line",
        "FBsn" => "strain",
        "FBlc" => "library",
        "FBco" => "split_system_combination",
        "FBmc" => "engineered_plasmid",
        "FBsf" => "sequence_feature",
        "FBte" => "transposable_element",
        "FBcl" => "clone",
        "FBch" => "chemical",
        "FBcv" | "FBbt" | "FBdv" => "cv_term",
        _ => "unknown",
    }
}

/// Rank: exact symbol > exact name of any kind > symbol prefix > other prefix > trigram similarity; a current name outranks any stale one; Dmel preferred when species unspecified.
/// One entry per ID (best score wins, ties broken by matched name).
pub fn rank(query: &str, mut c: Vec<Candidate>, species: Option<&str>) -> Vec<Candidate> {
    let q = query.to_lowercase();
    for x in &mut c {
        let m = x.matched.to_lowercase();
        let mut s = if m == q && x.name_kind == "symbol" {
            1000
        } else if m == q {
            800
        } else if m.starts_with(&q) && x.name_kind == "symbol" {
            450
        } else if m.starts_with(&q) {
            400
        } else {
            (x.sim * 300.0) as i32
        };
        if x.is_current {
            s += 500;
        }
        if species.is_none() && x.species == "Dmel" {
            s += 50;
        }
        x.score = s;
    }
    c.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.id.cmp(&b.id))
            .then(a.matched.cmp(&b.matched))
    });
    let mut seen = HashSet::new();
    c.retain(|x| seen.insert(x.id.clone()));
    c
}

pub async fn resolve_ids(
    chado: &Chado,
    tool: &'static str,
    ids: &[String],
) -> ToolResult<Vec<IdStatus>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let rows = chado.query(tool, "resolve/id_status", &[id]).await?;
        let pick = |status: &str| -> Vec<(String, Option<String>)> {
            rows.iter()
                .filter(|r| r.get::<_, &str>("status") == status)
                .map(|r| (r.get("uniquename"), r.get("name")))
                .collect()
        };
        let current = pick("current");
        let secondary = pick("secondary");
        let obsolete = rows
            .iter()
            .any(|r| r.get::<_, &str>("status") == "obsolete");
        let (status, list) = if !current.is_empty() {
            ("current", current)
        } else if secondary.len() == 1 {
            ("updated", secondary)
        } else if secondary.len() > 1 {
            ("split", secondary)
        } else if obsolete {
            ("obsolete", vec![])
        } else {
            ("unknown", vec![])
        };
        out.push(IdStatus {
            input: id.clone(),
            symbol: list.first().and_then(|x| x.1.clone()),
            current_ids: list.into_iter().map(|x| x.0).collect(),
            status,
        });
    }
    Ok(out)
}

#[tool_router(router = tool_router_resolve, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "resolve",
        description = "Turn one gene, allele, insertion, stock, cell line, library, strain, or other symbol, name, synonym, stock number, or ID into current FlyBase IDs. Handles obsolete and secondary IDs. Use this first whenever you have a name and need an ID for another tool. For a batch of exact IDs use validate_ids; for free-text discovery use search."
    )]
    pub async fn resolve(
        &self,
        Parameters(p): Parameters<ResolveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "resolve";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.resolve_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn resolve_impl(&self, p: ResolveParams) -> ToolResult<CallToolResult> {
        const T: &str = "resolve";
        let q = p.query.trim();
        if q.is_empty() || q.len() > 200 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "query",
                message: "must be 1 to 200 characters".into(),
            });
        }
        let q = normalize_fb_prefix(q);
        let q = q.as_ref();
        let species = match &p.species {
            Some(s) => Some(normalize_species(&self.chado, T, s).await?),
            None => None,
        };
        let limit = clamp_limit(p.limit) as usize;
        let mut matches = Vec::new();
        if looks_like_fb_id(q) {
            let st = resolve_ids(&self.chado, T, &[q.to_string()])
                .await?
                .remove(0);
            match st.status {
                "unknown" => return Err(ToolError::NotFound { tool: T, message: format!("{q} is not a current, secondary, or obsolete FlyBase ID") }),
                "current" => matches.push(json!({"id": st.current_ids[0], "symbol": st.symbol, "status": "current", "data_class": class_of(&st.current_ids[0])})),
                "obsolete" => matches.push(json!({"id": q, "status": "obsolete", "replaced_by": [], "data_class": class_of(q)})),
                _ => matches.push(json!({"id": q, "status": st.status, "replaced_by": st.current_ids, "symbol": st.symbol, "data_class": class_of(q)})),
            }
        } else {
            let species = species.as_deref();
            let class = p.data_class.map(DataClass::as_str);
            let escaped = like_escape(q);
            let rows = self
                .chado
                .query(T, "resolve/candidates", &[&q, &species, &class, &escaped])
                .await?;
            let cands: Vec<Candidate> = rows
                .iter()
                .map(|r| Candidate {
                    id: r.get("entity_id"),
                    data_class: r.get("data_class"),
                    // Some names (e.g. cell lines) carry no organism; a NULL species reads as "".
                    species: r.get::<_, Option<String>>("species").unwrap_or_default(),
                    symbol: r.get("current_symbol"),
                    matched: r.get("name"),
                    name_kind: r.get("name_kind"),
                    is_current: r.get("is_current"),
                    sim: r.get("sim"),
                    score: 0,
                })
                .collect();
            if cands.is_empty() {
                return Err(ToolError::NotFound {
                    tool: T,
                    message: format!("no FlyBase entity matches '{q}'; try search for free text"),
                });
            }
            for c in rank(q, cands, species).into_iter().take(limit) {
                matches.push(json!({"id": c.id, "symbol": c.symbol, "data_class": c.data_class, "species": c.species, "status": "current", "matched": c.matched, "match_kind": c.name_kind, "score": c.score}));
            }
        }
        let top = &matches[0];
        let replaced: Vec<&str> = top["replaced_by"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        // Link the current entity: the replacement for an updated or split ID, else the match itself.
        let top_id = replaced
            .first()
            .copied()
            .or(top["id"].as_str())
            .unwrap_or("?")
            .to_string();
        let class = top["data_class"].as_str().unwrap_or("?");
        let symbol_note = top["symbol"]
            .as_str()
            .map(|s| format!(", symbol {s}"))
            .unwrap_or_default();
        let summary = match top["status"].as_str() {
            Some("updated") => {
                format!(
                    "'{q}' is a secondary ID; it was replaced by {top_id} ({class}{symbol_note})."
                )
            }
            Some("split") => format!(
                "'{q}' was split into {}. Linked the first; check each before use.",
                replaced.join(", ")
            ),
            Some("obsolete") => {
                format!("'{q}' is an obsolete FlyBase ID with no current replacement.")
            }
            _ => format!(
                "'{q}' resolves to {top_id} ({class}{symbol_note}). {} match(es).",
                matches.len()
            ),
        };
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .link("report", report_link(&top_id))
            .field("matches", json!(matches))
            .finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(id: &str, matched: &str, kind: &str, species: &str, current: bool, sim: f32) -> Candidate {
        Candidate {
            id: id.into(),
            data_class: "gene".into(),
            species: species.into(),
            symbol: None,
            matched: matched.into(),
            name_kind: kind.into(),
            is_current: current,
            sim,
            score: 0,
        }
    }
    #[test]
    fn current_fullname_beats_stale_symbol() {
        let r = rank(
            "notch",
            vec![
                c("FBgn_stale", "notch", "symbol", "Dmel", false, 1.0),
                c("FBgn_cur", "Notch", "fullname", "Dmel", true, 1.0),
            ],
            None,
        );
        assert_eq!(r[0].id, "FBgn_cur");
        assert_eq!(r[0].score, 1350);
        assert_eq!(r[1].score, 1050);
    }
    #[test]
    fn kept_candidate_per_id_is_deterministic() {
        let a = c("FBgn1", "Notch", "fullname", "Dmel", true, 1.0);
        let b = c("FBgn1", "notch", "fullname", "Dmel", true, 1.0);
        let x = rank("notch", vec![a.clone(), b.clone()], None);
        let y = rank("notch", vec![b, a], None);
        assert_eq!(x.len(), 1);
        assert_eq!(x[0].matched, y[0].matched);
        assert_eq!(x[0].matched, "Notch");
    }
    #[test]
    fn like_escape_quotes_wildcards() {
        assert_eq!(like_escape("w[1118]"), "w[1118]");
        assert_eq!(like_escape("50%_a\\b"), "50\\%\\_a\\\\b");
    }
    #[test]
    fn exact_symbol_beats_fullname_and_prefix() {
        let r = rank(
            "N",
            vec![
                c("FBgn0001", "Notch", "fullname", "Dmel", true, 0.3),
                c("FBgn0004647", "N", "symbol", "Dmel", true, 1.0),
                c("FBgn0002", "Nup", "symbol", "Dmel", true, 0.5),
            ],
            None,
        );
        assert_eq!(r[0].id, "FBgn0004647");
        assert_eq!(r[1].id, "FBgn0002");
    }
    #[test]
    fn dmel_bonus_only_when_species_unspecified() {
        let a = rank(
            "N",
            vec![
                c("FBgn_sim", "N", "symbol", "Dsim", true, 1.0),
                c("FBgn_mel", "N", "symbol", "Dmel", true, 1.0),
            ],
            None,
        );
        assert_eq!(a[0].id, "FBgn_mel");
        let b = rank(
            "N",
            vec![
                c("FBgn_sim", "N", "symbol", "Dsim", true, 1.0),
                c("FBgn_mel", "N", "symbol", "Dmel", true, 1.0),
            ],
            Some("Dsim"),
        );
        assert_eq!(b[0].score, b[1].score);
    }
    #[test]
    fn same_id_appears_once_with_best_score() {
        let r = rank(
            "notch",
            vec![
                c("FBgn0004647", "Notch", "fullname", "Dmel", true, 1.0),
                c("FBgn0004647", "N", "symbol", "Dmel", true, 0.2),
            ],
            None,
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].score, 1350);
    }
    #[test]
    fn fb_prefix_is_canonicalized_only_for_id_shaped_input() {
        assert_eq!(normalize_fb_prefix("fbgn0004647"), "FBgn0004647");
        assert_eq!(normalize_fb_prefix("FBGN0004647"), "FBgn0004647");
        assert_eq!(normalize_fb_prefix("FBog0000012345"), "FBog0000012345");
        assert!(matches!(
            normalize_fb_prefix("FBgn0004647"),
            Cow::Borrowed(_)
        ));
        assert_eq!(normalize_fb_prefix("fbgn004647"), "fbgn004647");
        assert_eq!(normalize_fb_prefix("Notch"), "Notch");
        assert_eq!(normalize_fb_prefix("fb1n0004647"), "fb1n0004647");
        assert!(looks_like_fb_id(&normalize_fb_prefix("FBAL0012701")));
    }
    #[test]
    fn fb_id_detection_and_class() {
        assert!(looks_like_fb_id("FBgn0004647"));
        assert!(looks_like_fb_id("FBog0000012345"));
        assert!(!looks_like_fb_id("FBgn004647"));
        assert!(!looks_like_fb_id("Notch"));
        assert_eq!(class_of("é"), "unknown");
        assert_eq!(class_of("FB"), "unknown");
        assert_eq!(class_of("FBal0012701"), "allele");
    }
}
