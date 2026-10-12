//! blast_submit, blast_status, blast_results: a thin front on the private BLAST job API. None of the three
//! is cached (job state changes between calls) and none takes a chado permit.
use crate::{
    backends::blast::{BLAST_SPECIES, BlastClient, MAX_SEQUENCE, SubmitRequest, parse_results},
    error::{ToolError, ToolResult},
    output::{Envelope, complete, report_link},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub const PUBLIC_BLAST_URL: &str = "https://flybase.org/blast/";
pub const MATRICES: &[&str] = &["BLOSUM62", "BLOSUM45", "BLOSUM80", "PAM30", "PAM70"];
pub const DEFAULT_POLL_SECONDS: u32 = 10;
pub const MAX_HITS: u32 = 100;
pub const MAX_HSPS: u32 = 20;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Program {
    Blastn,
    Blastp,
    Blastx,
    Tblastn,
    Tblastx,
}

impl Program {
    pub const ALL: [Program; 5] = [
        Self::Blastn,
        Self::Blastp,
        Self::Blastx,
        Self::Tblastn,
        Self::Tblastx,
    ];
    fn as_str(self) -> &'static str {
        match self {
            Self::Blastn => "blastn",
            Self::Blastp => "blastp",
            Self::Blastx => "blastx",
            Self::Tblastn => "tblastn",
            Self::Tblastx => "tblastx",
        }
    }
    /// blastp and blastx search protein databases; the others search nucleotide databases.
    fn searches_protein_db(self) -> bool {
        matches!(self, Self::Blastp | Self::Blastx)
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Database {
    Assembly,
    Transcript,
    Translation,
    Intergenic,
    SyntenicRegion,
    Transposon,
    CanonicalTransposons,
    Genbank,
    Est,
    Gss,
    Genpept,
}

impl Database {
    pub const ALL: [Database; 11] = [
        Self::Assembly,
        Self::Transcript,
        Self::Translation,
        Self::Intergenic,
        Self::SyntenicRegion,
        Self::Transposon,
        Self::CanonicalTransposons,
        Self::Genbank,
        Self::Est,
        Self::Gss,
        Self::Genpept,
    ];
    fn as_str(self) -> &'static str {
        match self {
            Self::Assembly => "assembly",
            Self::Transcript => "transcript",
            Self::Translation => "translation",
            Self::Intergenic => "intergenic",
            Self::SyntenicRegion => "syntenic_region",
            Self::Transposon => "transposon",
            Self::CanonicalTransposons => "canonical_transposons",
            Self::Genbank => "genbank",
            Self::Est => "est",
            Self::Gss => "gss",
            Self::Genpept => "genpept",
        }
    }
    fn is_protein(self) -> bool {
        matches!(self, Self::Translation | Self::Genpept)
    }
    fn describe(self) -> &'static str {
        match self {
            Self::Assembly => "genome assembly",
            Self::Transcript => "annotated transcripts",
            Self::Translation => "annotated polypeptides",
            Self::Intergenic => "intergenic regions",
            Self::SyntenicRegion => "syntenic regions",
            Self::Transposon => "transposable element sequences",
            Self::CanonicalTransposons => "canonical natural transposons",
            Self::Genbank => "GenBank nucleotide records",
            Self::Est => "ESTs",
            Self::Gss => "genome survey sequences",
            Self::Genpept => "GenPept protein records",
        }
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct BlastSubmitParams {
    /// blastn, blastp, blastx, tblastn, or tblastx.
    pub program: Option<Program>,
    /// Target database; translation and genpept are protein, the rest nucleotide. list_options=true describes them.
    pub database: Option<Database>,
    /// Species to search, FlyBase abbreviations (default ["Dmel"]).
    pub species: Option<Vec<String>>,
    /// Query sequence, raw or FASTA (one record), up to 100,000 residues.
    pub sequence: Option<String>,
    /// Expectation value cutoff (service default when omitted).
    pub evalue: Option<f64>,
    /// blastn only: use megablast.
    pub megablast: Option<bool>,
    /// Maximum hits the service keeps (1 to 1000; service default when omitted).
    pub max_hits: Option<u32>,
    /// Protein scoring matrix: BLOSUM62, BLOSUM45, BLOSUM80, PAM30, PAM70.
    pub matrix: Option<String>,
    /// NCBI genetic code for translated searches (1 to 33).
    pub genetic_code: Option<u8>,
    /// Extra BLAST+ options, letters, digits, spaces, "-", "_", "." and "=" only (for example "-word_size 11").
    pub advanced: Option<String>,
    /// If true, return the programs, databases, species, and matrices and nothing else.
    pub list_options: Option<bool>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlastJobParams {
    /// The job_id from blast_submit.
    pub job_id: String,
    /// The job_token from blast_submit.
    pub job_token: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlastResultsParams {
    /// The job_id from blast_submit.
    pub job_id: String,
    /// The job_token from blast_submit.
    pub job_token: String,
    /// Hits to return (default 25, max 100).
    pub max_hits: Option<u32>,
    /// HSPs per hit (default 3, max 20).
    pub max_hsps_per_hit: Option<u32>,
}

fn unconfigured(tool: &'static str) -> ToolError {
    ToolError::BackendUnavailable {
        tool,
        message: format!("BLAST is not configured on this server; use {PUBLIC_BLAST_URL}"),
    }
}

/// Strip FASTA headers and whitespace, uppercase, and validate residues and length.
fn clean_sequence(raw: &str, tool: &'static str) -> ToolResult<String> {
    if raw
        .lines()
        .filter(|l| l.trim_start().starts_with('>'))
        .count()
        > 1
    {
        return Err(ToolError::InvalidInput {
            tool,
            arg: "sequence",
            message: "one sequence per job; the input has more than one FASTA record".into(),
        });
    }
    let seq: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with('>'))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if seq.is_empty() {
        return Err(ToolError::InvalidInput {
            tool,
            arg: "sequence",
            message: "no residues found; give raw sequence or one FASTA record".into(),
        });
    }
    if seq.len() > MAX_SEQUENCE {
        return Err(ToolError::LimitExceeded {
            tool,
            message: format!("{MAX_SEQUENCE} residues max; got {}", seq.len()),
        });
    }
    if let Some(bad) = seq
        .chars()
        .find(|c| !(c.is_ascii_alphabetic() || *c == '*' || *c == '-'))
    {
        return Err(ToolError::InvalidInput {
            tool,
            arg: "sequence",
            message: format!("unexpected character '{bad}' in the sequence"),
        });
    }
    Ok(seq)
}

/// Check the job ID shape and the caller's token locally; a mismatch never reaches the BLAST service.
fn job_access(
    caches: &Arc<crate::backends::cache::Caches>,
    tool: &'static str,
    job_id: &str,
    token: &str,
) -> ToolResult<()> {
    if !crate::backends::blast::valid_job_id(job_id) {
        return Err(ToolError::InvalidInput {
            tool,
            arg: "job_id",
            message: "job IDs are 8 to 64 letters, digits, '-' or '_'".into(),
        });
    }
    if caches.blast_tokens.check(job_id, token) {
        Ok(())
    } else {
        Err(ToolError::NotFound {
            tool,
            message: format!(
                "job {job_id} is unknown or the job_token does not match; jobs expire 24 h after submission"
            ),
        })
    }
}

fn status_json(s: &crate::backends::blast::JobStatus) -> Value {
    json!({
        "job_id": s.job_id,
        "state": s.state,
        "queue_position": s.queue_position,
        "error": s.error,
        "result_url": s.result_url,
        "downloads": s.downloads,
    })
}

#[tool_router(router = tool_router_blast, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "blast_submit",
        description = "Submit a BLAST search of a nucleotide or protein sequence against FlyBase databases (genome assembly, transcripts, polypeptides, transposons, and more) for one or more Drosophila species. Returns a job_id and job_token; poll blast_status, then fetch blast_results. Use list_options=true to see programs, databases, and species."
    )]
    pub async fn blast_submit(
        &self,
        Parameters(p): Parameters<BlastSubmitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "blast_submit";
        let started = std::time::Instant::now();
        let res = self.blast_submit_impl(p).await;
        Ok(complete(T, started, res))
    }

    async fn blast_submit_impl(&self, p: BlastSubmitParams) -> ToolResult<CallToolResult> {
        const T: &str = "blast_submit";
        if p.list_options.unwrap_or(false) {
            return Ok(Envelope::new(T, &self.release)
                .summary(format!(
                    "BLAST options: {} programs, {} databases, {} species. {}",
                    Program::ALL.len(),
                    Database::ALL.len(),
                    BLAST_SPECIES.len(),
                    if self.blast.is_some() {
                        "BLAST is configured."
                    } else {
                        "BLAST is not configured on this server."
                    }
                ))
                .field("configured", json!(self.blast.is_some()))
                .field(
                    "programs",
                    json!(Program::ALL.iter().map(|x| x.as_str()).collect::<Vec<_>>()),
                )
                .field(
                    "databases",
                    json!(
                        Database::ALL
                            .iter()
                            .map(|d| json!({"name": d.as_str(), "protein": d.is_protein(), "description": d.describe()}))
                            .collect::<Vec<_>>()
                    ),
                )
                .field("species", json!(BLAST_SPECIES))
                .field("matrices", json!(MATRICES))
                .link("public_blast", PUBLIC_BLAST_URL.into())
                .finish());
        }
        let client: Arc<BlastClient> = self.blast.clone().ok_or_else(|| unconfigured(T))?;
        let program = p.program.ok_or_else(|| ToolError::InvalidInput {
            tool: T,
            arg: "program",
            message: "required: blastn, blastp, blastx, tblastn, or tblastx".into(),
        })?;
        let database = p.database.ok_or_else(|| ToolError::InvalidInput {
            tool: T,
            arg: "database",
            message: "required; call with list_options=true".into(),
        })?;
        if program.searches_protein_db() != database.is_protein() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "database",
                message: format!(
                    "{} searches a {} database but {} is {}",
                    program.as_str(),
                    if program.searches_protein_db() {
                        "protein"
                    } else {
                        "nucleotide"
                    },
                    database.as_str(),
                    if database.is_protein() {
                        "protein"
                    } else {
                        "nucleotide"
                    }
                ),
            });
        }
        let sequence = clean_sequence(p.sequence.as_deref().unwrap_or(""), T)?;
        let species_in = p.species.unwrap_or_else(|| vec!["Dmel".into()]);
        if species_in.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "species",
                message: format!(
                    "give at least one species, or omit it for Dmel; choose from {}",
                    BLAST_SPECIES.join(", ")
                ),
            });
        }
        let mut species = Vec::new();
        for s in &species_in {
            let canon = BLAST_SPECIES
                .iter()
                .find(|x| x.eq_ignore_ascii_case(s.trim()))
                .ok_or_else(|| ToolError::InvalidInput {
                    tool: T,
                    arg: "species",
                    message: format!(
                        "species has no BLAST database; choose from {}",
                        BLAST_SPECIES.join(", ")
                    ),
                })?;
            let code = canon.to_ascii_lowercase();
            if !species.contains(&code) {
                species.push(code);
            }
        }
        if let Some(e) = p.evalue
            && !(e > 0.0 && e <= 1000.0)
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "evalue",
                message: "must be above 0 and at most 1000".into(),
            });
        }
        if p.megablast == Some(true) && program != Program::Blastn {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "megablast",
                message: "megablast applies to blastn only".into(),
            });
        }
        if p.matrix.is_some() && program == Program::Blastn {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "matrix",
                message:
                    "scoring matrices apply to blastp, blastx, tblastn, and tblastx, not blastn"
                        .into(),
            });
        }
        if let Some(n) = p.max_hits
            && !(1..=1000).contains(&n)
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "max_hits",
                message: "must be 1 to 1000".into(),
            });
        }
        let matrix = match p.matrix.as_deref().map(str::trim) {
            None => None,
            Some(m) => Some(
                MATRICES
                    .iter()
                    .find(|x| x.eq_ignore_ascii_case(m))
                    .map(|x| x.to_string())
                    .ok_or_else(|| ToolError::InvalidInput {
                        tool: T,
                        arg: "matrix",
                        message: format!("choose from {}", MATRICES.join(", ")),
                    })?,
            ),
        };
        if let Some(g) = p.genetic_code
            && !(1..=33).contains(&g)
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "genetic_code",
                message: "must be 1 to 33".into(),
            });
        }
        let advanced = match p
            .advanced
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
        {
            None => None,
            Some(a)
                if a.len() <= 200
                    && a.chars()
                        .all(|c| c.is_ascii_alphanumeric() || " -_.=".contains(c)) =>
            {
                Some(a.to_string())
            }
            Some(_) => {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "advanced",
                    message:
                        "only letters, digits, spaces, '-', '_', '.', '=' and at most 200 characters"
                            .into(),
                });
            }
        };
        let req = SubmitRequest {
            program: program.as_str().into(),
            database: database.as_str().into(),
            species: species.clone(),
            sequence: format!(">query\n{sequence}\n"),
            evalue: p.evalue,
            megablast: p.megablast,
            max_hits: p.max_hits,
            matrix,
            genetic_code: p.genetic_code,
            advanced,
            client: "flybase-mcp",
        };
        // The submit permit covers only the HTTP round trip, never the validation above.
        let resp = {
            let _permit = self.limits.blast(T).await?;
            client.submit(&req).await.map_err(|e| e.into_tool(T))?
        };
        let token = self.caches.blast_tokens.issue(&resp.job_id);
        let poll = resp.poll_after_seconds.unwrap_or(DEFAULT_POLL_SECONDS);
        let release_note = resp
            .database_release
            .clone()
            .unwrap_or_else(|| "the BLAST service's current database build".into());
        let note = format!(
            "Results come from {release_note}, which may lag the current FlyBase release. Keep job_token: blast_status and blast_results require it. Poll no sooner than {poll} s."
        );
        Ok(Envelope::new(T, &self.release)
            .summary(format!(
                "{} of {} residues against {} ({}) submitted as job {} ({}{}); poll blast_status after {poll} s.",
                program.as_str(),
                sequence.len(),
                database.as_str(),
                species.join(","),
                resp.job_id,
                resp.state,
                resp.queue_position
                    .map(|q| format!(", queue position {q}"))
                    .unwrap_or_default()
            ))
            .field("job_id", json!(resp.job_id))
            .field("job_token", json!(token))
            .field("state", json!(resp.state))
            .field("queue_position", json!(resp.queue_position))
            .field("poll_after_seconds", json!(poll))
            .field("database_release", json!(resp.database_release))
            .field("note", json!(note))
            .link("public_blast", PUBLIC_BLAST_URL.into())
            .finish())
    }

    #[tool(
        name = "blast_status",
        description = "Check a BLAST job submitted with blast_submit: queued, running, complete, failed, or cancelled. Use after the poll interval blast_submit gave; when complete, call blast_results."
    )]
    pub async fn blast_status(
        &self,
        Parameters(p): Parameters<BlastJobParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "blast_status";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.blast_poll(T).await?;
            self.blast_status_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn blast_status_impl(&self, p: BlastJobParams) -> ToolResult<CallToolResult> {
        const T: &str = "blast_status";
        let client = self.blast.clone().ok_or_else(|| unconfigured(T))?;
        job_access(&self.caches, T, &p.job_id, &p.job_token)?;
        let s = client.status(&p.job_id).await.map_err(|e| e.into_tool(T))?;
        let mut env = Envelope::new(T, &self.release);
        let summary = match s.state.as_str() {
            "complete" => format!("Job {} is complete; call blast_results.", s.job_id),
            "failed" => format!(
                "Job {} failed: {}.",
                s.job_id,
                s.error.as_deref().unwrap_or("no details")
            ),
            "cancelled" => format!("Job {} was cancelled.", s.job_id),
            other => format!(
                "Job {} is {other}{}; poll again in {DEFAULT_POLL_SECONDS} s.",
                s.job_id,
                s.queue_position
                    .filter(|q| *q > 0)
                    .map(|q| format!(" at queue position {q}"))
                    .unwrap_or_default()
            ),
        };
        if let Some(u) = &s.result_url {
            env = env.link("result_page", u.clone());
        }
        let body = status_json(&s);
        for (k, v) in body.as_object().into_iter().flatten() {
            env = env.field(k, v.clone());
        }
        Ok(env.summary(summary).finish())
    }

    #[tool(
        name = "blast_results",
        description = "Fetch the hits of a completed BLAST job: subject IDs with FlyBase IDs where present, e-values, bit scores, identities, and the best HSPs, plus download links for the full TSV, XML, and text reports. Use after blast_status reports complete."
    )]
    pub async fn blast_results(
        &self,
        Parameters(p): Parameters<BlastResultsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "blast_results";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.blast_poll(T).await?;
            self.blast_results_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn blast_results_impl(&self, p: BlastResultsParams) -> ToolResult<CallToolResult> {
        const T: &str = "blast_results";
        let client = self.blast.clone().ok_or_else(|| unconfigured(T))?;
        job_access(&self.caches, T, &p.job_id, &p.job_token)?;
        let max_hits = p.max_hits.unwrap_or(25).clamp(1, MAX_HITS) as usize;
        let max_hsps = p.max_hsps_per_hit.unwrap_or(3).clamp(1, MAX_HSPS) as usize;
        let status = client.status(&p.job_id).await.map_err(|e| e.into_tool(T))?;
        if status.state != "complete" {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "job_id",
                message: format!(
                    "the job is {}; call blast_status until it is complete",
                    status.state
                ),
            });
        }
        let tsv = client
            .results_tsv(&p.job_id)
            .await
            .map_err(|e| e.into_tool(T))?;
        let (hits, total) = parse_results(&tsv, max_hits, max_hsps);
        let hits_json: Vec<Value> = hits
            .iter()
            .map(|h| {
                let mut v = serde_json::to_value(h).unwrap_or_default();
                v["link"] = json!(h.flybase_id.as_deref().map(report_link));
                v
            })
            .collect();
        let summary = if hits.is_empty() {
            format!("Job {} finished with no hits.", p.job_id)
        } else {
            let top = &hits[0];
            format!(
                "{} of {total} hit(s) for job {}; best: {} ({}), e-value {:e}, bit score {}.",
                hits.len(),
                p.job_id,
                top.subject_id,
                top.flybase_id.as_deref().unwrap_or("no FlyBase ID"),
                top.evalue,
                top.bitscore
            )
        };
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .field("job_id", json!(p.job_id))
            .field("hits", json!(hits_json))
            .field("hits_shown", json!(hits.len()))
            .field("total_hits", json!(total))
            .field("downloads", json!(status.downloads));
        if let Some(u) = &status.result_url {
            env = env.link("result_page", u.clone());
        }
        if let Some(first) = hits.first().and_then(|h| h.flybase_id.as_deref()) {
            env = env.link("first_report", report_link(first));
        }
        Ok(env.finish())
    }
}
