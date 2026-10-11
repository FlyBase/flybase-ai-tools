use crate::{
    backends::{
        chado::Chado,
        seq::{fasta, reverse_complement, splice, to_chado},
        species::normalize_species,
    },
    error::{ToolError, ToolResult},
    limits::MAX_FASTA_BYTES,
    output::{Envelope, complete, report_link},
    server::FlyBaseServer,
    tools::resolve::class_of,
};
use deadpool_postgres::Object;
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

pub const MAX_REGION: u64 = 500_000;
/// Above this length the inline `sequence` field is omitted and only `fasta` is returned.
pub const INLINE_SEQ_MAX: usize = 100_000;

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SeqKind {
    Gene,
    Transcript,
    Cds,
    Protein,
    FivePrimeUtr,
    ThreePrimeUtr,
    Intron,
    Upstream,
    Downstream,
}

impl SeqKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gene => "gene",
            Self::Transcript => "transcript",
            Self::Cds => "cds",
            Self::Protein => "protein",
            Self::FivePrimeUtr => "five_prime_utr",
            Self::ThreePrimeUtr => "three_prime_utr",
            Self::Intron => "intron",
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
        }
    }
    fn part_type(self) -> &'static str {
        match self {
            Self::Cds => "CDS",
            Self::FivePrimeUtr => "five_prime_UTR",
            Self::ThreePrimeUtr => "three_prime_UTR",
            Self::Intron => "intron",
            _ => "",
        }
    }
}

#[derive(Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Chromosome arm name as in FlyBase (X, 2L, 2R, 3L, 3R, 4, Y).
    pub chromosome: String,
    /// 1-based inclusive start.
    pub start: u64,
    /// 1-based inclusive end.
    pub end: u64,
    /// 1 or -1 (default 1).
    pub strand: Option<i8>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceParams {
    /// FlyBase ID: FBgn, FBtr, FBpp, FBti, FBsf. Provide this or `region`.
    pub id: Option<String>,
    /// Genomic region on a chromosome arm, 1-based inclusive, max 500 kb.
    pub region: Option<Region>,
    /// For FBgn: which sequence to return (default gene = genomic span). Transcript-level kinds return one record per isoform.
    pub kind: Option<SeqKind>,
    /// Limit transcript-level kinds to one isoform.
    pub transcript_id: Option<String>,
    /// Extra bases on each side for gene and FBti, or the flank length for upstream and downstream (required for those, max 50000). FBti default 500.
    pub padding: Option<u32>,
    /// Species abbreviation for region requests (default Dmel).
    pub species: Option<String>,
}

#[tool_router(router = tool_router_sequence, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_sequence",
        description = "Return FASTA sequence for a FlyBase ID (gene span, transcripts, CDS, protein, UTRs, introns, insertion flanks) or for a genomic region (chromosome, start, end, strand). Max 500 kb per region. Use resolve first if you only have a symbol."
    )]
    pub async fn get_sequence(
        &self,
        Parameters(p): Parameters<SequenceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_sequence";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.sequence_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    /// Every query for one call runs on the single connection checked out here (see `Chado`).
    async fn sequence_impl(&self, mut p: SequenceParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_sequence";
        match (p.id.take(), p.region.take()) {
            (None, None) | (Some(_), Some(_)) => Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: "provide exactly one of id or region".into(),
            }),
            (None, Some(r)) => {
                // normalize_species checks out and returns its own connection before ours is taken.
                let species = match &p.species {
                    Some(s) => normalize_species(&self.chado, T, s).await?,
                    None => "Dmel".to_string(),
                };
                let conn = self.chado.get(T).await?;
                self.region_seq(&conn, T, r, &species, None).await
            }
            (Some(id), None) => {
                let conn = self.chado.get(T).await?;
                self.id_seq(&conn, T, &id, p).await
            }
        }
    }

    /// Fetch a region. `id` is set when the region was derived from a feature, for the header and links.
    async fn region_seq(
        &self,
        conn: &Object,
        t: &'static str,
        r: Region,
        species: &str,
        id: Option<&str>,
    ) -> ToolResult<CallToolResult> {
        if r.end < r.start || r.start == 0 {
            return Err(ToolError::InvalidInput {
                tool: t,
                arg: "region",
                message: format!(
                    "end ({}) must be >= start ({}) and start must be >= 1",
                    r.end, r.start
                ),
            });
        }
        let span = r.end - r.start + 1;
        if span > MAX_REGION {
            return Err(ToolError::LimitExceeded {
                tool: t,
                message: format!("region is {span} bp; max is {MAX_REGION} bp"),
            });
        }
        let strand = r.strand.unwrap_or(1);
        if strand != 1 && strand != -1 {
            return Err(ToolError::InvalidInput {
                tool: t,
                arg: "region.strand",
                message: "must be 1 or -1".into(),
            });
        }
        let (fmin, _) = to_chado(r.start, r.end);
        let rows = Chado::query_on(
            conn,
            t,
            "sequence/region",
            &[&species, &r.chromosome, &(fmin as i32), &(span as i32)],
        )
        .await?;
        let row = rows.first().ok_or_else(|| ToolError::NotFound {
            tool: t,
            message: format!("no chromosome arm '{}' for {species}", r.chromosome),
        })?;
        // seqlen and residues are nullable in chado (a few unsequenced golden_path scaffolds have neither).
        let seqlen: Option<i32> = row.get("seqlen");
        let seq: Option<String> = row.get("seq");
        let mut seq = seq.ok_or_else(|| ToolError::NotFound {
            tool: t,
            message: format!("{} has no stored sequence", r.chromosome),
        })?;
        let known_end = seqlen
            .map(|n| n as u64)
            .unwrap_or(r.start - 1 + seq.len() as u64);
        if r.end > known_end {
            return Err(ToolError::InvalidInput {
                tool: t,
                arg: "region.end",
                message: format!(
                    "{} is beyond the end of {} ({known_end} bp)",
                    r.end, r.chromosome
                ),
            });
        }
        if strand < 0 {
            seq = reverse_complement(&seq);
        }
        let coords = format!(
            "{}:{}..{}({})",
            r.chromosome,
            r.start,
            r.end,
            if strand < 0 { "-" } else { "+" }
        );
        let header = match id {
            Some(id) => format!("{id} {coords} {species} {}", self.release),
            None => format!("{coords} {species} {}", self.release),
        };
        let mut env = Envelope::new(t, &self.release)
            .summary(format!("{} bp from {coords} ({species}).", seq.len()))
            .field("coordinates", json!(coords))
            .field("species", json!(species))
            .field("length", json!(seq.len()))
            .field("fasta", json!(fasta(&header, &seq)));
        if seq.len() <= INLINE_SEQ_MAX {
            env = env.field("sequence", json!(seq));
        }
        if let Some(id) = id {
            env = env.field("id", json!(id)).link("report", report_link(id));
        }
        Ok(env.finish())
    }

    async fn id_seq(
        &self,
        conn: &Object,
        t: &'static str,
        id: &str,
        p: SequenceParams,
    ) -> ToolResult<CallToolResult> {
        let class = class_of(id);
        let kind = p.kind.unwrap_or(SeqKind::Gene);
        match class {
            "transcript" | "polypeptide" => {
                let rows = Chado::query_on(conn, t, "sequence/feature_residues", &[&id]).await?;
                let row = rows.first().ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{id} not found"),
                })?;
                let seq: Option<String> = row.get("residues");
                let seq = seq.ok_or_else(|| ToolError::NotFound {
                    tool: t,
                    message: format!("{id} has no stored sequence"),
                })?;
                let name: Option<String> = row.get("name");
                let species: Option<String> = row.get("species");
                let species = species.unwrap_or_default();
                let header = format!(
                    "{id} {} {species} {}",
                    name.clone().unwrap_or_default(),
                    self.release
                );
                let mut env = Envelope::new(t, &self.release)
                    .summary(format!(
                        "{} residues for {id} ({}).",
                        seq.len(),
                        name.unwrap_or_default()
                    ))
                    .link("report", report_link(id))
                    .field("id", json!(id))
                    .field("species", json!(species))
                    .field("length", json!(seq.len()))
                    .field("fasta", json!(fasta(&header, &seq)));
                if seq.len() <= INLINE_SEQ_MAX {
                    env = env.field("sequence", json!(seq));
                }
                Ok(env.finish())
            }
            "transposable_element" => Err(ToolError::InvalidInput {
                tool: t,
                arg: "id",
                message: "natural transposable elements (FBte) have no sequence in chado; BLAST against the canonical_transposons database instead".into(),
            }),
            "insertion" | "sequence_feature" => {
                self.span_seq(conn, t, id, class, kind, p.padding).await
            }
            "gene" if matches!(kind, SeqKind::Gene | SeqKind::Upstream | SeqKind::Downstream) => {
                self.span_seq(conn, t, id, class, kind, p.padding).await
            }
            "gene" => {
                self.gene_kind_seq(conn, t, id, kind, p.transcript_id.as_deref())
                    .await
            }
            _ => Err(ToolError::InvalidInput {
                tool: t,
                arg: "id",
                message: format!(
                    "{id} ({class}) is not a sequence-bearing ID; use FBgn, FBtr, FBpp, FBti, or FBsf"
                ),
            }),
        }
    }

    async fn span_seq(
        &self,
        conn: &Object,
        t: &'static str,
        id: &str,
        class: &str,
        kind: SeqKind,
        padding: Option<u32>,
    ) -> ToolResult<CallToolResult> {
        let rows = Chado::query_on(conn, t, "sequence/gene_location", &[&id]).await?;
        let row = rows.first().ok_or_else(|| ToolError::NotFound {
            tool: t,
            message: format!("{id} is not located on a chromosome arm"),
        })?;
        // fmin and fmax are nullable; a feature placed on an arm without coordinates has no span.
        let (fmin, fmax) = match (
            row.get::<_, Option<i32>>("fmin"),
            row.get::<_, Option<i32>>("fmax"),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                return Err(ToolError::NotFound {
                    tool: t,
                    message: format!("{id} has no coordinates on its chromosome arm"),
                });
            }
        };
        let strand = row.get::<_, Option<i16>>("strand").unwrap_or(1) as i8;
        let pad = match (kind, padding) {
            (SeqKind::Upstream | SeqKind::Downstream, None) => {
                return Err(ToolError::InvalidInput {
                    tool: t,
                    arg: "padding",
                    message:
                        "upstream and downstream need padding (flank length in bp, 1 to 50000)"
                            .into(),
                });
            }
            (_, Some(p)) if p == 0 || p > 50_000 => {
                return Err(ToolError::InvalidInput {
                    tool: t,
                    arg: "padding",
                    message: "must be 1 to 50000".into(),
                });
            }
            (_, Some(p)) => p as i32,
            (_, None) if class == "insertion" => 500,
            (_, None) => 0,
        };
        let (s0, e0) = match kind {
            SeqKind::Upstream => {
                if strand > 0 {
                    (fmin - pad, fmin)
                } else {
                    (fmax, fmax + pad)
                }
            }
            SeqKind::Downstream => {
                if strand > 0 {
                    (fmax, fmax + pad)
                } else {
                    (fmin - pad, fmin)
                }
            }
            _ => (fmin - pad, fmax + pad),
        };
        let region = Region {
            chromosome: row.get("chromosome"),
            start: (s0.max(0) + 1) as u64,
            end: e0.max(1) as u64,
            strand: Some(strand),
        };
        let species: Option<String> = row.get("species");
        self.region_seq(conn, t, region, &species.unwrap_or_default(), Some(id))
            .await
    }

    async fn gene_kind_seq(
        &self,
        conn: &Object,
        t: &'static str,
        id: &str,
        kind: SeqKind,
        transcript_id: Option<&str>,
    ) -> ToolResult<CallToolResult> {
        let trs = Chado::query_on(conn, t, "sequence/gene_transcripts", &[&id]).await?;
        if trs.is_empty() {
            return Err(ToolError::NotFound {
                tool: t,
                message: format!("{id} has no transcripts"),
            });
        }
        let loc = Chado::query_on(conn, t, "sequence/gene_location", &[&id]).await?;
        let (chromosome, species): (Option<String>, Option<String>) = match loc.first() {
            Some(r) => (Some(r.get("chromosome")), r.get("species")),
            None => (None, None),
        };
        let mut records: Vec<Value> = Vec::new();
        let mut total = 0usize;
        for tr in trs
            .iter()
            .filter(|r| transcript_id.is_none_or(|want| r.get::<_, String>("uniquename") == want))
        {
            let tid: String = tr.get("uniquename");
            let tname: Option<String> = tr.get("name");
            let seq: Option<String> = match kind {
                SeqKind::Transcript => tr.get("residues"),
                SeqKind::Protein => {
                    let parts =
                        Chado::query_on(conn, t, "sequence/transcript_parts", &[&tid]).await?;
                    parts
                        .iter()
                        .find(|r| r.get::<_, String>("type") == "polypeptide")
                        .and_then(|r| r.get::<_, Option<String>>("residues"))
                }
                SeqKind::Cds | SeqKind::FivePrimeUtr | SeqKind::ThreePrimeUtr | SeqKind::Intron => {
                    let parts =
                        Chado::query_on(conn, t, "sequence/transcript_parts", &[&tid]).await?;
                    let locs: Vec<(i64, i64)> = parts
                        .iter()
                        .filter(|r| r.get::<_, String>("type") == kind.part_type())
                        .filter_map(|r| {
                            Some((
                                i64::from(r.get::<_, Option<i32>>("fmin")?),
                                i64::from(r.get::<_, Option<i32>>("fmax")?),
                            ))
                        })
                        .collect();
                    match (&chromosome, &species, locs.is_empty()) {
                        (Some(chrom), Some(sp), false) => {
                            let gmin = locs.iter().map(|l| l.0).min().unwrap_or(0);
                            let gmax = locs.iter().map(|l| l.1).max().unwrap_or(0);
                            let strand = tr.get::<_, Option<i16>>("strand").unwrap_or(1) as i8;
                            let g = Chado::query_on(
                                conn,
                                t,
                                "sequence/region",
                                &[sp, chrom, &(gmin as i32), &((gmax - gmin) as i32)],
                            )
                            .await?;
                            let genomic: String =
                                g.first().map(|r| r.get("seq")).unwrap_or_default();
                            Some(splice(&locs, &genomic, gmin, strand))
                        }
                        _ => None,
                    }
                }
                SeqKind::Gene | SeqKind::Upstream | SeqKind::Downstream => None,
            };
            if let Some(seq) = seq {
                total += seq.len();
                if total > MAX_FASTA_BYTES {
                    return Err(ToolError::LimitExceeded {
                        tool: t,
                        message:
                            "combined sequence exceeds 512 KB; pass transcript_id to select one isoform"
                                .into(),
                    });
                }
                let header = format!(
                    "{tid} {} {id} {} {}",
                    tname.clone().unwrap_or_default(),
                    kind.as_str(),
                    self.release
                );
                let mut rec = json!({"transcript_id": tid, "name": tname, "kind": kind.as_str(), "length": seq.len(), "fasta": fasta(&header, &seq)});
                if seq.len() <= INLINE_SEQ_MAX {
                    rec["sequence"] = json!(seq);
                }
                records.push(rec);
            }
        }
        if records.is_empty() {
            return Err(ToolError::NotFound {
                tool: t,
                message: format!("no {} sequence available for {id}", kind.as_str()),
            });
        }
        Ok(Envelope::new(t, &self.release)
            .summary(format!(
                "{} {} record(s) for {id}.",
                records.len(),
                kind.as_str()
            ))
            .link("report", report_link(id))
            .field("id", json!(id))
            .field("records", json!(records))
            .finish())
    }
}
