//! Database catalogue: maps `(species, category)` to a BLAST volume prefix under the database root
//! (design 3.2, 3.3). Any JSON file with a `databases` array of `{species, category, protein, prefix}`
//! works; extra fields are ignored.
use crate::types::Rejection;
use crate::validate::quoted;
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

#[derive(Deserialize)]
struct File {
    databases: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    species: String,
    category: String,
    protein: bool,
    prefix: String,
}

/// The databases one search runs against: their molecule type and absolute volume prefixes, in the
/// order the species were requested.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub protein: bool,
    pub prefixes: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Catalogue {
    /// category -> molecule type (one per category; checked at load).
    categories: BTreeMap<String, bool>,
    /// (species, category) -> absolute prefix.
    entries: HashMap<(String, String), PathBuf>,
}

fn is_code(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Catalogue {
    /// Load `path` and check every entry against `root`: the root and each prefix carry no whitespace or
    /// quote characters (space separates databases in `-db`), the prefix is relative and names an
    /// existing `.nsq`/`.psq` volume; no `(species, category)` pair repeats and each category has one
    /// molecule type. Any failure refuses to start, so a bad mount shows at boot rather than at the
    /// first job.
    pub fn load(path: impl AsRef<Path>, root: impl AsRef<Path>) -> anyhow::Result<Catalogue> {
        let path = path.as_ref();
        let root = std::path::absolute(root.as_ref())
            .with_context(|| format!("database root {}", root.as_ref().display()))?;
        if root
            .to_string_lossy()
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'')
        {
            bail!(
                "database root {} contains whitespace or quotes, which would split the -db list",
                root.display()
            );
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading catalogue {}", path.display()))?;
        let file: File = serde_json::from_str(&text)
            .with_context(|| format!("parsing catalogue {}", path.display()))?;
        if file.databases.is_empty() {
            bail!("catalogue {} lists no databases", path.display());
        }
        let mut categories = BTreeMap::new();
        let mut entries = HashMap::new();
        for e in file.databases {
            let name = format!("{} {}", e.species, e.category);
            if !is_code(&e.species) || !is_code(&e.category) {
                bail!("catalogue entry {name:?}: species and category must be [a-z0-9_]+");
            }
            if e.prefix.is_empty()
                || e.prefix
                    .chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'')
            {
                bail!(
                    "catalogue entry {name}: prefix {:?} is empty or contains whitespace or quotes",
                    e.prefix
                );
            }
            let rel = Path::new(&e.prefix);
            if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
                bail!(
                    "catalogue entry {name}: prefix {:?} must be relative to the database root, without '..'",
                    e.prefix
                );
            }
            let abs = root.join(rel);
            let ext = if e.protein { "psq" } else { "nsq" };
            let volume = PathBuf::from(format!("{}.{ext}", abs.display()));
            if !volume.is_file() {
                bail!(
                    "catalogue entry {name}: {} does not exist",
                    volume.display()
                );
            }
            match categories.get(&e.category) {
                Some(&p) if p != e.protein => bail!(
                    "catalogue category {} is listed as both protein and nucleotide",
                    e.category
                ),
                _ => {
                    categories.insert(e.category.clone(), e.protein);
                }
            }
            if entries.insert((e.species, e.category), abs).is_some() {
                bail!("catalogue entry {name} appears more than once");
            }
        }
        Ok(Catalogue {
            categories,
            entries,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Molecule type of `category` (`Some(true)` = protein), or `None` when no species has it.
    pub fn category_protein(&self, category: &str) -> Option<bool> {
        self.categories.get(category).copied()
    }

    /// The `invalid_database` rejection for a category no species has, listing the available ones.
    pub fn unknown_database(&self, category: &str) -> Rejection {
        let known: Vec<&str> = self.categories.keys().map(String::as_str).collect();
        Rejection::database(format!(
            "database {} is not available; available: {}",
            quoted(category),
            known.join(", ")
        ))
    }

    /// Resolve every species for `category`. Unknown category: `invalid_database`. Species lacking it:
    /// `invalid_species` naming all of them (no silent subset).
    pub fn resolve(&self, species: &[String], category: &str) -> Result<Resolved, Rejection> {
        let protein = self
            .category_protein(category)
            .ok_or_else(|| self.unknown_database(category))?;
        let mut prefixes = Vec::with_capacity(species.len());
        let mut missing = Vec::new();
        for s in species {
            match self.entries.get(&(s.clone(), category.to_string())) {
                Some(p) => prefixes.push(p.clone()),
                None => missing.push(s.as_str()),
            }
        }
        if !missing.is_empty() {
            return Err(Rejection::species(format!(
                "no {category} database for species: {}",
                missing.join(", ")
            )));
        }
        Ok(Resolved { protein, prefixes })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::types::Code;
    use serde_json::json;

    pub(crate) fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture")
    }

    pub(crate) fn fixture() -> Catalogue {
        let d = fixture_dir();
        Catalogue::load(d.join("catalogue.json"), d.join("db")).unwrap()
    }

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A temp root with the volume files `files` and a catalogue of `entries`.
    fn temp(
        entries: serde_json::Value,
        files: &[&str],
    ) -> (tempfile::TempDir, anyhow::Result<Catalogue>) {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        let cat = dir.path().join("catalogue.json");
        std::fs::write(
            &cat,
            json!({ "version": 1, "databases": entries }).to_string(),
        )
        .unwrap();
        let r = Catalogue::load(&cat, dir.path());
        (dir, r)
    }

    fn refusal(entries: serde_json::Value, files: &[&str]) -> String {
        let (_d, r) = temp(entries, files);
        format!("{:#}", r.unwrap_err())
    }

    #[test]
    fn fixture_loads_with_hyphenated_prefix() {
        let c = fixture();
        assert_eq!(c.len(), 6);
        let r = c.resolve(&sv(&["dmel"]), "assembly").unwrap();
        assert!(!r.protein);
        assert_eq!(
            r.prefixes,
            vec![fixture_dir().join("db/dmel/dmel-assembly")]
        );
        assert!(r.prefixes[0].is_absolute());
        let r = c.resolve(&sv(&["dmel"]), "translation").unwrap();
        assert!(r.protein);
    }

    #[test]
    fn two_species_keep_request_order() {
        let r = fixture()
            .resolve(&sv(&["dsim", "dmel"]), "assembly")
            .unwrap();
        assert_eq!(
            r.prefixes,
            vec![
                fixture_dir().join("db/dsim/dsim-assembly"),
                fixture_dir().join("db/dmel/dmel-assembly")
            ]
        );
    }

    #[test]
    fn missing_pair_names_the_species() {
        let e = fixture()
            .resolve(&sv(&["dmel", "dsim"]), "transcript")
            .unwrap_err();
        assert_eq!(e.code, Code::InvalidSpecies);
        assert!(
            e.message.contains("dsim") && !e.message.contains("dmel"),
            "{}",
            e.message
        );
    }

    #[test]
    fn unknown_category_is_invalid_database() {
        let e = fixture().resolve(&sv(&["dmel"]), "est").unwrap_err();
        assert_eq!(e.code, Code::InvalidDatabase);
        assert!(
            e.message.contains("assembly"),
            "lists what is available: {}",
            e.message
        );
    }

    #[test]
    fn extra_fields_are_ignored() {
        let (_d, r) = temp(
            json!([{"species": "dmel", "category": "assembly", "protein": false,
                    "prefix": "dmel/dmel-assembly", "label": "x", "sequences": 3}]),
            &["dmel/dmel-assembly.nsq"],
        );
        assert_eq!(r.unwrap().len(), 1);
    }

    #[test]
    fn refuses_missing_volume() {
        let m = refusal(
            json!([{"species": "dmel", "category": "assembly", "protein": false, "prefix": "dmel/dmel-assembly"}]),
            &[],
        );
        assert!(m.contains("does not exist"), "{m}");
        // A nucleotide entry needs .nsq; a .psq alone does not satisfy it.
        let m = refusal(
            json!([{"species": "dmel", "category": "assembly", "protein": false, "prefix": "dmel/dmel-assembly"}]),
            &["dmel/dmel-assembly.psq"],
        );
        assert!(m.contains("dmel-assembly.nsq"), "{m}");
    }

    #[test]
    fn refuses_whitespace_and_quotes_in_prefix() {
        for prefix in [
            "dmel/dmel assembly",
            "dmel/dmel\tassembly",
            "dmel/\"x",
            "dmel/'x",
        ] {
            let m = refusal(
                json!([{"species": "dmel", "category": "assembly", "protein": false, "prefix": prefix}]),
                &[],
            );
            assert!(m.contains("whitespace or quotes"), "{prefix}: {m}");
        }
    }

    #[test]
    fn refuses_whitespace_in_database_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("blast db");
        std::fs::create_dir_all(root.join("dmel")).unwrap();
        std::fs::write(root.join("dmel/dmel-assembly.nsq"), b"").unwrap();
        let cat = dir.path().join("catalogue.json");
        std::fs::write(
            &cat,
            json!({"databases": [{"species": "dmel", "category": "assembly", "protein": false,
                "prefix": "dmel/dmel-assembly"}]})
            .to_string(),
        )
        .unwrap();
        let m = format!("{:#}", Catalogue::load(&cat, &root).unwrap_err());
        assert!(
            m.contains("database root") && m.contains("whitespace"),
            "{m}"
        );
    }

    #[test]
    fn refuses_absolute_or_parent_prefix() {
        for prefix in ["/etc/passwd", "../outside", "dmel/../../x"] {
            let m = refusal(
                json!([{"species": "dmel", "category": "assembly", "protein": false, "prefix": prefix}]),
                &[],
            );
            assert!(m.contains("relative"), "{prefix}: {m}");
        }
    }

    #[test]
    fn refuses_duplicate_pair() {
        let m = refusal(
            json!([
                {"species": "dmel", "category": "assembly", "protein": false, "prefix": "dmel/dmel-assembly"},
                {"species": "dmel", "category": "assembly", "protein": false, "prefix": "dmel/dmel-assembly"}
            ]),
            &["dmel/dmel-assembly.nsq"],
        );
        assert!(m.contains("more than once"), "{m}");
    }

    #[test]
    fn refuses_mixed_molecule_category_and_bad_codes() {
        let m = refusal(
            json!([
                {"species": "dmel", "category": "genbank", "protein": false, "prefix": "a"},
                {"species": "dsim", "category": "genbank", "protein": true, "prefix": "b"}
            ]),
            &["a.nsq", "b.psq"],
        );
        assert!(m.contains("both protein and nucleotide"), "{m}");
        let m = refusal(
            json!([{"species": "Dmel", "category": "assembly", "protein": false, "prefix": "a"}]),
            &["a.nsq"],
        );
        assert!(m.contains("[a-z0-9_]+"), "{m}");
        let m = refusal(json!([]), &[]);
        assert!(m.contains("no databases"), "{m}");
    }
}
