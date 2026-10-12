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

/// Multi-valued cells stop at this many values and append "(+N more)".
pub const CELL_CAP: i64 = 50;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Json,
    Tsv,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Up to 500 FlyBase IDs (secondary IDs are updated first).
    pub ids: Vec<String>,
    /// Field names from the catalog; pass list_options=true to see them by class.
    pub fields: Vec<String>,
    /// json (default) returns rows as objects; tsv returns a tab-separated text block and row_count instead.
    pub format: Option<Format>,
    /// If true, return only the field catalog.
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
        description = "Download chosen fields for up to 500 FlyBase IDs at once as rows (JSON or TSV): symbols, names, locations, synonyms, references, GO terms, alleles, transcripts, human orthologs, allele classes, stocks, and more. Use list_options=true to see the field catalog. Use when you have a list of IDs and want a table; use get_report for one entity in depth."
    )]
    pub async fn batch_download(
        &self,
        Parameters(p): Parameters<BatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "batch_download";
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    if p.list_options.unwrap_or(false) {
                        return Ok(Envelope::new(T, &self.release)
                            .summary("Fields by class; 'any' fields apply to every feature ID.")
                            .field("fields", catalog())
                            .finish());
                    }
                    let _permit = self.limits.heavy(T).await?;
                    self.batch_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn batch_impl(&self, p: BatchParams) -> ToolResult<CallToolResult> {
        const T: &str = "batch_download";
        if p.ids.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "ids",
                message: "provide at least one ID".into(),
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
        let inputs: Vec<String> = p
            .ids
            .iter()
            .map(|s| normalize_fb_prefix(s.trim()).into_owned())
            .collect();
        if let Some(bad) = inputs.iter().find(|s| !looks_like_fb_id(s)) {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "ids",
                message: format!(
                    "'{bad}' is not a FlyBase ID; use validate_ids for symbols and external IDs"
                ),
            });
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
            rows.push(Value::Object(row));
        }
        let unknown = statuses
            .iter()
            .filter(|s| !matches!(s.status, "current" | "updated"))
            .count();
        let summary = format!(
            "{} row(s) with {} field(s); {unknown} input(s) without a current ID.",
            rows.len(),
            fields.len()
        );
        let names: Vec<&str> = fields.iter().map(|f| f.0).collect();
        // A table that would be cut by the envelope's array halving is useless, so refuse it whole instead.
        let payload = if p.format.unwrap_or_default() == Format::Tsv {
            let header = tsv_line(
                ["input", "id"]
                    .into_iter()
                    .map(String::from)
                    .chain(names.iter().map(|f| f.to_string())),
            );
            let body: Vec<String> = rows
                .iter()
                .map(|r| {
                    tsv_line(
                        ["input", "id"]
                            .into_iter()
                            .chain(names.iter().copied())
                            .map(|k| r[k].as_str().unwrap_or("").to_string()),
                    )
                })
                .collect();
            json!({"tsv": format!("{header}\n{}\n", body.join("\n")), "row_count": rows.len()})
        } else {
            json!({"rows": rows})
        };
        let bytes = serde_json::to_vec(&payload)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
        if bytes + 1024 > MAX_JSON_BYTES {
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
                    "{} KB of results for {} IDs and {} fields exceeds the {} KB limit; largest fields: {}; request fewer fields or fewer IDs per call (format: tsv is smaller, it has no per-row keys or null cells)",
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
        if let Value::Object(m) = payload {
            for (k, v) in m {
                env = env.field(&k, v);
            }
        }
        Ok(env.finish())
    }
}
