use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, report_link},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
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

/// Rank: exact symbol > exact name of any kind > symbol prefix > other prefix > trigram similarity; current names first; Dmel preferred when species unspecified.
/// One entry per ID (best score wins).
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
            s += 100;
        }
        if species.is_none() && x.species == "Dmel" {
            s += 50;
        }
        x.score = s;
    }
    c.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
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
        description = "Turn one gene, allele, insertion, stock, or other symbol, name, synonym, or ID into current FlyBase IDs. Handles obsolete and secondary IDs. Use this first whenever you have a name and need an ID for another tool. For a batch of exact IDs use validate_ids; for free-text discovery use search."
    )]
    pub async fn resolve(
        &self,
        Parameters(p): Parameters<ResolveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let _permit = self.limits.light().await;
        match self.resolve_impl(p).await {
            Ok(r) => Ok(r),
            Err(e) => Ok(e.into_result()),
        }
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
            let species = p.species.as_deref();
            let class = p.data_class.map(DataClass::as_str);
            let rows = self
                .chado
                .query(T, "resolve/candidates", &[&q, &species, &class])
                .await?;
            let cands: Vec<Candidate> = rows
                .iter()
                .map(|r| Candidate {
                    id: r.get("entity_id"),
                    data_class: r.get("data_class"),
                    species: r.get("species"),
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
        let top = matches[0].clone();
        let top_id = top["id"].as_str().unwrap_or("?").to_string();
        let symbol_note = top["symbol"]
            .as_str()
            .map(|s| format!(", symbol {s}"))
            .unwrap_or_default();
        let summary = format!(
            "'{q}' resolves to {top_id} ({}{symbol_note}). {} match(es).",
            top["data_class"].as_str().unwrap_or("?"),
            matches.len()
        );
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
        assert_eq!(r[0].score, 950);
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
