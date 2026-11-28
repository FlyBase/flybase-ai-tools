use crate::backends::cache::ResponseCache;
use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
    limits::{MAX_BATCH, MAX_JSON_BYTES},
    output::{Envelope, complete},
    report::strip_markup,
    server::FlyBaseServer,
    tools::resolve::{class_of, looks_like_fb_id, normalize_fb_prefix, resolve_ids},
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

mod precomputed_files;
mod record_xrefs;

const MAX_DOWNLOAD_IDS: usize = 50_000;
const MAX_PAGE_ROWS: usize = 1_000;
const TOOL: &str = "batch_download";

/// Multi-valued cells stop at this many values and append "(+N more)".
pub const CELL_CAP: i64 = 50;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Json,
    Tsv,
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    ReportFields,
    RecordCrossReferences,
    PrecomputedFiles,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Up to 50,000 FlyBase IDs. Record cross-references and precomputed files also accept exact symbols. Resend the same list for each page.
    #[serde(default)]
    #[schemars(length(max = 50000))]
    pub ids: Vec<String>,
    /// Output mode: report_fields (default), record_cross_references (internal links), or precomputed_files (filtered bulk file lines).
    pub mode: Option<Mode>,
    /// Field names for report_fields mode. Pass list_options=true to see them by class.
    #[serde(default)]
    pub fields: Vec<String>,
    /// File keys in output order for precomputed_files mode. Use list_options=true in that mode to discover keys.
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub files: Vec<String>,
    /// Optional target FlyBase ID prefixes such as FBgn, FBal, or FBtr, for record_cross_references mode.
    #[schemars(length(min = 1, max = 64))]
    pub target_classes: Option<Vec<String>>,
    /// json (default) returns rows as objects. tsv returns a text block and row_count. Precomputed files preserve source lines in both formats.
    pub format: Option<Format>,
    /// Zero-based index of the first input, or a source position in precomputed_files mode. Use next_offset from the previous response.
    pub offset: Option<usize>,
    /// Offset within one record's cross-references. Pass next_link_offset when non-null. Omit link_offset when next_link_offset is null.
    pub link_offset: Option<usize>,
    /// Maximum rows per page (1 to 1,000). Wide rows can reduce the number that fit.
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<usize>,
    /// If true, return only the options for the selected mode.
    pub list_options: Option<bool>,
}

/// `(name, class, sql, description)`. `class` is "any" (every feature), "gene", or "allele"; a field applied to an ID of
/// another class is simply null.
pub const FIELDS: &[(&str, &str, &str, &str)] = &[
    ("symbol", "any", "batch/symbol", "current symbol"),
    ("name", "any", "batch/name", "current full name"),
    ("species", "any", "batch/species", "species abbreviation"),
    ("type", "any", "batch/type", "SO feature type"),
    (
        "location",
        "any",
        "batch/location",
        "chromosome:start..end(strand), 1-based",
    ),
    ("synonyms", "any", "batch/synonyms", "all synonyms"),
    (
        "references",
        "any",
        "batch/references",
        "FBrf IDs of citing references",
    ),
    (
        "pubmed_ids",
        "any",
        "batch/pubmed_ids",
        "PubMed IDs of citing references",
    ),
    (
        "external_ids",
        "any",
        "batch/external_ids",
        "db:accession cross-references",
    ),
    ("annotation_id", "gene", "batch/annotation_id", "CG number"),
    ("gene_type", "gene", "batch/gene_type", "promoted gene type"),
    (
        "cytology",
        "gene",
        "batch/cytology",
        "computed cytological location",
    ),
    (
        "go_biological_process",
        "gene",
        "batch/go_terms",
        "GO biological process terms",
    ),
    (
        "go_molecular_function",
        "gene",
        "batch/go_terms",
        "GO molecular function terms",
    ),
    (
        "go_cellular_component",
        "gene",
        "batch/go_terms",
        "GO cellular component terms",
    ),
    (
        "alleles",
        "gene",
        "batch/alleles",
        "FBal IDs of classical and transgenic alleles",
    ),
    ("transcripts", "gene", "batch/transcripts", "FBtr IDs"),
    ("polypeptides", "gene", "batch/polypeptides", "FBpp IDs"),
    (
        "human_orthologs",
        "gene",
        "batch/human_orthologs",
        "human orthologs with HGNC ID and DIOPT score",
    ),
    (
        "stock_count",
        "gene",
        "batch/stock_count",
        "distinct stocks carrying an allele of the gene",
    ),
    (
        "allele_gene",
        "allele",
        "batch/allele_gene",
        "FBgn and symbol of the gene",
    ),
    (
        "allele_classes",
        "allele",
        "batch/allele_classes",
        "allele classes",
    ),
    (
        "allele_mutagens",
        "allele",
        "batch/allele_mutagens",
        "mutagens",
    ),
    (
        "allele_insertions",
        "allele",
        "batch/allele_insertions",
        "FBti IDs",
    ),
    (
        "allele_constructs",
        "allele",
        "batch/allele_constructs",
        "FBtp IDs",
    ),
    ("allele_stocks", "allele", "batch/allele_stocks", "FBst IDs"),
    (
        "transgenic_product_class",
        "allele",
        "batch/transgenic_product_class",
        "transgenic product classes",
    ),
];

type FieldSpec = (&'static str, &'static str, &'static str, &'static str);

fn field(name: &str) -> Option<&'static FieldSpec> {
    FIELDS.iter().find(|f| f.0 == name)
}

fn aspect(field: &str) -> Option<&'static str> {
    match field {
        "go_biological_process" => Some("biological_process"),
        "go_molecular_function" => Some("molecular_function"),
        "go_cellular_component" => Some("cellular_component"),
        _ => None,
    }
}

fn catalog() -> Value {
    let mut by_class: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for (name, class, _, desc) in FIELDS {
        by_class
            .entry(class)
            .or_default()
            .push(json!({"name": name, "description": desc}));
    }
    json!(by_class)
}

/// One TSV line; tabs and newlines inside a cell become spaces.
fn tsv_line(cells: impl Iterator<Item = String>) -> String {
    cells
        .map(|c| c.replace(['\t', '\n', '\r'], " "))
        .collect::<Vec<_>>()
        .join("\t")
}

#[tool_router(router = tool_router_batch, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "batch_download",
        description = "Download report fields (default), internal FlyBase record cross-references (mode=record_cross_references), or matching lines from release bulk files (mode=precomputed_files). JSON and TSV are available for up to 50,000 inputs. Use list_options=true to see options for the selected mode. Keep inputs, filters, and format fixed when paging. Pass next_offset as offset and, for record cross-references, pass next_link_offset as link_offset when present. Omit link_offset when next_link_offset is null. Stop when next_offset is null. Use get_report for one entity in depth."
    )]
    pub async fn batch_download(
        &self,
        Parameters(p): Parameters<BatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = TOOL;
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let mode = p.mode.unwrap_or_default();
                    if p.list_options.unwrap_or(false) {
                        return match mode {
                            Mode::ReportFields => Ok(Envelope::new(T, &self.release)
                                .summary("Fields by class; 'any' fields apply to every feature ID.")
                                .field("fields", catalog())
                                .field(
                                    "modes",
                                    json!([
                                        Mode::ReportFields,
                                        Mode::RecordCrossReferences,
                                        Mode::PrecomputedFiles
                                    ]),
                                )
                                .finish()),
                            Mode::RecordCrossReferences => {
                                Ok(self.record_cross_reference_options())
                            }
                            Mode::PrecomputedFiles => self.precomputed_file_options(),
                        };
                    }
                    if mode != Mode::PrecomputedFiles && !p.files.is_empty() {
                        return Err(ToolError::InvalidInput {
                            tool: T,
                            arg: "files",
                            message: "only available in precomputed_files mode".into(),
                        });
                    }
                    match mode {
                        Mode::ReportFields => {
                            let _permit = self.limits.heavy(T).await?;
                            self.batch_impl(p).await
                        }
                        Mode::RecordCrossReferences => self.record_cross_references(p).await,
                        Mode::PrecomputedFiles => self.download_precomputed_files(p).await,
                    }
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn batch_impl(&self, p: BatchParams) -> ToolResult<CallToolResult> {
        const T: &str = TOOL;
        if p.link_offset.is_some() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "link_offset",
                message: "only available in record_cross_references mode".into(),
            });
        }
        if p.target_classes.is_some() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "target_classes",
                message: "only available in record_cross_references mode".into(),
            });
        }
        if p.ids.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "ids",
                message: "provide at least one ID".into(),
            });
        }
        if p.ids.len() > MAX_DOWNLOAD_IDS {
            return Err(ToolError::LimitExceeded {
                tool: T,
                message: format!(
                    "{MAX_DOWNLOAD_IDS} IDs max per call ({} given); split the list",
                    p.ids.len()
                ),
            });
        }
        if p.fields.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "fields",
                message: "choose at least one field; list_options=true shows the catalog".into(),
            });
        }
        let mut fields: Vec<&'static FieldSpec> = Vec::new();
        for f in &p.fields {
            match field(f) {
                Some(spec) if !fields.iter().any(|x| x.0 == spec.0) => fields.push(spec),
                Some(_) => {}
                None => {
                    return Err(ToolError::InvalidInput {
                        tool: T,
                        arg: "fields",
                        message: format!(
                            "unknown field '{f}'; list_options=true shows the catalog"
                        ),
                    });
                }
            }
        }
        let total = p.ids.len();
        let paged = total > MAX_BATCH || p.offset.is_some() || p.limit.is_some();
        let offset = p.offset.unwrap_or(0);
        let limit = p.limit.unwrap_or(MAX_PAGE_ROWS);
        if offset >= total {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: "must identify a row in the input list".into(),
            });
        }
        if !(1..=MAX_PAGE_ROWS).contains(&limit) {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "limit",
                message: format!("must be between 1 and {MAX_PAGE_ROWS}"),
            });
        }
        let end = (offset + limit).min(total);
        let mut inputs = Vec::with_capacity(end - offset);
        for (index, raw) in p.ids.iter().enumerate() {
            let id = normalize_fb_prefix(raw.trim());
            if !looks_like_fb_id(&id) {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "ids",
                    message: format!(
                        "'{id}' is not a FlyBase ID; use validate_ids for symbols and external IDs"
                    ),
                });
            }
            if (offset..end).contains(&index) {
                inputs.push(id.into_owned());
            }
        }
        // Secondary IDs are updated first so every row reports both the input and the current ID.
        let statuses = resolve_ids(&self.chado, T, &inputs).await?;
        let current: Vec<String> = statuses
            .iter()
            .filter(|s| matches!(s.status, "current" | "updated"))
            .map(|s| s.current_ids[0].clone())
            .collect();
        let fields_for_query = fields.clone();
        let cells: HashMap<(String, &'static str), (String, i64)> = self
            .chado
            .heavy(T, async |tx| {
                let mut cells = HashMap::new();
                for (name, _, sql, _) in fields_for_query {
                    let rows = match aspect(name) {
                        Some(a) => Chado::query_tx(tx, T, sql, &[&current, &a]).await?,
                        None => Chado::query_tx(tx, T, sql, &[&current]).await?,
                    };
                    for r in &rows {
                        if let (Some(id), Some(value)) = (
                            r.get::<_, Option<String>>("id"),
                            r.get::<_, Option<String>>("value"),
                        ) {
                            cells.insert((id, *name), (value, r.get::<_, i64>("n")));
                        }
                    }
                }
                Ok(cells)
            })
            .await?;
        let names: Vec<&str> = fields.iter().map(|f| f.0).collect();
        let columns = ["input", "id"].into_iter().chain(names.iter().copied());
        let tsv = p.format.unwrap_or_default() == Format::Tsv;
        let mut table = String::new();
        if tsv {
            table = tsv_line(columns.clone().map(String::from));
            table.push('\n');
        }
        // Reserve space for envelope keys, field names, the release, and page metadata.
        let overhead =
            1024 + json!(names).to_string().len() + json!(self.release).to_string().len();
        let mut bytes = overhead + json!(table).to_string().len();
        let mut unknown = 0;
        let mut rows: Vec<Value> = Vec::with_capacity(inputs.len());
        for st in &statuses {
            let mut row = Map::new();
            row.insert("input".into(), json!(st.input));
            // A split ID has several targets and no single current ID; its row carries the targets in `note`.
            let id = if matches!(st.status, "current" | "updated") {
                st.current_ids.first().cloned()
            } else {
                None
            };
            row.insert("id".into(), json!(id));
            row.insert("status".into(), json!(st.status));
            row.insert("class".into(), json!(id.as_deref().map(class_of)));
            if st.status == "split" {
                row.insert(
                    "note".into(),
                    json!(format!("split into {}", st.current_ids.join("|"))),
                );
            }
            for (name, _, _, _) in &fields {
                let v = id
                    .as_ref()
                    .and_then(|id| cells.get(&(id.clone(), *name)))
                    .map(|(value, n)| {
                        let mut text = strip_markup(value);
                        if *n > CELL_CAP {
                            text.push_str(&format!(" (+{} more)", n - CELL_CAP));
                        }
                        text
                    });
                row.insert(name.to_string(), json!(v));
            }
            let row = Value::Object(row);
            let mut line = String::new();
            if tsv {
                line = tsv_line(
                    columns
                        .clone()
                        .map(|k| row[k].as_str().unwrap_or("").to_owned()),
                );
                line.push('\n');
                // The TSV is embedded in a JSON string. Exclude the two enclosing quotes.
                bytes += json!(line).to_string().len() - 2;
            } else {
                bytes += row.to_string().len() + 1;
            }
            if paged && bytes > MAX_JSON_BYTES {
                if rows.is_empty() {
                    return Err(ToolError::LimitExceeded {
                        tool: T,
                        message: format!(
                            "row at offset {offset} exceeds the response size limit; request fewer fields"
                        ),
                    });
                }
                break;
            }
            table.push_str(&line);
            unknown += usize::from(!matches!(st.status, "current" | "updated"));
            rows.push(row);
        }
        let count = rows.len();
        let summary = format!(
            "{count} row(s) with {} field(s); {unknown} input(s) without a current ID.",
            fields.len()
        );
        if !paged && bytes > MAX_JSON_BYTES {
            // Name the heaviest fields so the caller knows what to drop; TSV has no per-row keys or null cells.
            let mut sizes: Vec<(&str, usize)> = names
                .iter()
                .map(|n| {
                    let cells: usize = rows.iter().map(|r| r[n].as_str().map_or(0, str::len)).sum();
                    (*n, cells + rows.len() * (n.len() + 4))
                })
                .collect();
            sizes.sort_by_key(|x| std::cmp::Reverse(x.1));
            let largest: Vec<String> = sizes
                .iter()
                .take(3)
                .map(|(n, b)| format!("{n} {} KB", b / 1024))
                .collect();
            return Err(ToolError::LimitExceeded {
                tool: T,
                message: format!(
                    "{} KB of results for {} IDs and {} fields exceeds the {} KB limit; largest fields: {}; request fewer fields or set limit to retrieve the table in pages",
                    bytes / 1024,
                    rows.len(),
                    names.len(),
                    MAX_JSON_BYTES / 1024,
                    largest.join(", ")
                ),
            });
        }
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .field("field_names", json!(names));
        if paged {
            let next = offset + count;
            env = env
                .field("total", json!(total))
                .field("offset", json!(offset))
                .field("next_offset", json!((next < total).then_some(next)));
        }
        if tsv {
            env = env
                .field("tsv", Value::String(table))
                .field("row_count", json!(count));
        } else {
            env = env.field("rows", Value::Array(rows));
        }
        Ok(env.finish())
    }
}
