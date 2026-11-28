use super::{BatchParams, Format, MAX_DOWNLOAD_IDS, MAX_PAGE_ROWS, Mode, TOOL, tsv_line};
use crate::{
    backends::record_xrefs::{MAX_SCAN_BYTES, RecordXrefs, id_prefix},
    error::{ToolError, ToolResult},
    limits::MAX_JSON_BYTES,
    output::Envelope,
    server::FlyBaseServer,
};
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const LINKS_PER_RECORD: usize = 4_000;
const MAX_INPUT_BYTES: usize = 256;
const MAX_TARGET_CLASSES: usize = 64;

const TARGETS: &[(&str, &str)] = &[
    ("FBab", "Aberrations"),
    ("FBal", "Alleles"),
    ("FBba", "Balancers"),
    ("FBch", "Chemicals"),
    ("FBcl", "Clones"),
    ("FBco", "Split System Combinations"),
    ("FBgg", "Gene Groups"),
    ("FBgn", "Genes"),
    ("FBhh", "Human Diseases"),
    ("FBig", "Physical Interactions"),
    ("FBim", "Images"),
    ("FBlc", "Large Dataset Metadata"),
    ("FBma", "Meeting Abstracts"),
    ("FBpp", "Polypeptides"),
    ("FBrf", "References"),
    ("FBsf", "Sequence Features"),
    ("FBsn", "Strains"),
    ("FBst", "Stocks"),
    ("FBtc", "Cell Lines"),
    ("FBte", "Natural Transposons"),
    ("FBti", "Insertions"),
    ("FBto", "Experimental Tools"),
    ("FBtp", "Recombinant Constructs"),
    ("FBtr", "Transcripts"),
];

fn invalid(arg: &'static str, message: impl Into<String>) -> ToolError {
    ToolError::InvalidInput {
        tool: TOOL,
        arg,
        message: message.into(),
    }
}

fn unavailable() -> ToolError {
    ToolError::BackendUnavailable {
        tool: TOOL,
        message: "the release record cross-reference catalogs are unavailable".into(),
    }
}

fn page_limit() -> ToolError {
    ToolError::LimitExceeded {
        tool: TOOL,
        message: "record cross-reference page exceeds the response size limit".into(),
    }
}

fn prefix(input: &str) -> Option<String> {
    match input.as_bytes() {
        [f, b, a, z]
            if [*f, *b].eq_ignore_ascii_case(b"FB")
                && a.is_ascii_alphabetic()
                && z.is_ascii_alphabetic() =>
        {
            Some(format!("FB{}", input[2..].to_ascii_lowercase()))
        }
        _ => None,
    }
}

fn input_token(raw: &str) -> ToolResult<String> {
    let raw = raw.trim();
    if !(1..=MAX_INPUT_BYTES).contains(&raw.len()) || raw.chars().any(char::is_control) {
        return Err(invalid(
            "ids",
            "each ID or symbol must be nonempty and at most 256 bytes, with no control characters",
        ));
    }
    if let Some(prefix) = raw.get(..4).and_then(prefix) {
        let id = format!("{prefix}{}", &raw[4..]);
        if id_prefix(&id).is_some() {
            return Ok(id);
        }
    }
    Ok(raw.to_owned())
}

#[derive(Serialize)]
struct RecordRow {
    input: String,
    id: Option<String>,
    status: &'static str,
    link_offset: usize,
    cross_references: BTreeMap<String, Vec<String>>,
}

impl RecordRow {
    fn tsv_prefix(&self) -> String {
        let input = if self.input.starts_with(['=', '+', '-', '@']) {
            format!("'{}", self.input)
        } else {
            self.input.clone()
        };
        tsv_line(
            [
                input,
                self.id.clone().unwrap_or_default(),
                self.status.to_owned(),
                self.link_offset.to_string(),
            ]
            .into_iter(),
        )
    }

    fn row_bytes(&self, tsv: bool) -> usize {
        if !tsv {
            // Include the comma between rows.
            return json!(self).to_string().len() + 1;
        }
        // Serialized prefix includes two quotes, which cover the escaped newline.
        // Separating tabs for the class columns are charged against the final
        // column set because a later row can introduce a new class.
        json!(self.tsv_prefix()).to_string().len()
            + self
                .cross_references
                .values()
                .map(|links| links.iter().map(String::len).sum::<usize>() + 2 * (links.len() - 1))
                .sum::<usize>()
    }
}

impl FlyBaseServer {
    pub(super) fn record_cross_reference_options(&self) -> CallToolResult {
        let targets: Vec<_> = TARGETS
            .iter()
            .map(|(prefix, label)| json!({"prefix": prefix, "label": label}))
            .collect();
        Envelope::new(TOOL, &self.release)
            .summary(
                "Export internal FlyBase record cross-references, grouped by linked record class.",
            )
            .field("mode", json!(Mode::RecordCrossReferences))
            .field("available", json!(self.record_xrefs.is_some()))
            .field("target_classes", json!(targets))
            .finish()
    }

    pub(super) async fn record_cross_references(
        &self,
        p: BatchParams,
    ) -> ToolResult<CallToolResult> {
        if !p.fields.is_empty() {
            return Err(invalid("fields", "only available in report_fields mode"));
        }
        let total = p.ids.len();
        if total == 0 {
            return Err(invalid(
                "ids",
                "provide at least one FlyBase ID or exact symbol",
            ));
        }
        if total > MAX_DOWNLOAD_IDS {
            return Err(ToolError::LimitExceeded {
                tool: TOOL,
                message: format!("{MAX_DOWNLOAD_IDS} IDs or symbols max per call ({total} given)"),
            });
        }
        let offset = p.offset.unwrap_or(0);
        let limit = p.limit.unwrap_or(MAX_PAGE_ROWS);
        if offset >= total {
            return Err(invalid("offset", "must identify a row in the input list"));
        }
        if !(1..=MAX_PAGE_ROWS).contains(&limit) {
            return Err(invalid(
                "limit",
                format!("must be between 1 and {MAX_PAGE_ROWS}"),
            ));
        }

        let mut targets = BTreeSet::new();
        if let Some(classes) = &p.target_classes {
            if !(1..=MAX_TARGET_CLASSES).contains(&classes.len()) {
                return Err(invalid(
                    "target_classes",
                    format!("choose between 1 and {MAX_TARGET_CLASSES} prefixes"),
                ));
            }
            for class in classes {
                let class = prefix(class).ok_or_else(|| {
                    invalid(
                        "target_classes",
                        "use FlyBase ID prefixes such as FBgn or FBal",
                    )
                })?;
                if matches!(class.as_str(), "FBmc" | "FBms") {
                    return Err(invalid(
                        "target_classes",
                        "engineered plasmids and molecular segments are grouped under FBtp",
                    ));
                }
                if !TARGETS.iter().any(|(known, _)| *known == class) {
                    return Err(invalid(
                        "target_classes",
                        format!("unknown target class {class}; use list_options=true"),
                    ));
                }
                targets.insert(class);
            }
        }
        let end = (offset + limit).min(total);
        let mut inputs = Vec::with_capacity(end - offset);
        for (index, input) in p.ids.iter().enumerate() {
            let input = input_token(input)?;
            if (offset..end).contains(&index) {
                inputs.push(input);
            }
        }

        let catalogs = self.record_xrefs.clone().ok_or_else(unavailable)?;
        let permit = self.limits.heavy(TOOL).await?;
        let release = self.release.clone();
        let start_link = p.link_offset.unwrap_or(0);
        let tsv = p.format.unwrap_or_default() == Format::Tsv;
        tokio::task::spawn_blocking(move || {
            // Keep the permit through blocking I/O if the calling request is canceled.
            let _permit = permit;
            record_page(
                &catalogs,
                &release,
                inputs,
                &targets,
                PageStart {
                    offset,
                    start_link,
                    total,
                },
                tsv,
            )
        })
        .await
        .map_err(|error| {
            tracing::error!(tool = TOOL, %error, "record cross-reference reader failed");
            unavailable()
        })?
    }
}

struct PageStart {
    offset: usize,
    start_link: usize,
    total: usize,
}

fn load_row(
    catalogs: &RecordXrefs,
    input: String,
    targets: &BTreeSet<String>,
    link_offset: usize,
    bytes_left: &mut u64,
) -> ToolResult<Option<(RecordRow, Option<usize>)>> {
    let record = (|| -> anyhow::Result<_> {
        let id = catalogs.resolve(&input)?;
        let groups = match &id {
            Some(id) => catalogs.associations(id, targets, bytes_left)?,
            None => Some(BTreeMap::new()),
        };
        Ok((id, groups))
    })()
    .map_err(|error| {
        tracing::error!(tool = TOOL, %error, "record cross-reference catalog read failed");
        unavailable()
    })?;
    let (id, Some(groups)) = record else {
        return Ok(None);
    };
    let total_links: usize = groups.values().map(BTreeSet::len).sum();
    if link_offset != 0 && link_offset >= total_links {
        return Err(invalid(
            "link_offset",
            "must identify a cross-reference within the input at offset",
        ));
    }

    let mut links = groups
        .into_iter()
        .flat_map(|(class, ids)| ids.into_iter().map(move |id| (class.clone(), id)));
    let mut cross_references = BTreeMap::<String, Vec<String>>::new();
    for (class, id) in links.by_ref().skip(link_offset).take(LINKS_PER_RECORD) {
        cross_references.entry(class).or_default().push(id);
    }
    let next_link =
        (link_offset + LINKS_PER_RECORD < total_links).then_some(link_offset + LINKS_PER_RECORD);
    let status = if id.is_some() { "resolved" } else { "unknown" };
    Ok(Some((
        RecordRow {
            input,
            id,
            status,
            link_offset,
            cross_references,
        },
        next_link,
    )))
}

fn record_page(
    catalogs: &RecordXrefs,
    release: &str,
    inputs: Vec<String>,
    targets: &BTreeSet<String>,
    PageStart {
        offset,
        start_link,
        total,
    }: PageStart,
    tsv: bool,
) -> ToolResult<CallToolResult> {
    let mut rows = Vec::new();
    let mut columns = BTreeSet::<String>::new();
    // Reserve space for the envelope, metadata, and fixed TSV headers.
    let mut bytes = 1_024 + json!(release).to_string().len();
    let mut next_offset = offset;
    let mut next_link = None;
    let mut bytes_left = MAX_SCAN_BYTES;
    for input in inputs {
        let link_offset = if rows.is_empty() { start_link } else { 0 };
        let Some((row, continuation)) =
            load_row(catalogs, input, targets, link_offset, &mut bytes_left)?
        else {
            if rows.is_empty() {
                return Err(ToolError::LimitExceeded {
                    tool: TOOL,
                    message: "record exceeds the cross-reference catalog scan limit".into(),
                });
            }
            break;
        };
        let new_columns = row
            .cross_references
            .keys()
            .filter(|c| !columns.contains(*c))
            .count();
        let column_count = columns.len() + new_columns;
        let row_bytes = row.row_bytes(tsv);
        // Column names are four ASCII bytes. This covers the encoded column
        // list and TSV header, including their delimiters.
        let column_bytes = column_count * 16
            + if tsv {
                2 * (rows.len() + 1) * column_count
            } else {
                0
            };
        if bytes + row_bytes + column_bytes > MAX_JSON_BYTES {
            if rows.is_empty() {
                return Err(page_limit());
            }
            break;
        }
        bytes += row_bytes;
        columns.extend(row.cross_references.keys().cloned());
        rows.push(row);
        if continuation.is_some() {
            next_link = continuation;
            break;
        }
        next_offset += 1;
    }

    let count = rows.len();
    let mut env = Envelope::new(TOOL, release)
        .summary(format!("{count} record cross-reference row(s)."))
        .field("mode", json!(Mode::RecordCrossReferences))
        .field("columns", json!(columns))
        .field("total", json!(total))
        .field("offset", json!(offset))
        .field("link_offset", json!(start_link))
        .field(
            "next_offset",
            json!((next_offset < total).then_some(next_offset)),
        )
        .field("next_link_offset", json!(next_link));
    if tsv {
        let mut table = tsv_line(
            ["input", "id", "status", "link_offset"]
                .into_iter()
                .chain(columns.iter().map(String::as_str))
                .map(String::from),
        );
        table.push('\n');
        for row in rows {
            table.push_str(&row.tsv_prefix());
            for class in &columns {
                table.push('\t');
                if let Some(links) = row.cross_references.get(class) {
                    table.push_str(&links.join("; "));
                }
            }
            table.push('\n');
        }
        env = env
            .field("tsv", Value::String(table))
            .field("row_count", json!(count));
    } else {
        env = env.field("rows", json!(rows));
    }
    env.finish_with_truncation(|_| Err(page_limit()))
}

#[cfg(test)]
mod tests;
