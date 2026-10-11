//! Controlled-vocabulary term report (GO, FBbt, FBcv, FBdv, DOID, SO, PSI-MI), served by `get_report` and `get_term`.
use super::{ReportType, SectionCtx, paged, rows_to_json};
use crate::error::{ToolError, ToolResult};
use serde_json::{Value, json};
use tokio_postgres::Row;

/// One report call's term. The term row is fetched once per call (`term` caches it); `for_id` builds a fresh value
/// per call.
#[derive(Default)]
pub struct CvTerm {
    term: tokio::sync::OnceCell<Row>,
}

pub const DEFAULT: &[&str] = &["definition", "synonyms", "parents", "children"];
pub const OPTIONAL: &[&str] = &["annotation_counts"];
/// Above this many descendants, annotation counts cover the term alone (the closure join would exceed the light
/// timeout).
pub const MAX_DESCENDANTS: i64 = 5000;
/// The db names (the part before the colon) this server treats as vocabulary term IDs; `DO` is accepted for `DOID`.
pub const VOCAB_DBS: &[&str] = &["GO", "FBbt", "FBcv", "FBdv", "DOID", "SO", "MI"];

/// Split `GO:0007219` into `("GO", "0007219")` when the prefix is a known vocabulary (case-insensitive, `DO` meaning
/// `DOID`) and the accession is digits.
pub fn parse_term_id(id: &str) -> Option<(&'static str, &str)> {
    let (db, acc) = id.split_once(':')?;
    let db = if db.eq_ignore_ascii_case("DO") {
        "DOID"
    } else {
        db
    };
    let db = VOCAB_DBS.iter().find(|d| d.eq_ignore_ascii_case(db))?;
    (!acc.is_empty() && acc.bytes().all(|b| b.is_ascii_digit())).then_some((*db, acc))
}

/// Canonical form (`go:0007219` -> `GO:0007219`), or None when the ID is not a vocabulary term ID.
pub fn canonical_term_id(id: &str) -> Option<String> {
    parse_term_id(id).map(|(db, acc)| format!("{db}:{acc}"))
}

async fn fetch_term(ctx: &SectionCtx) -> ToolResult<Row> {
    let (db, acc) = parse_term_id(&ctx.id).ok_or_else(|| ToolError::InvalidInput {
        tool: ctx.tool,
        arg: "id",
        message: format!(
            "'{}' is not a vocabulary term ID such as GO:0007219",
            ctx.id
        ),
    })?;
    let mut rows = ctx.query("report/cvterm/term", &[&db, &acc]).await?;
    if rows.is_empty() {
        return Err(ToolError::NotFound {
            tool: ctx.tool,
            message: format!("{} is not a term in the {db} vocabulary", ctx.id),
        });
    }
    Ok(rows.remove(0))
}

impl CvTerm {
    /// The term row, fetched on first use; concurrent sections of one call wait on the same fetch.
    async fn term_row(&self, ctx: &SectionCtx) -> ToolResult<&Row> {
        self.term.get_or_try_init(|| fetch_term(ctx)).await
    }
}

fn related_term(r: &Row) -> Value {
    json!({
        "relationship": r.get::<_, String>("relationship"),
        "term_id": r.get::<_, String>("term_id"),
        "name": r.get::<_, String>("name"),
    })
}

#[async_trait::async_trait]
impl ReportType for CvTerm {
    fn name(&self) -> &'static str {
        "cv_term"
    }
    fn default_sections(&self) -> &'static [&'static str] {
        DEFAULT
    }
    fn optional_sections(&self) -> &'static [&'static str] {
        OPTIONAL
    }
    fn resolves_feature_id(&self) -> bool {
        false
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let term = self.term_row(ctx).await?;
        let cvterm_id: i32 = term.get("cvterm_id");
        Ok(match name {
            "definition" => json!({
                "id": term.get::<_, String>("term_id"),
                "name": term.get::<_, String>("name"),
                "vocabulary": term.get::<_, String>("vocabulary"),
                "definition": term.get::<_, Option<String>>("definition"),
                "comment": term.get::<_, Option<String>>("comment"),
                "is_obsolete": term.get::<_, bool>("is_obsolete"),
            }),
            "synonyms" => {
                let r = ctx.query("report/cvterm/synonyms", &[&cvterm_id]).await?;
                json!(rows_to_json(&r, |r| {
                    json!({"name": r.get::<_, String>("name"), "scope": r.get::<_, Option<String>>("scope")})
                }))
            }
            "parents" => {
                let r = ctx.query("report/cvterm/parents", &[&cvterm_id]).await?;
                json!(rows_to_json(&r, related_term))
            }
            "children" => {
                let r = ctx
                    .query("report/cvterm/children", &[&cvterm_id, &ctx.fetch])
                    .await?;
                paged(rows_to_json(&r, related_term), ctx.limit)
            }
            "annotation_counts" => {
                let d = ctx
                    .query("report/cvterm/descendant_count", &[&cvterm_id])
                    .await?;
                let descendants = d.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0);
                let include = descendants <= MAX_DESCENDANTS;
                let r = ctx
                    .query("report/cvterm/annotation_counts", &[&cvterm_id, &include])
                    .await?;
                let counts: Vec<Value> = r
                    .iter()
                    .filter(|r| r.get::<_, i64>("n") > 0)
                    .map(|r| json!({"class": r.get::<_, String>("class"), "count": r.get::<_, i64>("n")}))
                    .collect();
                json!({"descendants": descendants, "descendants_included": include, "counts": counts})
            }
            other => {
                return Err(ToolError::InvalidInput {
                    tool: ctx.tool,
                    arg: "sections",
                    message: format!("unknown cv_term section '{other}'"),
                });
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn term_ids_parse() {
        assert_eq!(parse_term_id("GO:0007219"), Some(("GO", "0007219")));
        assert_eq!(canonical_term_id("doid:162").as_deref(), Some("DOID:162"));
        assert_eq!(canonical_term_id("DO:162").as_deref(), Some("DOID:162"));
        assert_eq!(parse_term_id("MI:0004"), Some(("MI", "0004")));
        assert_eq!(parse_term_id("GO:abc"), None);
        assert_eq!(parse_term_id("GO:"), None);
        assert_eq!(parse_term_id("HGNC:7881"), None);
        assert_eq!(parse_term_id("FBgn0004647"), None);
    }
}
