//! Term reports projected from a matching FlyBase website release.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::Path,
    sync::Arc,
};

pub fn vocabulary(id: &str) -> Option<&'static str> {
    let (prefix, accession) = id.split_once(':')?;
    if accession.is_empty() || !accession.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match prefix {
        "FBbi" => Some("Imaging Methods"),
        "CHEBI" => Some("Application and Biological Role"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct Projection {
    release: String,
    term_counts: HashMap<String, usize>,
    edge_count: usize,
    alias_count: usize,
    terms: HashMap<String, ReleaseTerm>,
    aliases: HashMap<String, String>,
}

pub struct ReleaseTerms {
    terms: HashMap<String, Arc<ReleaseTerm>>,
    aliases: HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct ReleaseTerm {
    pub name: String,
    pub definition: Option<String>,
    pub comment: Option<String>,
    pub is_obsolete: bool,
    pub synonyms: Vec<Synonym>,
    pub parents: Vec<Relation>,
    #[serde(skip)]
    pub children: Vec<Relation>,
}

#[derive(Deserialize, Serialize)]
pub struct Synonym {
    name: String,
    scope: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Relation {
    relationship: String,
    term_id: String,
    name: String,
}

impl ReleaseTerms {
    pub fn load(path: &Path, release: &str) -> anyhow::Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::read(BufReader::new(file), release)
            .with_context(|| format!("read {}", path.display()))
    }

    fn read(reader: impl std::io::Read, release: &str) -> anyhow::Result<Self> {
        let mut projection: Projection = serde_json::from_reader(reader)?;
        ensure!(
            projection.release == release,
            "ontology projection is for {}, expected {release}",
            projection.release
        );
        let mut term_counts = HashMap::new();
        let mut edge_count = 0;
        let mut children = HashMap::<String, Vec<Relation>>::new();
        for (id, term) in &projection.terms {
            ensure!(
                vocabulary(id).is_some() && !term.name.is_empty(),
                "invalid ontology term {id}"
            );
            let (prefix, _) = id.split_once(':').expect("validated ID");
            *term_counts.entry(prefix.to_owned()).or_insert(0) += 1;
            let mut parents = HashSet::new();
            for parent in &term.parents {
                ensure!(
                    vocabulary(&parent.term_id).is_some()
                        && !parent.relationship.is_empty()
                        && !parent.name.is_empty()
                        && parents.insert((&parent.relationship, &parent.term_id)),
                    "invalid ontology parent of {id}"
                );
                edge_count += 1;
                children
                    .entry(parent.term_id.clone())
                    .or_default()
                    .push(Relation {
                        relationship: parent.relationship.clone(),
                        term_id: id.clone(),
                        name: term.name.clone(),
                    });
            }
        }
        ensure!(
            term_counts.len() == 2
                && term_counts == projection.term_counts
                && edge_count == projection.edge_count
                && projection.aliases.len() == projection.alias_count,
            "incomplete ontology projection"
        );
        for (alias, target) in &projection.aliases {
            ensure!(
                vocabulary(alias).is_some()
                    && vocabulary(alias) == vocabulary(target)
                    && !projection.terms.contains_key(alias)
                    && projection.terms.contains_key(target),
                "invalid alternate ontology ID {alias}"
            );
        }
        let terms = projection
            .terms
            .drain()
            .map(|(id, mut term)| {
                term.children = children.remove(&id).unwrap_or_default();
                term.children.sort_by(relation_order);
                term.parents.sort_by(relation_order);
                (id, Arc::new(term))
            })
            .collect();
        Ok(Self {
            terms,
            aliases: projection.aliases,
        })
    }

    pub fn get(&self, id: &str) -> Option<(&str, Arc<ReleaseTerm>)> {
        let id = self.aliases.get(id).map(String::as_str).unwrap_or(id);
        self.terms
            .get_key_value(id)
            .map(|(id, term)| (id.as_str(), Arc::clone(term)))
    }
}

fn relation_order(left: &Relation, right: &Relation) -> std::cmp::Ordering {
    (&left.relationship, &left.name, &left.term_id).cmp(&(
        &right.relationship,
        &right.name,
        &right.term_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/release_terms.json");

    #[test]
    fn rejects_mismatched_and_incomplete_projections() {
        let mismatch = ReleaseTerms::read(FIXTURE, "FB2026_03").err().unwrap();
        assert!(mismatch.to_string().contains("expected FB2026_03"));

        let mut data: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        data["terms"].as_object_mut().unwrap().remove("CHEBI:27780");
        let data = serde_json::to_vec(&data).unwrap();
        let incomplete = ReleaseTerms::read(data.as_slice(), "FB_TEST")
            .err()
            .unwrap();
        assert!(incomplete.to_string().contains("incomplete"));
    }
}
