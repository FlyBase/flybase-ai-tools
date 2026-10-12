//! Client for the private BLAST job API (spec 5.5) and the job-token table that makes ownership stateless.
use crate::error::ToolError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

pub const MAX_SEQUENCE: usize = 100_000;
/// Largest results.tsv body the client reads (16 MiB); longer reports are available through `downloads`.
pub const MAX_RESULTS_BYTES: usize = 16 * 1024 * 1024;
/// Species with BLAST databases, as FlyBase abbreviations; the service takes them lowercased.
pub const BLAST_SPECIES: &[&str] = &[
    "Dmel", "Dsim", "Dsec", "Dyak", "Dere", "Dana", "Dpse", "Dper", "Dwil", "Dmoj", "Dvir", "Dgri",
];
/// The service keeps jobs for 24 h; tokens live exactly as long.
pub const TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const TOKEN_CAPACITY: u64 = 10_000;

#[derive(Serialize, Debug, Clone)]
pub struct SubmitRequest {
    pub program: String,
    pub database: String,
    pub species: Vec<String>,
    pub sequence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evalue: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub megablast: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hits: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matrix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genetic_code: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advanced: Option<String>,
    pub client: &'static str,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SubmitResponse {
    pub job_id: String,
    pub state: String,
    pub queue_position: Option<u32>,
    pub poll_after_seconds: Option<u32>,
    pub database_release: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JobStatus {
    pub job_id: String,
    pub state: String,
    pub queue_position: Option<u32>,
    pub error: Option<String>,
    pub result_url: Option<String>,
    pub downloads: Option<HashMap<String, String>>,
}

/// Prefix every link the service hands back must carry; anything else is dropped before it reaches a client.
const PUBLIC_LINK_PREFIX: &str = "https://flybase.org/";

/// Null `result_url`, and `downloads` as a whole, when any URL is not a public flybase.org page.
fn public_links_only(s: &mut JobStatus) {
    if s.result_url
        .as_deref()
        .is_some_and(|u| !u.starts_with(PUBLIC_LINK_PREFIX))
    {
        tracing::warn!(job_id = %s.job_id, field = "result_url", "BLAST service returned a non-flybase.org link; dropped");
        s.result_url = None;
    }
    if s.downloads
        .as_ref()
        .is_some_and(|d| d.values().any(|u| !u.starts_with(PUBLIC_LINK_PREFIX)))
    {
        tracing::warn!(job_id = %s.job_id, field = "downloads", "BLAST service returned a non-flybase.org link; dropped");
        s.downloads = None;
    }
}

#[derive(Deserialize, Debug, Default)]
struct ErrorBody {
    error: Option<String>,
    message: Option<String>,
    state: Option<String>,
    retry_after_seconds: Option<u32>,
}

#[derive(Debug)]
pub enum BlastError {
    /// The service rejected the request (400): the caller's input is wrong. `code` is the service's error code.
    Rejected {
        code: String,
        message: String,
    },
    QueueFull {
        retry_after: u32,
    },
    NotFound,
    NotReady(String),
    Unavailable(String),
}

impl BlastError {
    pub fn into_tool(self, tool: &'static str) -> ToolError {
        match self {
            Self::Rejected { code, message } => {
                let arg = match code.as_str() {
                    "invalid_program" => "program",
                    "invalid_database" => "database",
                    "invalid_species" => "species",
                    "invalid_evalue" => "evalue",
                    "invalid_max_hits" => "max_hits",
                    "invalid_matrix" => "matrix",
                    "invalid_megablast" => "megablast",
                    "invalid_genetic_code" => "genetic_code",
                    "invalid_option" => "advanced",
                    _ => "sequence",
                };
                ToolError::InvalidInput {
                    tool,
                    arg,
                    message: format!("the BLAST service rejected the request: {message}"),
                }
            }
            Self::QueueFull { retry_after } => ToolError::BackendUnavailable {
                tool,
                message: format!("the BLAST queue is full; try again in {retry_after} s"),
            },
            Self::NotFound => ToolError::NotFound {
                tool,
                message: "unknown job; jobs are kept for 24 h".into(),
            },
            Self::NotReady(state) => ToolError::InvalidInput {
                tool,
                arg: "job_id",
                message: format!("the job is {state}; call blast_status until it is complete"),
            },
            Self::Unavailable(m) => {
                tracing::error!(tool, error = %m, "BLAST service unavailable");
                ToolError::BackendUnavailable {
                    tool,
                    message: "the BLAST service is not reachable; try again in a few minutes"
                        .into(),
                }
            }
        }
    }
}

/// Job IDs the service may hand out: 8 to 64 URL-safe characters.
pub fn valid_job_id(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub struct BlastClient {
    http: reqwest::Client,
    base: Url,
    token: Option<String>,
}

impl BlastClient {
    /// `base` is the service root; a missing trailing slash is added so `Url::join` keeps its path.
    /// Only `http://` is accepted: this build compiles no TLS backend, so an `https://` base would fail
    /// every request; failing here surfaces the mistake once at startup.
    pub fn new(mut base: Url, token: Option<String>) -> anyhow::Result<Self> {
        if base.scheme() != "http" {
            anyhow::bail!("BLAST_BASE_URL must be http://; this build has no TLS backend");
        }
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(3))
            // The private API never redirects; following one could fetch an arbitrary reachable URL
            // and return its body as hits.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { http, base, token })
    }

    fn url(&self, path: &str) -> Result<Url, BlastError> {
        self.base
            .join(path)
            .map_err(|e| BlastError::Unavailable(format!("bad BLAST url: {e}")))
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }

    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response, BlastError> {
        let resp = rb
            .send()
            .await
            .map_err(|e| BlastError::Unavailable(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body: ErrorBody = resp.json().await.unwrap_or_default();
        Err(match status.as_u16() {
            400 | 422 => BlastError::Rejected {
                code: body.error.unwrap_or_default(),
                message: body.message.unwrap_or_else(|| "invalid request".into()),
            },
            401 | 403 => BlastError::Unavailable("service token rejected".into()),
            404 => BlastError::NotFound,
            300..=399 => {
                BlastError::Unavailable(format!("unexpected redirect (HTTP {})", status.as_u16()))
            }
            409 => BlastError::NotReady(body.state.unwrap_or_else(|| "not complete".into())),
            429 => BlastError::QueueFull {
                retry_after: body.retry_after_seconds.unwrap_or(30),
            },
            s => BlastError::Unavailable(format!("HTTP {s}")),
        })
    }

    pub async fn submit(&self, req: &SubmitRequest) -> Result<SubmitResponse, BlastError> {
        let resp = self
            .send(
                self.auth(self.http.post(self.url("internal/jobs")?))
                    .json(req),
            )
            .await?;
        let r: SubmitResponse = resp
            .json()
            .await
            .map_err(|e| BlastError::Unavailable(format!("bad submit response: {e}")))?;
        if !valid_job_id(&r.job_id) {
            return Err(BlastError::Unavailable(format!(
                "malformed job id from service ({} chars)",
                r.job_id.len()
            )));
        }
        Ok(r)
    }

    /// Job state, with a 5 s budget so polling tools never hold their permit long.
    pub async fn status(&self, job_id: &str) -> Result<JobStatus, BlastError> {
        let rb = self
            .auth(self.http.get(self.url(&format!("internal/jobs/{job_id}"))?))
            .timeout(Duration::from_secs(5));
        let resp = self.send(rb).await?;
        let mut s: JobStatus = resp
            .json()
            .await
            .map_err(|e| BlastError::Unavailable(format!("bad status response: {e}")))?;
        public_links_only(&mut s);
        Ok(s)
    }

    /// The results table, read in chunks and capped at MAX_RESULTS_BYTES.
    pub async fn results_tsv(&self, job_id: &str) -> Result<String, BlastError> {
        let mut resp = self
            .send(
                self.auth(
                    self.http
                        .get(self.url(&format!("internal/jobs/{job_id}/results.tsv"))?),
                ),
            )
            .await?;
        if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
            let media = ct
                .to_str()
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if media != "text/tab-separated-values" && media != "text/plain" {
                return Err(BlastError::Unavailable(format!(
                    "results.tsv has content-type {media:?}, not a TSV table"
                )));
            }
        }
        if resp
            .content_length()
            .is_some_and(|n| n as usize > MAX_RESULTS_BYTES)
        {
            return Err(BlastError::Unavailable(
                "results.tsv exceeds the 16 MiB limit".into(),
            ));
        }
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| BlastError::Unavailable(e.to_string()))?
        {
            if buf.len() + chunk.len() > MAX_RESULTS_BYTES {
                return Err(BlastError::Unavailable(
                    "results.tsv exceeds the 16 MiB limit".into(),
                ));
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// True when `GET /internal/health` answers 2xx within 2 s.
    pub async fn healthy(&self) -> bool {
        match self.url("internal/health") {
            Ok(u) => self
                .http
                .get(u)
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success()),
            Err(_) => false,
        }
    }
}

/// job_id -> SHA-256 of the token handed to the submitter. Bounded and time-limited; a restart forgets jobs,
/// which then read as not found (the public result pages still work).
pub struct JobTokens {
    cache: moka::sync::Cache<String, [u8; 32]>,
}

fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

impl JobTokens {
    pub fn new() -> Self {
        Self {
            cache: moka::sync::Cache::builder()
                .max_capacity(TOKEN_CAPACITY)
                .time_to_live(TOKEN_TTL)
                .build(),
        }
    }

    /// Mint a token for `job_id` and remember its hash.
    pub fn issue(&self, job_id: &str) -> String {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.cache.insert(job_id.to_string(), hash(&token));
        token
    }

    pub fn check(&self, job_id: &str, token: &str) -> bool {
        self.cache.get(job_id).is_some_and(|h| h == hash(token))
    }

    pub fn len(&self) -> u64 {
        self.cache.run_pending_tasks();
        self.cache.entry_count()
    }

    // Exists only to satisfy clippy::len_without_is_empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for JobTokens {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Hsp {
    pub query_start: u64,
    pub query_end: u64,
    pub subject_start: u64,
    pub subject_end: u64,
    pub align_length: u64,
    pub identity: f64,
    pub evalue: f64,
    pub bitscore: f64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Hit {
    /// The subject sequence ID as BLAST printed it (sseqid).
    pub subject_id: String,
    /// The accession part of the subject ID: the last non-empty `|` field (gi|123|gb|AY1 gives AY1).
    pub subject_accession: String,
    pub flybase_id: Option<String>,
    pub description: Option<String>,
    pub evalue: f64,
    pub bitscore: f64,
    pub identity: f64,
    pub hsp_count: usize,
    pub hsps: Vec<Hsp>,
}

/// First FlyBase ID (FB + two lowercase letters + 7 or more digits) in `s`.
pub fn find_fb_id(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i + 11 <= b.len() {
        if &b[i..i + 2] == b"FB" && b[i + 2].is_ascii_lowercase() && b[i + 3].is_ascii_lowercase() {
            let digits = b[i + 4..].iter().take_while(|c| c.is_ascii_digit()).count();
            if digits >= 7 {
                return Some(s[i..i + 4 + digits].to_string());
            }
        }
        i += 1;
    }
    None
}

fn accession(sseqid: &str) -> String {
    sseqid
        .split('|')
        .rfind(|p| !p.is_empty())
        .unwrap_or(sseqid)
        .to_string()
}

/// Parse `-outfmt "6 std stitle"` rows into hits grouped by subject in order of first appearance.
/// Returns the first `max_hits` hits (each with at most `max_hsps` HSPs) and the total number of hits.
pub fn parse_results(tsv: &str, max_hits: usize, max_hsps: usize) -> (Vec<Hit>, usize) {
    let mut hits: Vec<Hit> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for line in tsv.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("qseqid\t") {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 12 {
            continue;
        }
        let num = |i: usize| f[i].trim().parse::<f64>().unwrap_or(0.0);
        let int = |i: usize| f[i].trim().parse::<u64>().unwrap_or(0);
        let hsp = Hsp {
            query_start: int(6),
            query_end: int(7),
            subject_start: int(8),
            subject_end: int(9),
            align_length: int(3),
            identity: num(2),
            evalue: num(10),
            bitscore: num(11),
        };
        let subject = f[1].to_string();
        let i = match index.get(&subject) {
            Some(&i) => i,
            None => {
                let title = f.get(12).map(|t| t.trim()).filter(|t| !t.is_empty());
                let description = title
                    .map(|t| t.strip_prefix(&subject).unwrap_or(t).trim().to_string())
                    .filter(|d| !d.is_empty());
                let flybase_id = find_fb_id(&subject).or_else(|| title.and_then(find_fb_id));
                hits.push(Hit {
                    subject_accession: accession(&subject),
                    subject_id: subject.clone(),
                    flybase_id,
                    description,
                    evalue: hsp.evalue,
                    bitscore: hsp.bitscore,
                    identity: hsp.identity,
                    hsp_count: 0,
                    hsps: Vec::new(),
                });
                index.insert(subject, hits.len() - 1);
                hits.len() - 1
            }
        };
        let h = &mut hits[i];
        h.hsp_count += 1;
        h.evalue = h.evalue.min(hsp.evalue);
        h.bitscore = h.bitscore.max(hsp.bitscore);
        if h.hsps.len() < max_hsps {
            h.hsps.push(hsp);
        }
    }
    let total = hits.len();
    hits.truncate(max_hits);
    (hits, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_codes_name_the_tool_argument() {
        for (code, want) in [
            ("invalid_program", "program"),
            ("invalid_database", "database"),
            ("invalid_species", "species"),
            ("invalid_sequence", "sequence"),
            ("invalid_evalue", "evalue"),
            ("invalid_max_hits", "max_hits"),
            ("invalid_matrix", "matrix"),
            ("invalid_megablast", "megablast"),
            ("invalid_genetic_code", "genetic_code"),
            ("invalid_option", "advanced"),
            ("something_new", "sequence"),
        ] {
            let e = BlastError::Rejected {
                code: code.into(),
                message: "m".into(),
            }
            .into_tool("blast_submit");
            match e {
                ToolError::InvalidInput { arg, .. } => assert_eq!(arg, want, "{code}"),
                other => panic!("{code}: {other:?}"),
            }
        }
    }

    #[test]
    fn tokens_round_trip_and_reject_mismatch() {
        let t = JobTokens::new();
        let tok = t.issue("job-0001");
        assert_eq!(tok.len(), 64);
        assert!(t.check("job-0001", &tok));
        assert!(!t.check("job-0001", &"0".repeat(64)));
        assert!(!t.check("job-0002", &tok));
        assert_ne!(t.issue("job-0001"), tok, "re-issuing replaces the token");
        assert_eq!(t.len(), 1);
        assert!(!t.is_empty());
    }

    #[test]
    fn job_id_format() {
        assert!(valid_job_id("j1234567"));
        assert!(valid_job_id("abc-DEF_012"));
        assert!(!valid_job_id("short"));
        assert!(!valid_job_id("has/slash1"));
        assert!(!valid_job_id(&"x".repeat(65)));
    }

    #[test]
    fn https_base_is_rejected_at_startup() {
        match BlastClient::new("https://blast.example/".parse().unwrap(), None) {
            Ok(_) => panic!("https base must be rejected"),
            Err(e) => assert!(e.to_string().contains("http://"), "{e}"),
        }
    }

    #[test]
    fn base_url_keeps_its_path() {
        let c = BlastClient::new("http://127.0.0.1:9/blast".parse().unwrap(), None).unwrap();
        assert_eq!(
            c.url("internal/jobs").unwrap().as_str(),
            "http://127.0.0.1:9/blast/internal/jobs"
        );
        let c = BlastClient::new("http://127.0.0.1:9/blast/".parse().unwrap(), None).unwrap();
        assert_eq!(
            c.url("internal/health").unwrap().as_str(),
            "http://127.0.0.1:9/blast/internal/health"
        );
    }

    #[test]
    fn parses_hits_and_hsps() {
        let tsv = "qseqid\tsseqid\tpident\tlength\tmismatch\tgapopen\tqstart\tqend\tsstart\tsend\tevalue\tbitscore\tstitle\n\
                   q\tFBtr0070507\t100.000\t60\t0\t0\t1\t60\t101\t160\t1e-30\t111\tFBtr0070507 type=mRNA; name=N-RA\n\
                   q\tFBtr0070507\t95.000\t40\t2\t0\t5\t44\t900\t939\t1e-10\t60.2\tFBtr0070507 type=mRNA; name=N-RA\n\
                   q\tgi|12345|gb|AY1\t90.000\t50\t5\t0\t1\t50\t10\t59\t1e-12\t65.8\tsome GenBank record\n";
        let (hits, total) = parse_results(tsv, 10, 1);
        assert_eq!(total, 2);
        assert_eq!(hits[0].subject_id, "FBtr0070507");
        assert_eq!(hits[0].subject_accession, "FBtr0070507");
        assert_eq!(hits[0].flybase_id.as_deref(), Some("FBtr0070507"));
        assert_eq!(hits[0].description.as_deref(), Some("type=mRNA; name=N-RA"));
        assert_eq!(hits[0].hsp_count, 2);
        assert_eq!(hits[0].hsps.len(), 1);
        assert_eq!(hits[0].evalue, 1e-30);
        assert_eq!(hits[0].bitscore, 111.0);
        assert_eq!(hits[1].subject_accession, "AY1");
        assert_eq!(hits[1].flybase_id, None);
        assert_eq!(hits[1].description.as_deref(), Some("some GenBank record"));
        assert_eq!(parse_results(tsv, 1, 5).0.len(), 1);
        assert_eq!(parse_results("", 5, 5), (Vec::new(), 0));
    }

    #[test]
    fn finds_flybase_ids() {
        assert_eq!(
            find_fb_id("x FBgn0004647 y").as_deref(),
            Some("FBgn0004647")
        );
        assert_eq!(
            find_fb_id("FBog0000245780").as_deref(),
            Some("FBog0000245780")
        );
        assert!(find_fb_id("FBgn123").is_none());
        assert!(find_fb_id("fbgn0004647").is_none());
    }
}
