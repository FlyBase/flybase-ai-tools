use super::{BatchParams, Format, MAX_DOWNLOAD_IDS, MAX_PAGE_ROWS, Mode, TOOL};
use crate::{
    backends::precomputed_files::{BulkFile, PrecomputedFiles, direct_id},
    error::{ToolError, ToolResult},
    limits::MAX_JSON_BYTES,
    output::Envelope,
    server::FlyBaseServer,
};
use rmcp::model::CallToolResult;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const MAX_FILES: usize = 64;

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
        message: "the index of precomputed files for this release is unavailable".into(),
    }
}

fn backend_error(error: impl std::fmt::Display) -> ToolError {
    tracing::error!(tool = TOOL, %error, "precomputed file index read failed");
    unavailable()
}

impl FlyBaseServer {
    pub(super) fn precomputed_file_options(&self) -> ToolResult<CallToolResult> {
        let files: Vec<_> = self
            .precomputed_files
            .iter()
            .flat_map(|index| &index.files)
            .map(|file| {
                let mut entry = json!(file);
                entry["url"] = json!(file.url(&self.release));
                entry
            })
            .collect();
        Envelope::new(TOOL, &self.release)
            .summary("Filter precomputed FlyBase release files by IDs. Select files by key.")
            .link("batch_download", "https://flybase.org/batchdownload".into())
            .field("mode", json!(Mode::PrecomputedFiles))
            .field("available", json!(self.precomputed_files.is_some()))
            .field(
                "identifier_lookup_available",
                json!(self.precomputed_files.is_some() && self.record_xrefs.is_some()),
            )
            .field("files", json!(files))
            .finish_with_truncation(|_| Err(page_limit()))
    }

    pub(super) async fn download_precomputed_files(
        &self,
        p: BatchParams,
    ) -> ToolResult<CallToolResult> {
        for (arg, present) in [
            ("fields", !p.fields.is_empty()),
            ("target_classes", p.target_classes.is_some()),
            ("link_offset", p.link_offset.is_some()),
        ] {
            if present {
                return Err(invalid(arg, "unavailable in precomputed_files mode"));
            }
        }
        if p.ids.is_empty() {
            return Err(invalid(
                "ids",
                "provide at least one FlyBase ID or exact symbol",
            ));
        }
        if p.ids.len() > MAX_DOWNLOAD_IDS {
            return Err(ToolError::LimitExceeded {
                tool: TOOL,
                message: format!(
                    "{MAX_DOWNLOAD_IDS} inputs max in precomputed_files mode; split the list"
                ),
            });
        }
        if p.ids.iter().any(|input| {
            input.trim().is_empty() || input.len() > 256 || input.chars().any(char::is_control)
        }) {
            return Err(invalid(
                "ids",
                "each input must be nonempty and at most 256 bytes, with no control characters",
            ));
        }
        if !(1..=MAX_FILES).contains(&p.files.len()) {
            return Err(invalid(
                "files",
                format!("choose between 1 and {MAX_FILES} file keys; use list_options=true"),
            ));
        }
        let limit = p.limit.unwrap_or(MAX_PAGE_ROWS);
        if !(1..=MAX_PAGE_ROWS).contains(&limit) {
            return Err(invalid(
                "limit",
                format!("must be between 1 and {MAX_PAGE_ROWS}"),
            ));
        }

        let index = self.precomputed_files.clone().ok_or_else(unavailable)?;
        let maps = self.record_xrefs.clone();
        let permit = self.limits.heavy(TOOL).await?;
        let release = self.release.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let files = select_files(&index, &p.files)?;
            let mut ids = HashSet::new();
            let mut unresolved_count = 0;
            let mut inputs = HashMap::new();
            for input in &p.ids {
                *inputs.entry(input.trim()).or_insert(0) += 1;
            }
            for (input, count) in inputs {
                if direct_id(input) {
                    ids.insert(input.to_owned());
                } else {
                    let maps = maps.as_deref().ok_or_else(|| ToolError::BackendUnavailable {
                        tool: TOOL,
                        message: "symbol and external identifier lookup is unavailable. Submit FlyBase IDs.".into(),
                    })?;
                    let resolved = index.resolve(maps, input).map_err(backend_error)?;
                    if resolved.is_empty() {
                        unresolved_count += count;
                    }
                    ids.extend(resolved);
                }
                if ids.len() > MAX_DOWNLOAD_IDS {
                    return Err(ToolError::LimitExceeded {
                        tool: TOOL,
                        message: format!("inputs resolved to more than {MAX_DOWNLOAD_IDS} distinct IDs. Split the list."),
                    });
                }
            }
            if ids.is_empty() {
                return Err(invalid("ids", "no inputs resolved to FlyBase IDs"));
            }

            read_page(&index, &files, &ids, &p, &release, unresolved_count)
        })
        .await
        .map_err(backend_error)?
    }
}

fn select_files<'a>(index: &'a PrecomputedFiles, keys: &[String]) -> ToolResult<Vec<&'a BulkFile>> {
    let mut seen = HashSet::new();
    keys.iter()
        .map(|key| {
            if !seen.insert(key) {
                return Err(invalid("files", "choose each file key once"));
            }
            index
                .files
                .iter()
                .find(|file| &file.key == key)
                .ok_or_else(|| invalid("files", "unknown file key; use list_options=true"))
        })
        .collect()
}

fn page_limit() -> ToolError {
    ToolError::LimitExceeded {
        tool: TOOL,
        message: "precomputed file page exceeds the response size limit".into(),
    }
}

fn read_page(
    index: &PrecomputedFiles,
    files: &[&BulkFile],
    ids: &HashSet<String>,
    p: &BatchParams,
    release: &str,
    unresolved_count: usize,
) -> ToolResult<CallToolResult> {
    let offset = p.offset.unwrap_or(0);
    let limit = p.limit.unwrap_or(MAX_PAGE_ROWS);
    let tsv = p.format.unwrap_or_default() == Format::Tsv;
    let total = files
        .iter()
        .try_fold(0usize, |total, file| total.checked_add(file.line_count))
        .ok_or_else(unavailable)?;
    if offset >= total {
        return Err(invalid(
            "offset",
            "use next_offset returned by the previous page",
        ));
    }

    let keys = json!(p.files);
    let mut bytes = 1024 + keys.to_string().len() + json!(release).to_string().len();
    let mut output_tsv = String::new();
    let mut output_rows = Vec::new();
    let mut count = 0;
    let mut next = None;
    let mut start = 0;
    for file in files {
        let end = start + file.line_count;
        if offset < end {
            index
                .visit_matches(
                    file,
                    ids,
                    offset.saturating_sub(start),
                    limit - count,
                    |ordinal, line| {
                        if count == limit {
                            next = Some(start + ordinal);
                            return false;
                        }
                        if tsv {
                            bytes += json!(line).to_string().len() - 2;
                            if bytes <= MAX_JSON_BYTES {
                                output_tsv.push_str(line);
                            }
                        } else {
                            let row = json!({"file": file.key, "line": line});
                            bytes += row.to_string().len() + 1;
                            if bytes <= MAX_JSON_BYTES {
                                output_rows.push(row);
                            }
                        }
                        if bytes > MAX_JSON_BYTES {
                            next = Some(start + ordinal);
                            return false;
                        }
                        count += 1;
                        true
                    },
                )
                .map_err(backend_error)?;
            if let Some(ordinal) = next {
                if count == 0 {
                    return Err(ToolError::LimitExceeded {
                        tool: TOOL,
                        message: format!(
                            "one source line at offset {ordinal} exceeds the response size limit; download the file at {}",
                            file.url(release)
                        ),
                    });
                }
                break;
            }
        }
        start = end;
    }

    let mut envelope = Envelope::new(TOOL, release)
        .summary(format!("{count} precomputed file line(s)."))
        .field("mode", json!(Mode::PrecomputedFiles))
        .field("files", keys)
        .field("offset", json!(offset))
        .field("next_offset", json!(next))
        .field("row_count", json!(count))
        .field("unresolved_count", json!(unresolved_count));
    if tsv {
        envelope = envelope.field("tsv", Value::String(output_tsv));
    } else {
        envelope = envelope.field("rows", Value::Array(output_rows));
    }
    envelope.finish_with_truncation(|_| Err(page_limit()))
}

#[cfg(test)]
mod tests;
