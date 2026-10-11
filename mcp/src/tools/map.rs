use crate::{
    backends::{chado::Chado, species::normalize_species},
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    report::jbrowse_link,
    server::FlyBaseServer,
    tools::{
        resolve::{looks_like_fb_id, normalize_fb_prefix},
        search::MAX_OFFSET,
        sequence::{MAX_REGION, Region},
    },
};
use deadpool_postgres::Transaction;
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FeatureType {
    Gene,
    Mrna,
    Ncrna,
    Exon,
    Cds,
    Intron,
    Insertion,
    TfBindingSite,
    RegulatoryRegion,
    PointMutation,
    Sgrna,
    RnaiReagent,
    ChromosomeBand,
    Oligo,
    PcrProduct,
    Breakpoint,
    PolyaSite,
    Tss,
    OriginOfReplication,
    Bac,
}

impl FeatureType {
    pub const ALL: &[FeatureType] = &[
        Self::Gene,
        Self::Mrna,
        Self::Ncrna,
        Self::Exon,
        Self::Cds,
        Self::Intron,
        Self::Insertion,
        Self::TfBindingSite,
        Self::RegulatoryRegion,
        Self::PointMutation,
        Self::Sgrna,
        Self::RnaiReagent,
        Self::ChromosomeBand,
        Self::Oligo,
        Self::PcrProduct,
        Self::Breakpoint,
        Self::PolyaSite,
        Self::Tss,
        Self::OriginOfReplication,
        Self::Bac,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Gene => "gene",
            Self::Mrna => "mrna",
            Self::Ncrna => "ncrna",
            Self::Exon => "exon",
            Self::Cds => "cds",
            Self::Intron => "intron",
            Self::Insertion => "insertion",
            Self::TfBindingSite => "tf_binding_site",
            Self::RegulatoryRegion => "regulatory_region",
            Self::PointMutation => "point_mutation",
            Self::Sgrna => "sgrna",
            Self::RnaiReagent => "rnai_reagent",
            Self::ChromosomeBand => "chromosome_band",
            Self::Oligo => "oligo",
            Self::PcrProduct => "pcr_product",
            Self::Breakpoint => "breakpoint",
            Self::PolyaSite => "polya_site",
            Self::Tss => "tss",
            Self::OriginOfReplication => "origin_of_replication",
            Self::Bac => "bac",
        }
    }
    /// The chado cvterm.name of the feature type.
    pub fn so_name(self) -> &'static str {
        match self {
            Self::Gene => "gene",
            Self::Mrna => "mRNA",
            Self::Ncrna => "ncRNA",
            Self::Exon => "exon",
            Self::Cds => "CDS",
            Self::Intron => "intron",
            Self::Insertion => "transposable_element_insertion_site",
            Self::TfBindingSite => "TF_binding_site",
            Self::RegulatoryRegion => "regulatory_region",
            Self::PointMutation => "point_mutation",
            Self::Sgrna => "sgRNA",
            Self::RnaiReagent => "RNAi_reagent",
            Self::ChromosomeBand => "chromosome_band",
            Self::Oligo => "oligo",
            Self::PcrProduct => "PCR_product",
            Self::Breakpoint => "chromosome_breakpoint",
            Self::PolyaSite => "polyA_site",
            Self::Tss => "TSS",
            Self::OriginOfReplication => "origin_of_replication",
            Self::Bac => "BAC_cloned_genomic_insert",
        }
    }
    pub fn catalog() -> Value {
        json!(
            Self::ALL
                .iter()
                .map(|t| json!({"name": t.name(), "so_type": t.so_name()}))
                .collect::<Vec<_>>()
        )
    }
    pub fn so_names(types: &[FeatureType]) -> Vec<String> {
        types.iter().map(|t| t.so_name().to_string()).collect()
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapParams {
    /// Genomic range, 1-based inclusive, max 500 kb. Provide this or `id`.
    pub region: Option<Region>,
    /// A located FlyBase feature (FBgn, FBti, FBtr, FBsf, ...) whose span is the range. Provide this or `region`.
    pub id: Option<String>,
    /// Feature types to return (default gene, mrna, insertion). Pass list_options=true to see them.
    pub feature_types: Option<Vec<FeatureType>>,
    /// Extra bases on each side of the range (max 100000).
    pub padding: Option<u32>,
    /// Species for `region` (default Dmel).
    pub species: Option<String>,
    /// Features per page (default 25, max 100).
    pub limit: Option<u32>,
    /// Features to skip; offset + limit may not exceed 10,000.
    pub offset: Option<u32>,
    /// If true, return only the feature type names.
    pub list_options: Option<bool>,
}

pub const MAX_PADDING: u32 = 100_000;
/// Largest coordinate chado can hold (featureloc.fmax is int4).
pub const MAX_COORD: u64 = i32::MAX as u64;

/// A query range on a golden path: chado coordinates (fmin 0-based) after padding and clamping to position 1.
pub struct Range {
    pub species: String,
    pub chromosome: String,
    pub fmin: i64,
    pub fmax: i64,
}

impl Range {
    pub fn from_region(species: String, r: &Region, padding: u32) -> Self {
        let start = r.start.saturating_sub(u64::from(padding)).max(1);
        Self {
            species,
            chromosome: r.chromosome.clone(),
            fmin: start as i64 - 1,
            fmax: r.end.saturating_add(u64::from(padding)).min(MAX_COORD) as i64,
        }
    }
    pub fn json(&self) -> Value {
        json!({"chromosome": self.chromosome, "start": self.fmin + 1, "end": self.fmax, "species": self.species})
    }
}

pub fn feature_row(r: &tokio_postgres::Row) -> Value {
    let id: String = r.get("id");
    let fmin: Option<i32> = r.get("fmin");
    json!({
        "id": id, "symbol": r.get::<_, Option<String>>("symbol"), "type": r.get::<_, String>("type"),
        "start": fmin.map(|x| x + 1), "end": r.get::<_, Option<i32>>("fmax"), "strand": r.get::<_, Option<i16>>("strand").unwrap_or(0),
        "overlap": r.get::<_, String>("overlap"), "link": if id.starts_with("FB") { Some(report_link(&id)) } else { None },
    })
}

/// Overlapping features of the given SO types in a range; shared with cytosearch.
pub async fn overlaps(
    tx: &Transaction<'_>,
    tool: &'static str,
    range: &Range,
    so_types: &[String],
    fetch: i64,
    offset: i64,
) -> ToolResult<Vec<tokio_postgres::Row>> {
    Chado::query_tx(
        tx,
        tool,
        "map/overlaps",
        &[
            &range.species,
            &range.chromosome,
            &(range.fmin as i32),
            &(range.fmax as i32),
            &so_types,
            &fetch,
            &offset,
        ],
    )
    .await
}

#[tool_router(router = tool_router_map, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "map_features",
        description = "List genome features (genes, transcripts, exons, insertions, regulatory regions, reagents, ...) overlapping a genomic range or the span of a located FlyBase feature, with coordinates, strand, and overlap kind. Use when you need what lies in a region; use cytosearch for cytological bands and get_sequence for the bases."
    )]
    pub async fn map_features(
        &self,
        Parameters(p): Parameters<MapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "map_features";
        let started = std::time::Instant::now();
        let res = async {
            if p.list_options.unwrap_or(false) {
                return Ok(Envelope::new(T, &self.release)
                    .summary("Feature types you can pass in `feature_types`.")
                    .field("feature_types", FeatureType::catalog())
                    .finish());
            }
            let _permit = self.limits.heavy(T).await?;
            self.map_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn map_impl(&self, p: MapParams) -> ToolResult<CallToolResult> {
        const T: &str = "map_features";
        let padding = p.padding.unwrap_or(0);
        if padding > MAX_PADDING {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "padding",
                message: format!("max is {MAX_PADDING}"),
            });
        }
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!(
                    "offset + limit may not exceed {MAX_OFFSET}; narrow the range instead"
                ),
            });
        }
        let types = p
            .feature_types
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| vec![FeatureType::Gene, FeatureType::Mrna, FeatureType::Insertion]);
        let so_types = FeatureType::so_names(&types);
        // Species and the feature's own location are looked up on their own connections before the heavy one is taken.
        let range = match (p.region, p.id) {
            (None, None) | (Some(_), Some(_)) => {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "region",
                    message: "provide exactly one of region or id".into(),
                });
            }
            (Some(r), None) => {
                if r.end < r.start || r.start == 0 || r.end > MAX_COORD {
                    return Err(ToolError::InvalidInput {
                        tool: T,
                        arg: "region",
                        message: format!(
                            "end ({}) must be >= start ({}), start must be >= 1, and end at most {MAX_COORD}",
                            r.end, r.start
                        ),
                    });
                }
                let species = match &p.species {
                    Some(s) => normalize_species(&self.chado, T, s).await?,
                    None => "Dmel".to_string(),
                };
                Range::from_region(species, &r, padding)
            }
            (None, Some(id)) => {
                let id = normalize_fb_prefix(id.trim()).into_owned();
                if !looks_like_fb_id(&id) {
                    return Err(ToolError::InvalidInput {
                        tool: T,
                        arg: "id",
                        message: format!("'{id}' is not a FlyBase ID; use resolve first"),
                    });
                }
                let rows = self
                    .chado
                    .query(T, "sequence/gene_location", &[&id])
                    .await?;
                let r = rows.first().ok_or_else(|| ToolError::InvalidInput {
                    tool: T,
                    arg: "id",
                    message: format!(
                        "{id} is not a located feature (genes, transcripts, insertions, and sequence features are)"
                    ),
                })?;
                let (fmin, fmax): (Option<i32>, Option<i32>) = (r.get("fmin"), r.get("fmax"));
                let (Some(fmin), Some(fmax)) = (fmin, fmax) else {
                    return Err(ToolError::InvalidInput {
                        tool: T,
                        arg: "id",
                        message: format!("{id} has no coordinates"),
                    });
                };
                let region = Region {
                    chromosome: r.get::<_, Option<String>>("chromosome").unwrap_or_default(),
                    start: fmin as u64 + 1,
                    end: fmax as u64,
                    strand: None,
                };
                Range::from_region(
                    r.get::<_, Option<String>>("species")
                        .unwrap_or_else(|| "Dmel".into()),
                    &region,
                    padding,
                )
            }
        };
        let span = range.fmax - range.fmin;
        if span > MAX_REGION as i64 {
            return Err(ToolError::LimitExceeded {
                tool: T,
                message: format!("range is {span} bp after padding; max is {MAX_REGION} bp"),
            });
        }
        let fetch = limit + 1;
        let rows = self
            .chado
            .heavy(T, async |tx| {
                overlaps(tx, T, &range, &so_types, fetch, offset).await
            })
            .await?;
        let has_more = rows.len() as i64 > limit;
        let features: Vec<Value> = rows.iter().take(limit as usize).map(feature_row).collect();
        let summary = format!(
            "{} feature(s) of type {} in {}:{}..{} ({}){}.",
            features.len(),
            types
                .iter()
                .map(|t| t.name())
                .collect::<Vec<_>>()
                .join(", "),
            range.chromosome,
            range.fmin + 1,
            range.fmax,
            range.species,
            if has_more {
                "; more available with offset"
            } else {
                ""
            }
        );
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .link(
                "jbrowse",
                jbrowse_link(&range.chromosome, range.fmin, range.fmax),
            )
            .field("region", range.json())
            .field("features", json!(features))
            .field("has_more", json!(has_more))
            .field("offset", json!(offset))
            .finish())
    }
}
