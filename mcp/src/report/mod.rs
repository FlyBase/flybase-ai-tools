//! Report page sections for `get_report`. One `ReportType` per FlyBase ID prefix.
pub mod gene;

use crate::{backends::chado::Chado, error::ToolResult};
use deadpool_postgres::Object;
use serde_json::{Value, json};
use tokio_postgres::{Row, types::ToSql};

/// Everything a section needs. A report call checks out ONE pooled connection and every section runs
/// its queries on it through `query`; concurrent sections stay parallel because tokio-postgres pipelines
/// queries on one client. Never check out a second connection from inside a section (see `Chado`).
pub struct SectionCtx {
    pub conn: Object,
    pub tool: &'static str,
    pub id: String,
    pub limit: i64,
    /// `limit + 1`: list sections fetch one extra row so `paged` can set `has_more`.
    pub fetch: i64,
}

impl SectionCtx {
    pub async fn query(&self, name: &str, params: &[&(dyn ToSql + Sync)]) -> ToolResult<Vec<Row>> {
        Chado::query_on(&self.conn, self.tool, name, params).await
    }
}

#[async_trait::async_trait]
pub trait ReportType: Send + Sync {
    fn name(&self) -> &'static str;
    fn default_sections(&self) -> &'static [&'static str];
    fn optional_sections(&self) -> &'static [&'static str];
    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value>;
    /// Chromosome, fmin (0-based), fmax for a JBrowse link, if the entity is located.
    async fn location(&self, _ctx: &SectionCtx) -> ToolResult<Option<(String, i64, i64)>> {
        Ok(None)
    }
}

pub fn for_id(id: &str) -> Option<Box<dyn ReportType>> {
    match id.get(..4).unwrap_or("") {
        "FBgn" => Some(Box::new(gene::Gene)),
        _ => None,
    }
}

/// Page a list section: callers fetch `limit + 1` rows and pass them here.
pub fn paged(rows: Vec<Value>, limit: i64) -> Value {
    let has_more = rows.len() as i64 > limit;
    let items: Vec<Value> = rows.into_iter().take(limit as usize).collect();
    json!({ "items": items, "has_more": has_more, "count_shown": items.len() })
}

pub fn rows_to_json(rows: &[Row], f: impl Fn(&Row) -> Value) -> Vec<Value> {
    rows.iter().map(f).collect()
}

/// Remove FlyBase inline markup such as `@SO0001217:protein_coding_gene@` and `<up>...</up>`.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '@' => {
                // @PREFIX:text@ -> text
                let mut inner = String::new();
                for n in chars.by_ref() {
                    if n == '@' {
                        break;
                    }
                    inner.push(n);
                }
                out.push_str(inner.split_once(':').map(|(_, t)| t).unwrap_or(&inner));
            }
            '<' => {
                let mut tag = String::new();
                for n in chars.by_ref() {
                    if n == '>' {
                        break;
                    }
                    tag.push(n);
                }
                match tag.as_str() {
                    "up" => out.push('['),
                    "/up" => out.push(']'),
                    "down" => out.push('['),
                    "/down" => out.push(']'),
                    _ => {}
                }
            }
            _ => out.push(c),
        }
    }
    out
}

pub fn jbrowse_link(chromosome: &str, fmin: i64, fmax: i64) -> String {
    let pad = ((fmax - fmin) / 10).max(500);
    format!(
        "https://flybase.org/jbrowse2/?config=dmel/config.json&assembly=dmel&loc={}:{}..{}&tracks=Gene&tracklist=true",
        chromosome,
        (fmin - pad).max(1),
        fmax + pad
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markup_is_stripped() {
        assert_eq!(
            strip_markup("@SO0001217:protein_coding_gene@"),
            "protein_coding_gene"
        );
        assert_eq!(strip_markup("N<up>55e11</up>"), "N[55e11]");
        assert_eq!(strip_markup("plain"), "plain");
    }
    #[test]
    fn paging() {
        let p = paged(vec![json!(1), json!(2), json!(3)], 2);
        assert_eq!(p["has_more"], true);
        assert_eq!(p["count_shown"], 2);
    }
    #[test]
    fn jbrowse_link_pads_and_clamps() {
        let l = jbrowse_link("X", 100, 200);
        assert!(l.contains("loc=X:1..700"), "{l}");
        assert!(for_id("FBgn0004647").is_some());
        assert!(for_id("FBal0012701").is_none());
        assert!(for_id("x").is_none());
    }
}
