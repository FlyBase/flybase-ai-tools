//! Image records projected from the XML behind a matching FlyBase website release.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File, io::BufReader, path::Path, sync::Arc};

#[derive(Deserialize)]
struct Projection {
    release: String,
    image_count: usize,
    images: HashMap<String, ImageRecord>,
}

pub struct ReleaseImages(HashMap<String, Arc<ImageRecord>>);

#[derive(Deserialize)]
pub struct ImageRecord {
    pub name: String,
    pub species: String,
    pub created: String,
    pub updated: String,
    pub caption: String,
    pub caption_terms: Vec<CaptionTerm>,
    pub annotations: Vec<Annotation>,
    pub regions: Vec<Region>,
    pub references: Vec<Reference>,
}

#[derive(Deserialize, Serialize)]
pub struct CaptionTerm {
    pub term_id: String,
    pub name: String,
}

#[derive(Deserialize, Serialize)]
pub struct Annotation {
    pub category: String,
    pub term_id: String,
    pub name: String,
    pub has_region: bool,
    pub reference_ids: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Region {
    pub term_id: String,
    pub coordinates: String,
    pub reference_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Reference {
    pub id: String,
    pub citation: Option<String>,
}

pub fn is_image_id(id: &str) -> bool {
    id.strip_prefix("FBim").is_some_and(|accession| {
        accession.len() == 7 && accession.bytes().all(|byte| byte.is_ascii_digit())
    })
}

impl ReleaseImages {
    pub fn load(path: &Path, release: &str) -> anyhow::Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::read(BufReader::new(file), release)
            .with_context(|| format!("read {}", path.display()))
    }

    fn read(reader: impl std::io::Read, release: &str) -> anyhow::Result<Self> {
        let projection: Projection = serde_json::from_reader(reader)?;
        ensure!(
            projection.release == release,
            "image projection is for {}, expected {release}",
            projection.release
        );
        ensure!(
            projection.image_count > 0 && projection.images.len() == projection.image_count,
            "incomplete image projection"
        );
        let mut images = HashMap::with_capacity(projection.images.len());
        for (id, record) in projection.images {
            ensure!(
                is_image_id(&id)
                    && !record.name.trim().is_empty()
                    && !record.species.trim().is_empty(),
                "invalid image record {id}"
            );
            images.insert(id, Arc::new(record));
        }
        Ok(Self(images))
    }

    pub fn get(&self, id: &str) -> Option<Arc<ImageRecord>> {
        self.0.get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_requires_a_complete_catalog_for_the_requested_release() {
        let fixture = include_bytes!("../../tests/fixtures/release_images.json");
        let images = ReleaseImages::read(&fixture[..], "FB2026_03").unwrap();
        assert!(images.get("FBim0000001").is_some());
        assert!(ReleaseImages::read(&fixture[..], "FB2026_02").is_err());
        let mut body: serde_json::Value = serde_json::from_slice(fixture).unwrap();
        body["images"]
            .as_object_mut()
            .unwrap()
            .remove("FBim0000001");
        let incomplete = serde_json::to_vec(&body).unwrap();
        assert!(ReleaseImages::read(incomplete.as_slice(), "FB2026_03").is_err());
    }
}
