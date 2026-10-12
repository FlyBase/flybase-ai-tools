//! Wire types of the internal job API (`docs/handoff-private-side.md` §7) and the program enum.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// The five BLAST+ search programs the service runs.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        Program::Blastn,
        Program::Blastp,
        Program::Blastx,
        Program::Tblastn,
        Program::Tblastx,
    ];

    pub fn parse(s: &str) -> Option<Program> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Program::Blastn => "blastn",
            Program::Blastp => "blastp",
            Program::Blastx => "blastx",
            Program::Tblastn => "tblastn",
            Program::Tblastx => "tblastx",
        }
    }

    /// The query is protein (blastp, tblastn); the others take nucleotide queries.
    pub fn protein_query(self) -> bool {
        matches!(self, Program::Blastp | Program::Tblastn)
    }

    /// The database is protein (blastp, blastx); the others search nucleotide databases.
    pub fn protein_database(self) -> bool {
        matches!(self, Program::Blastp | Program::Blastx)
    }

    /// Programs that translate the query or the database (they take a genetic code and the shorter
    /// `translated_wall_seconds` limit).
    pub fn translated(self) -> bool {
        matches!(self, Program::Blastx | Program::Tblastn | Program::Tblastx)
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `POST /internal/jobs` body. No `deny_unknown_fields`: a key a future MCP adds must not break submits.
/// Fields are loosely typed so that a wrong value becomes a coded rejection from `validate::check`
/// rather than a deserialisation error.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct SubmitRequest {
    pub program: String,
    pub database: String,
    #[serde(default)]
    pub species: Vec<String>,
    pub sequence: String,
    #[serde(default)]
    pub evalue: Option<f64>,
    #[serde(default)]
    pub megablast: Option<bool>,
    #[serde(default)]
    pub max_hits: Option<u32>,
    #[serde(default)]
    pub matrix: Option<String>,
    #[serde(default)]
    pub genetic_code: Option<u32>,
    #[serde(default)]
    pub advanced: Option<String>,
    #[serde(default)]
    pub client: Option<String>,
}

/// Debug without the query sequence, so a stray `{:?}` never logs residues.
impl fmt::Debug for SubmitRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubmitRequest")
            .field("program", &self.program)
            .field("database", &self.database)
            .field("species", &self.species)
            .field("sequence_len", &self.sequence.len())
            .field("evalue", &self.evalue)
            .field("megablast", &self.megablast)
            .field("max_hits", &self.max_hits)
            .field("matrix", &self.matrix)
            .field("genetic_code", &self.genetic_code)
            .field("advanced", &self.advanced)
            .field("client", &self.client)
            .finish()
    }
}

/// Job state as the contract spells it. The service never produces `Cancelled` (there is no cancel
/// route); it is here because the contract lists it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Complete => "complete",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }
}

/// `202` body of `POST /internal/jobs`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SubmitResponse {
    pub job_id: String,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_after_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_release: Option<String>,
}

/// `200` body of `GET /internal/jobs/{id}`. `result_url` and `downloads` stay `None` (ruling 5) and are
/// then omitted from the JSON.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct JobStatus {
    pub job_id: String,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<HashMap<String, String>>,
    /// Not part of the wire format: set by the store when `results.tsv` was cut at the size cap, so the
    /// results route can add `X-Blast-Jobs-Truncated: true`.
    #[serde(skip)]
    pub truncated: bool,
}

/// Error codes of a `400` answer. The first five are the ones the MCP maps to a tool argument;
/// `InvalidRequest` is for a body that is not a valid request object at all.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    InvalidProgram,
    InvalidDatabase,
    InvalidSpecies,
    InvalidSequence,
    InvalidOption,
    InvalidRequest,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::InvalidProgram => "invalid_program",
            Code::InvalidDatabase => "invalid_database",
            Code::InvalidSpecies => "invalid_species",
            Code::InvalidSequence => "invalid_sequence",
            Code::InvalidOption => "invalid_option",
            Code::InvalidRequest => "invalid_request",
        }
    }
}

/// A `400` rejection; serialises as the contract's `{error, message}`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {message}", code.as_str())]
pub struct Rejection {
    #[serde(rename = "error")]
    pub code: Code,
    pub message: String,
}

impl Rejection {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn program(m: impl Into<String>) -> Self {
        Self::new(Code::InvalidProgram, m)
    }
    pub fn database(m: impl Into<String>) -> Self {
        Self::new(Code::InvalidDatabase, m)
    }
    pub fn species(m: impl Into<String>) -> Self {
        Self::new(Code::InvalidSpecies, m)
    }
    pub fn sequence(m: impl Into<String>) -> Self {
        Self::new(Code::InvalidSequence, m)
    }
    pub fn option(m: impl Into<String>) -> Self {
        Self::new(Code::InvalidOption, m)
    }
    pub fn request(m: impl Into<String>) -> Self {
        Self::new(Code::InvalidRequest, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_serialises_as_error_and_message() {
        let v =
            serde_json::to_value(Rejection::option("advanced: -remote is not allowed")).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"error": "invalid_option", "message": "advanced: -remote is not allowed"})
        );
    }

    #[test]
    fn submit_request_accepts_unknown_keys_and_omitted_optionals() {
        let r: SubmitRequest = serde_json::from_value(serde_json::json!({
            "program": "blastn", "database": "assembly", "species": ["dmel"],
            "sequence": ">query\nACGT\n", "client": "flybase-mcp", "future_key": 1
        }))
        .unwrap();
        assert_eq!(r.program, "blastn");
        assert!(r.evalue.is_none() && r.advanced.is_none());
        let dbg = format!("{r:?}");
        assert!(!dbg.contains("ACGT"), "Debug must not print the sequence");
    }

    #[test]
    fn status_omits_absent_links_and_truncated() {
        let s = JobStatus {
            job_id: "abcdefgh".into(),
            state: State::Complete,
            queue_position: None,
            error: None,
            result_url: None,
            downloads: None,
            truncated: true,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"job_id": "abcdefgh", "state": "complete"})
        );
        let r = SubmitResponse {
            job_id: "abcdefgh".into(),
            state: State::Queued,
            queue_position: Some(1),
            poll_after_seconds: Some(5),
            database_release: Some("FB2023_04".into()),
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({"job_id": "abcdefgh", "state": "queued", "queue_position": 1,
                "poll_after_seconds": 5, "database_release": "FB2023_04"})
        );
    }

    #[test]
    fn program_helpers() {
        for p in Program::ALL {
            assert_eq!(Program::parse(p.as_str()), Some(p));
        }
        assert_eq!(Program::parse("BLASTN"), None);
        assert!(Program::Blastx.protein_database() && !Program::Blastx.protein_query());
        assert!(Program::Tblastn.protein_query() && !Program::Tblastn.protein_database());
        assert!(!Program::Blastp.translated() && Program::Tblastx.translated());
    }
}
