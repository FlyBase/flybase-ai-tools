//! Report page sections for `get_report`. One `ReportType` per FlyBase ID prefix.
pub mod allele;
pub mod construct;
pub mod gene;
pub mod insertion;
pub mod polypeptide;
pub mod reference;
pub mod stock;
pub mod transcript;

use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
};
use deadpool_postgres::Object;
use serde_json::{Map, Value, json};
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

/// Every ID prefix in spec 4.5 with the label `get_report`'s description uses for it. The description must name
/// exactly the labels whose prefix `for_id` dispatches (unit test in tools/report.rs).
pub const PREFIX_LABELS: &[(&str, &str)] = &[
    ("FBgn", "genes"),
    ("FBal", "alleles"),
    ("FBti", "insertions"),
    ("FBtp", "constructs"),
    ("FBtr", "transcripts"),
    ("FBpp", "polypeptides"),
    ("FBab", "aberrations"),
    ("FBba", "balancers"),
    ("FBst", "stocks"),
    ("FBrf", "references"),
    ("FBgg", "gene groups"),
    ("FBhh", "disease models"),
    ("FBtc", "cell lines"),
    ("FBsn", "strains"),
    ("FBlc", "libraries"),
    ("GO:0", "vocabulary terms"),
    ("FBcl", "clones"),
    ("FBsf", "sequence features"),
    ("FBte", "natural transposons"),
    ("FBch", "chemicals"),
    ("FBog", "ortholog genes"),
    ("FBco", "split system combinations"),
    ("FBmc", "plasmids"),
];

pub fn for_id(id: &str) -> Option<Box<dyn ReportType>> {
    match id.get(..4).unwrap_or("") {
        "FBal" => Some(Box::new(allele::Allele)),
        "FBgn" => Some(Box::new(gene::Gene)),
        "FBrf" => Some(Box::new(reference::Reference)),
        "FBst" => Some(Box::new(stock::Stock)),
        "FBti" => Some(Box::new(insertion::Insertion)),
        "FBtp" => Some(Box::new(construct::Construct)),
        "FBtr" => Some(Box::new(transcript::Transcript)),
        "FBpp" => Some(Box::new(polypeptide::Polypeptide)),
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

/// A golden-path location as chado stores it (fmin 0-based, fmax 1-based, strand 1, -1, or 0).
pub struct Location {
    pub chromosome: String,
    pub fmin: i64,
    pub fmax: i64,
    pub strand: i16,
}

impl Location {
    pub fn jbrowse(&self) -> (String, i64, i64) {
        (self.chromosome.clone(), self.fmin, self.fmax)
    }
}

/// `{"error": code, "message": ...}` in place of a section that failed (see tools/report.rs).
pub fn section_error(e: &ToolError) -> Value {
    json!({ "error": e.code(), "message": e.to_string() })
}

/// True when the test harness has asked for the test-only sections (`__timeout_probe`). Never set in production.
pub fn test_sections_enabled() -> bool {
    std::env::var("FLYBASE_MCP_TEST_SECTIONS").is_ok_and(|v| v == "1")
}

/// `{id, citation, year, pubmed_id}` from any references query with columns uniquename, miniref, pyear, pmid.
pub fn pub_row(r: &Row) -> Value {
    json!({
        "id": r.get::<_, String>("uniquename"),
        "citation": r.get::<_, Option<String>>("miniref"),
        "year": r.get::<_, Option<String>>("pyear"),
        "pubmed_id": r.get::<_, Option<String>>("pmid"),
    })
}

/// Identity of any feature: `{id, symbol, name, species, type, length}`; NotFound when the ID is not a current feature.
pub async fn feature_identity(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx
        .query("report/common/feature_identity", &[&ctx.id])
        .await?;
    let r = r.first().ok_or_else(|| ToolError::NotFound {
        tool: ctx.tool,
        message: format!("{} not found", ctx.id),
    })?;
    Ok(json!({
        "id": ctx.id,
        "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
        "name": r.get::<_, Option<String>>("fullname").map(|s| strip_markup(&s)),
        "species": r.get::<_, Option<String>>("species"),
        "type": r.get::<_, Option<String>>("type"),
        "length": r.get::<_, Option<i32>>("seqlen"),
    }))
}

pub async fn feature_location(ctx: &SectionCtx) -> ToolResult<Option<Location>> {
    let rows = ctx.query("report/common/location", &[&ctx.id]).await?;
    Ok(rows.first().and_then(|r| {
        Some(Location {
            chromosome: r.get::<_, Option<String>>("chromosome")?,
            fmin: i64::from(r.get::<_, Option<i32>>("fmin")?),
            fmax: i64::from(r.get::<_, Option<i32>>("fmax")?),
            strand: r.get::<_, Option<i16>>("strand").unwrap_or(0),
        })
    }))
}

/// `{located, chromosome, start, end, strand}` with start 1-based.
pub fn location_json(loc: &Option<Location>) -> Value {
    match loc {
        None => json!({ "located": false }),
        Some(l) => json!({
            "located": true,
            "chromosome": l.chromosome,
            "start": l.fmin + 1,
            "end": l.fmax,
            "strand": l.strand,
        }),
    }
}

/// Selected featureprops as `type -> [values in rank order]`, markup stripped.
pub async fn props(ctx: &SectionCtx, types: &[&str]) -> ToolResult<Map<String, Value>> {
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    let rows = ctx.query("report/common/props", &[&ctx.id, &types]).await?;
    Ok(props_map(&rows))
}

pub async fn references_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx
        .query("report/common/references", &[&ctx.id, &ctx.fetch])
        .await?;
    Ok(paged(rows_to_json(&r, pub_row), ctx.limit))
}

/// `{name, kind, is_current}` from any synonyms query with columns name, kind, is_current (feature, grp, humanhealth,
/// cell_line, strain, library).
pub fn synonym_row(r: &Row) -> Value {
    json!({
        "name": strip_markup(&r.get::<_, String>("name")),
        "kind": r.get::<_, String>("kind"),
        "is_current": r.get::<_, bool>("is_current"),
    })
}

pub async fn synonyms_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx.query("report/common/synonyms", &[&ctx.id]).await?;
    Ok(json!(rows_to_json(&r, synonym_row)))
}

/// Property rows (columns type, value, rank, in rank order) as `type -> [values]`, markup stripped. Shared by the
/// feature props queries and the cell line, strain, and library props queries.
pub fn props_map(rows: &[Row]) -> Map<String, Value> {
    let mut out = Map::new();
    for r in rows {
        if let Some(v) = r.get::<_, Option<String>>("value") {
            out.entry(r.get::<_, String>("type"))
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("array")
                .push(json!(strip_markup(&v)));
        }
    }
    out
}

/// gene/external_links.sql joins feature by uniquename, so it serves every feature type.
pub async fn external_links_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx.query("report/gene/external_links", &[&ctx.id]).await?;
    Ok(json!(rows_to_json(&r, |r| {
        json!({
            "database": r.get::<_, String>("db"),
            "accession": r.get::<_, String>("accession"),
            "url": r.get::<_, Option<String>>("url"),
        })
    })))
}

pub async fn relationships_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx
        .query("report/common/relationships", &[&ctx.id, &ctx.fetch])
        .await?;
    Ok(paged(
        rows_to_json(&r, |r| {
            json!({
                "relationship": r.get::<_, String>("relationship"),
                "direction": r.get::<_, String>("direction"),
                "id": r.get::<_, String>("other_id"),
                "symbol": r.get::<_, Option<String>>("other_symbol").map(|s| strip_markup(&s)),
                "type": r.get::<_, String>("other_type"),
            })
        }),
        ctx.limit,
    ))
}

/// Stocks from the derived_stock_* featureprops via flybase.get_stocks (insertions, aberrations, balancers, constructs).
pub async fn stocks_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx
        .query("report/common/stocks", &[&ctx.id, &ctx.fetch])
        .await?;
    Ok(paged(
        rows_to_json(&r, |r| {
            json!({
                "id": r.get::<_, Option<String>>("id"),
                "provider": r.get::<_, Option<String>>("provider"),
                "stock_number": r.get::<_, Option<String>>("stock_number"),
                "genotype": r.get::<_, Option<String>>("genotype").map(|s| strip_markup(&s)),
            })
        }),
        ctx.limit,
    ))
}

pub async fn stock_count_section(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx.query("report/common/stock_count", &[&ctx.id]).await?;
    Ok(json!(r.first().map(|r| r.get::<_, i64>("n")).unwrap_or(0)))
}

/// Test-only section that exceeds the light statement timeout, so tests can see a per-section error.
pub async fn timeout_probe(ctx: &SectionCtx) -> ToolResult<Value> {
    let r = ctx.query("report/common/timeout_probe", &[]).await?;
    Ok(json!(r.len()))
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
        assert_eq!(for_id("FBal0012701").unwrap().name(), "allele");
        assert_eq!(for_id("FBrf0227604").unwrap().name(), "reference");
        assert_eq!(for_id("FBst0028813").unwrap().name(), "stock");
        assert_eq!(for_id("FBti0000488").unwrap().name(), "insertion");
        assert_eq!(for_id("FBtp0000743").unwrap().name(), "construct");
        assert_eq!(for_id("FBtr0070507").unwrap().name(), "transcript");
        assert_eq!(for_id("FBpp0070483").unwrap().name(), "polypeptide");
        assert!(for_id("x").is_none());
    }
}
