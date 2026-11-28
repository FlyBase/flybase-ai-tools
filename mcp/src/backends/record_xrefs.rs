//! Read-only release catalogs used by the website's record cross-reference export.
use anyhow::{Context, Result, ensure};
use regex::{Captures, Regex};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Component, Path},
    sync::{LazyLock, Mutex},
};

const REPORT_PREFIXES: [&str; 3] = ["FBst", "FBlc", "FBtc"];
const MAX_LINE_BYTES: u64 = 1024 * 1024;
pub const MAX_SCAN_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_ID_BYTES: usize = 32;

pub type Groups = BTreeMap<String, BTreeSet<String>>;

pub struct RecordXrefs {
    maps: Mutex<Connection>,
    offsets: Mutex<Connection>,
    catalogs: HashMap<&'static str, Mutex<File>>,
    report_ids: HashSet<String>,
}

#[derive(Deserialize)]
struct Source {
    file: String,
    records: u64,
    bytes: u64,
    sha256: String,
}

fn read_index(path: &Path, release: &str) -> Result<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", path.display()))?;
    db.pragma_update(None, "cache_size", -2048)
        .with_context(|| format!("configure {}", path.display()))?;
    for (key, expected) in [
        ("schema", "1"),
        ("scope", "full-release"),
        ("complete", "true"),
        ("release", release),
    ] {
        ensure!(
            metadata(&db, path, key)? == expected,
            "incompatible {key} in {}",
            path.display()
        );
    }
    Ok(db)
}

fn metadata(db: &Connection, path: &Path, key: &str) -> Result<String> {
    db.query_row("select value from metadata where key=?1", [key], |r| {
        r.get(0)
    })
    .with_context(|| format!("read metadata key {key} from {}", path.display()))
}

fn report_ids(root: &Path, release: &str) -> Result<HashSet<String>> {
    let file =
        File::open(root.join("report-membership.tsv")).context("open report-membership.tsv")?;
    ensure!(
        file.metadata()?.len() <= 8 * 1024 * 1024,
        "report membership file too large"
    );
    let mut lines = BufReader::new(file).lines();
    ensure!(
        lines.next().transpose()?.as_deref() == Some(&format!("# release: {release}")),
        "incompatible report membership release"
    );
    let mut ids = HashSet::new();
    let mut previous = String::new();
    let mut prefixes = HashSet::new();
    let mut complete = false;
    for line in lines {
        let id = line?;
        ensure!(
            !complete,
            "unexpected content after report membership footer"
        );
        if id == "# complete" {
            complete = true;
            continue;
        }
        let prefix = id_prefix(&id)
            .filter(|p| REPORT_PREFIXES.contains(p))
            .context("invalid report membership ID")?;
        prefixes.insert(prefix.to_owned());
        ensure!(
            id > previous,
            "report membership IDs must be sorted and unique"
        );
        previous.clone_from(&id);
        ids.insert(id);
    }
    ensure!(
        complete && prefixes.len() == REPORT_PREFIXES.len(),
        "incomplete report membership"
    );
    Ok(ids)
}

pub fn id_prefix(id: &str) -> Option<&str> {
    let bytes = id.as_bytes();
    (matches!(bytes, [b'F', b'B', a, b, rest @ ..]
        if a.is_ascii_lowercase() && b.is_ascii_lowercase() && !rest.is_empty() && rest.iter().all(u8::is_ascii_digit))
        && bytes.len() <= MAX_ID_BYTES)
        .then(|| &id[..4])
}

static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^\t]+)\t.+\n?\z").unwrap());

fn identifier(value: &str) -> &str {
    IDENTIFIER
        .captures(value)
        .map_or(value, |capture| capture.get(1).unwrap().as_str())
}

fn symbol_key(input: &str) -> String {
    static TAG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)<\s*(/?)\s*(up|down)\s*>").unwrap());
    static GREEK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&([A-Za-z][A-Za-z]?gr);").unwrap());
    let key = TAG.replace_all(input, |c: &Captures<'_>| {
        let token = if &c[1] == "/" { "]" } else { "[" };
        token.repeat(if c[2].eq_ignore_ascii_case("down") {
            2
        } else {
            1
        })
    });
    if let Some(c) = GREEK.captures(&key)
        && let Some(name) = greek_name(&c[1])
    {
        return key.replace(&c[0], &name);
    }
    key.into_owned()
}

fn greek_name(entity: &str) -> Option<String> {
    let stem = entity.strip_suffix("gr")?;
    let name = match stem.to_ascii_lowercase().as_str() {
        "a" => "alpha",
        "b" => "beta",
        "g" => "gamma",
        "d" => "delta",
        "e" => "epsilon",
        "z" => "zeta",
        "ee" => "eta",
        "th" => "theta",
        "i" => "iota",
        "k" => "kappa",
        "l" => "lambda",
        "m" => "mu",
        "n" => "nu",
        "x" => "xi",
        "o" => "omicron",
        "p" => "pi",
        "r" => "rho",
        "s" => "sigma",
        "sf" if stem == "sf" => "s",
        "t" => "tau",
        "u" => "upsilon",
        "ph" => "phi",
        "kh" => "chi",
        "ps" => "psi",
        "oh" => "omega",
        _ => return None,
    };
    if stem.bytes().all(|b| b.is_ascii_uppercase()) {
        Some(format!("{}{}", name[..1].to_ascii_uppercase(), &name[1..]))
    } else if stem.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(name.to_owned())
    } else {
        None
    }
}

impl RecordXrefs {
    pub fn load(root: &Path, release: &str) -> Result<Self> {
        let report_ids = report_ids(root, release)
            .with_context(|| format!("load {}", root.join("report-membership.tsv").display()))?;

        let maps_path = root.join("maps.sqlite");
        let maps = read_index(&maps_path, release)?;
        let maps_sources: serde_json::Value =
            serde_json::from_str(&metadata(&maps, &maps_path, "sources")?)
                .with_context(|| format!("parse sources metadata in {}", maps_path.display()))?;

        let offsets_path = root.join("relationship-index.sqlite");
        let offsets = read_index(&offsets_path, release)?;
        let sources: HashMap<String, Source> =
            serde_json::from_str(&metadata(&offsets, &offsets_path, "sources")?)
                .with_context(|| format!("parse sources metadata in {}", offsets_path.display()))?;

        let mut open_catalogs = Vec::new();
        for name in ["general", "references", "stock_objects"] {
            let source = sources
                .get(name)
                .with_context(|| format!("missing {name} source in {}", offsets_path.display()))?;
            let relative = Path::new(&source.file);
            ensure!(
                !source.file.is_empty()
                    && relative
                        .components()
                        .all(|c| matches!(c, Component::Normal(_))),
                "invalid {name} catalog path in {}",
                offsets_path.display()
            );
            let catalog_path = root.join("relationship-sources").join(relative);
            let file = File::open(&catalog_path)
                .with_context(|| format!("open {name} catalog {}", catalog_path.display()))?;
            let actual_size = file
                .metadata()
                .with_context(|| format!("read catalog size for {}", catalog_path.display()))?
                .len();
            ensure!(
                actual_size == source.bytes,
                "{name} catalog size mismatch in {}",
                catalog_path.display()
            );
            open_catalogs.push((name, source, catalog_path, file));
        }

        for name in ["symbols", "symbol_ids"] {
            let count: i64 = maps
                .query_row("select count(*) from maps where source=?1", [name], |r| {
                    r.get(0)
                })
                .with_context(|| format!("count {name} map entries in {}", maps_path.display()))?;
            ensure!(
                count > 0 && maps_sources[name]["rows"].as_i64() == Some(count),
                "incomplete {name} map in {}",
                maps_path.display()
            );
        }
        for (name, source, _, _) in &open_catalogs {
            let count: i64 = offsets
                .query_row(
                    "select count(*) from offsets where source=?1",
                    [name],
                    |r| r.get(0),
                )
                .with_context(|| format!("count {name} offsets in {}", offsets_path.display()))?;
            ensure!(
                count > 0 && u64::try_from(count)? == source.records,
                "incomplete {name} catalog index in {}",
                offsets_path.display()
            );
        }

        let mut catalogs = HashMap::new();
        for (name, source, path, mut file) in open_catalogs {
            let mut sha = Sha256::new();
            std::io::copy(&mut file, &mut sha)
                .with_context(|| format!("hash {name} catalog {}", path.display()))?;
            ensure!(
                format!("{:x}", sha.finalize()) == source.sha256,
                "{name} catalog hash mismatch in {}",
                path.display()
            );
            catalogs.insert(name, Mutex::new(file));
        }
        Ok(Self {
            maps: Mutex::new(maps),
            offsets: Mutex::new(offsets),
            catalogs,
            report_ids,
        })
    }

    pub fn resolve(&self, input: &str) -> Result<Option<String>> {
        if input.starts_with("FBrf") && input.len() == 11 && id_prefix(input).is_some()
            || self.report_ids.contains(input)
        {
            return Ok(Some(input.to_owned()));
        }
        let db = self
            .maps
            .lock()
            .map_err(|_| anyhow::anyhow!("lock symbol index"))?;
        let mut query = db.prepare_cached("select value from maps where source=?1 and id=?2")?;
        let mut get = |source, key| -> Result<Option<String>> {
            Ok(query
                .query_row([source, key], |r| r.get::<_, String>(0))
                .optional()?
                .filter(|s| !s.is_empty() && s != "0"))
        };
        if let Some(prefix) = id_prefix(input) {
            if REPORT_PREFIXES.contains(&prefix) {
                return Ok(None);
            }
            let Some(legacy) = get("symbols", input)? else {
                return Ok(None);
            };
            let utf8 = get("symbols", &format!("utf8 {input}"))?;
            let display = utf8.as_deref().unwrap_or_else(|| identifier(&legacy));
            return Ok((display != input).then(|| input.to_owned()));
        }
        Ok(get("symbol_ids", &symbol_key(input))?.and_then(|v| {
            let id = identifier(&v);
            id_prefix(id).is_some().then(|| id.to_owned())
        }))
    }

    /// Return None when this page has spent its catalog scan allowance.
    pub fn associations(
        &self,
        id: &str,
        targets: &BTreeSet<String>,
        bytes_left: &mut u64,
    ) -> Result<Option<Groups>> {
        let source = match &id[..4] {
            "FBrf" => "references",
            "FBst" => "stock_objects",
            _ => "general",
        };
        let position: Option<i64> = self
            .offsets
            .lock()
            .map_err(|_| anyhow::anyhow!("lock relationship index"))?
            .prepare_cached("select position from offsets where source=?1 and id=?2")?
            .query_row([source, id], |r| r.get(0))
            .optional()?;
        let mut groups = Groups::new();
        let Some(position) = position else {
            return Ok(Some(groups));
        };
        let mut file = self.catalogs[source]
            .lock()
            .map_err(|_| anyhow::anyhow!("lock relationship catalog"))?;
        let position = u64::try_from(position).context("negative relationship offset")?;
        ensure!(
            position < file.metadata()?.len(),
            "relationship offset outside catalog"
        );
        file.seek(SeekFrom::Start(position))?;
        let mut reader = BufReader::new(&mut *file);
        let mut line = Vec::new();
        let mut first = true;
        loop {
            line.clear();
            let bytes = (&mut reader)
                .take(MAX_LINE_BYTES.min(*bytes_left) + 1)
                .read_until(b'\n', &mut line)? as u64;
            if bytes == 0 {
                break;
            }
            ensure!(
                bytes <= MAX_LINE_BYTES,
                "relationship catalog line exceeds its read limit"
            );
            if bytes > *bytes_left {
                return Ok(None);
            }
            *bytes_left -= bytes;
            let line = std::str::from_utf8(&line).context("invalid catalog encoding")?;
            let link = if source == "stock_objects" {
                let mut fields = line.split_ascii_whitespace();
                if fields.next() != Some(id) {
                    break;
                }
                let Some(link) = fields.next() else {
                    continue;
                };
                link
            } else {
                let Some(link) = link_from_line(source, id, line, first) else {
                    break;
                };
                first = false;
                link
            };
            let Some(prefix) = id_prefix(link) else {
                // Stock rows also carry non-record targets such as controlled vocabulary terms.
                if source == "stock_objects" {
                    continue;
                }
                anyhow::bail!("invalid associated FlyBase ID");
            };
            let group = match prefix {
                "FBmc" | "FBms" => "FBtp",
                _ => prefix,
            };
            if (targets.is_empty() || targets.contains(group))
                && match source {
                    "references" => [
                        "FBgn", "FBal", "FBtr", "FBpp", "FBtp", "FBti", "FBab", "FBba",
                    ]
                    .contains(&group),
                    "stock_objects" => ["FBgn", "FBal", "FBti", "FBab", "FBba"].contains(&group),
                    _ => true,
                }
            {
                groups
                    .entry(group.to_owned())
                    .or_default()
                    .insert(link.to_owned());
            }
        }
        Ok(Some(groups))
    }
}

fn link_from_line<'a>(source: &str, id: &str, line: &'a str, first: bool) -> Option<&'a str> {
    static GENERAL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\A(?:[ \t]+)(FB[a-z]{2}[0-9]+)").unwrap());
    static REFERENCE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\A(?:[ \t]+)(FB[a-z]{2}[0-9]+)\n").unwrap());
    let rest = if first { line.strip_prefix(id)? } else { line };
    let pattern = if source == "references" {
        &*REFERENCE
    } else {
        &*GENERAL
    };
    pattern.captures(rest)?.get(1).map(|m| m.as_str())
}
