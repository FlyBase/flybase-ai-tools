//! Read-only, indexed views of release bulk download files.
use super::record_xrefs::{RecordXrefs, id_prefix};
use anyhow::{Context, Result, ensure};
use flate2::read::ZlibDecoder;
use regex::Regex;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::{
    collections::HashSet,
    io::Read,
    path::Path,
    sync::{LazyLock, Mutex},
};

const LINES_PER_BLOCK: usize = 256;
const MAX_BLOCK_BYTES: u64 = 16 * 1024 * 1024;

// Seek only the posting lists for the requested IDs and the remaining source blocks.
// Deduplication lets a row that matches several IDs appear once.
const SELECT_BLOCKS: &str = "
    SELECT DISTINCT m.block
    FROM json_each(?2) AS input
    CROSS JOIN matches AS m
    WHERE m.file = ?1 AND m.id = input.value AND m.block >= ?3
    ORDER BY m.block
    LIMIT ?4";
const READ_BLOCK: &str = "SELECT data FROM blocks WHERE file = ?1 AND block = ?2";
const SELECT_ALIAS: &str = "SELECT value FROM aliases WHERE key = ?1";
const SELECT_EXTERNAL: &str = "SELECT id FROM external WHERE kind = ?1 AND key = ?2";

pub(crate) fn direct_id(input: &str) -> bool {
    id_prefix(input).is_some_and(|prefix| {
        matches!(
            prefix,
            "FBgn"
                | "FBgg"
                | "FBal"
                | "FBab"
                | "FBba"
                | "FBtr"
                | "FBpp"
                | "FBst"
                | "FBtp"
                | "FBmc"
                | "FBms"
                | "FBti"
                | "FBcl"
                | "FBlc"
                | "FBrf"
        )
    })
}

#[derive(Serialize)]
pub(crate) struct BulkFile {
    #[serde(skip)]
    id: i64,
    pub key: String,
    label: String,
    path: String,
    classes: Vec<String>,
    #[serde(skip)]
    pub line_count: usize,
}

impl BulkFile {
    pub fn url(&self, release: &str) -> String {
        format!(
            "https://s3ftp.flybase.org/releases/{release}/precomputed_files/{}",
            self.path
        )
    }

    fn is_physical_mitab(&self) -> bool {
        self.key == "physical_interactions"
            && self
                .path
                .starts_with("genes/physical_interactions_mitab_fb_")
    }
}

pub(crate) struct PrecomputedFiles {
    db: Mutex<Connection>,
    pub files: Vec<BulkFile>,
}

impl PrecomputedFiles {
    pub fn load(path: &Path, release: &str) -> Result<Self> {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open precomputed file index {}", path.display()))?;
        Self::from_connection(db, release)
            .with_context(|| format!("load precomputed file index {}", path.display()))
    }

    pub(crate) fn from_connection(db: Connection, release: &str) -> Result<Self> {
        db.pragma_update(None, "cache_size", -2048)?;
        for (key, expected) in [("schema", "1"), ("release", release), ("complete", "true")] {
            let actual: String = db
                .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
                    row.get(0)
                })
                .with_context(|| format!("read {key} metadata"))?;
            ensure!(actual == expected, "incompatible {key} metadata");
        }
        db.prepare_cached(SELECT_BLOCKS)?;
        db.prepare_cached(READ_BLOCK)?;
        db.prepare_cached(SELECT_ALIAS)?;
        db.prepare_cached(SELECT_EXTERNAL)?;
        let files = {
            let mut select = db.prepare(
                "SELECT id, key, label, path, classes, line_count, sha256 FROM files ORDER BY key",
            )?;
            let mut rows = select.query([])?;
            let mut files = Vec::new();
            while let Some(row) = rows.next()? {
                let key: String = row.get(1)?;
                let label: String = row.get(2)?;
                let path: String = row.get(3)?;
                let classes: Vec<String> = serde_json::from_str(row.get_ref(4)?.as_str()?)?;
                let line_count = usize::try_from(row.get::<_, i64>(5)?)?;
                let sha256: String = row.get(6)?;
                ensure!(
                    valid_component(&key)
                        && !label.is_empty()
                        && label.len() <= 256
                        && path.len() <= 256
                        && path.split('/').all(valid_component)
                        && !classes.is_empty()
                        && classes.iter().all(|class| {
                            matches!(class.as_bytes(), [b'F', b'B', a, b]
                                if a.is_ascii_lowercase() && b.is_ascii_lowercase())
                        })
                        && line_count > 0
                        && sha256.len() == 64
                        && sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid precomputed file metadata"
                );
                files.push(BulkFile {
                    id: row.get(0)?,
                    key,
                    label,
                    path,
                    classes,
                    line_count,
                });
            }
            ensure!(!files.is_empty(), "empty precomputed file catalog");
            files
        };
        Ok(Self {
            db: Mutex::new(db),
            files,
        })
    }

    pub fn resolve(&self, maps: &RecordXrefs, input: &str) -> Result<Vec<String>> {
        if let Some(id) = maps.resolve(input)? {
            return Ok(vec![id]);
        }
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("lock precomputed file index"))?;
        resolve_alternative(&db, input)
    }

    /// The start position can leave the first candidate block without matches.
    /// Every later candidate contains at least one matching line, so at most
    /// limit + 2 blocks are needed to fill the page and detect a next row.
    pub fn visit_matches(
        &self,
        file: &BulkFile,
        ids: &HashSet<String>,
        start: usize,
        limit: usize,
        mut visit: impl FnMut(usize, &str) -> bool,
    ) -> Result<()> {
        let ids_parameter = serde_json::to_string(
            &std::iter::once("")
                .chain(ids.iter().map(String::as_str))
                .collect::<Vec<_>>(),
        )?;
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("lock precomputed file index"))?;
        let mut candidates = db.prepare_cached(SELECT_BLOCKS)?;
        let mut blocks = candidates.query(params![
            file.id,
            ids_parameter,
            i64::try_from(start / LINES_PER_BLOCK)?,
            i64::try_from(limit + 2)?,
        ])?;
        let mut read = db.prepare_cached(READ_BLOCK)?;
        while let Some(row) = blocks.next()? {
            let block: i64 = row.get(0)?;
            let first = usize::try_from(block)?
                .checked_mul(LINES_PER_BLOCK)
                .context("invalid source block position")?;
            ensure!(first < file.line_count, "source block out of bounds");
            let bytes: Vec<u8> = read.query_row(params![file.id, block], |row| row.get(0))?;
            let mut text = String::new();
            ZlibDecoder::new(bytes.as_slice())
                .take(MAX_BLOCK_BYTES + 1)
                .read_to_string(&mut text)?;
            ensure!(
                text.len() as u64 <= MAX_BLOCK_BYTES,
                "source block exceeds size limit"
            );
            let lines = text.split_inclusive('\n');
            ensure!(
                lines.clone().count() == (file.line_count - first).min(LINES_PER_BLOCK),
                "source block has the wrong number of lines"
            );
            for (index, line) in lines.enumerate().skip(start.saturating_sub(first)) {
                if matching_line(line, ids, file.is_physical_mitab()) && !visit(first + index, line)
                {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

fn resolve_alternative(db: &Connection, input: &str) -> Result<Vec<String>> {
    let mut query = db.prepare_cached(SELECT_ALIAS)?;
    let mut ids = Vec::new();
    let mut has_alias = false;
    for prefix in ["sec ", "syn "] {
        let key = format!("{prefix}{input}");
        let value: Option<Vec<u8>> = query
            .query_row([key.as_bytes()], |row| row.get(0))
            .optional()?;
        if let Some(value) = value {
            for id in std::str::from_utf8(&value)?.split_ascii_whitespace() {
                has_alias = true;
                if id_prefix(id).is_some() && id.len() >= 7 {
                    ids.push(id.to_owned());
                }
            }
        }
    }
    if has_alias {
        return Ok(ids);
    }
    if id_prefix(input).is_some() && input.len() >= 7 {
        return Ok(vec![input.to_owned()]);
    }
    static ACCESSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(NP_\d+)\.\d+").unwrap());
    let accession = ACCESSION
        .captures(input)
        .and_then(|capture| capture.get(1))
        .map_or(input, |matched| matched.as_str());
    let mut query = db.prepare_cached(SELECT_EXTERNAL)?;
    for (kind, key) in [
        ("dgc", input),
        ("accession", accession),
        ("publication", input),
    ] {
        let mut rows = query.query([kind, key])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            if id_prefix(&id).is_some() && id.len() >= 7 {
                ids.push(id);
            }
        }
        if !ids.is_empty() {
            break;
        }
    }
    Ok(ids)
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn matching_line(line: &str, ids: &HashSet<String>, physical_mitab: bool) -> bool {
    let line = line.trim_end_matches(['\r', '\n']);
    line.starts_with('#')
        || line
            .split([' ', '\t', '\n'])
            .any(|token| ids.contains(token))
        || physical_mitab
            && line.split('\t').take(2).any(|cell| {
                cell.strip_prefix("flybase:")
                    .is_some_and(|id| ids.contains(id))
            })
}
