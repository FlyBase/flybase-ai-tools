//! Service-side validation of a submit (design 3.4), independent of the MCP's own checks.
use crate::advanced;
use crate::catalogue::{Catalogue, Resolved};
use crate::config::Config;
use crate::types::{Program, Rejection, SubmitRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// Most species in one search.
pub const MAX_SPECIES: usize = 20;
pub const MATRICES: &[&str] = &["BLOSUM62", "BLOSUM45", "BLOSUM80", "PAM30", "PAM70"];
/// Query alphabet for blastn, blastx and tblastx (IUPAC nucleotide codes plus gap).
pub const NUCLEOTIDE: &str = "ACGTUNRYKMSWBDHV-";
/// Query alphabet for blastp and tblastn: the 20 amino acids plus `BJOUXZ*-`.
pub const PROTEIN: &str = "ACDEFGHIKLMNPQRSTVWYBJOUXZ*-";
const CLIENT_MAX: usize = 64;
/// Longest piece of a caller's string echoed back in a rejection message, in characters.
pub const ECHO_MAX: usize = 40;

/// `s` quoted for a rejection message, cut to `ECHO_MAX` characters (on a char boundary) with `…`
/// appended when longer, so a huge field never produces a huge error body.
pub fn quoted(s: &str) -> String {
    match s.char_indices().nth(ECHO_MAX) {
        None => format!("{s:?}"),
        Some((cut, _)) => format!("{:?}…", &s[..cut]),
    }
}

/// The NCBI genetic codes: 1-6, 9-16, 21-31, 33 (no 7, 8, 17-20, 32).
pub fn genetic_code_known(g: u32) -> bool {
    matches!(g, 1..=6 | 9..=16 | 21..=31 | 33)
}

/// A submit that passed every check, with defaults applied and databases resolved, ready for the
/// store and the command builder.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Validated {
    pub program: Program,
    /// Catalogue category.
    pub database: String,
    /// Lowercase species codes, deduplicated, in request order.
    pub species: Vec<String>,
    /// Query residues: uppercase, whitespace and the FASTA header removed.
    pub residues: String,
    pub evalue: f64,
    pub max_hits: u32,
    /// `true` only for blastn.
    pub megablast: bool,
    /// Canonical uppercase matrix name; never set for blastn.
    pub matrix: Option<String>,
    /// Set only for translated programs (blastx, tblastn, tblastx); ignored and dropped otherwise.
    pub genetic_code: Option<u32>,
    /// Parsed `advanced` pairs (`advanced::parse`): flag without `-`, value `None` for a Switch.
    pub advanced: Vec<(String, Option<String>)>,
    pub resolved: Resolved,
    pub client: Option<String>,
}

/// Debug without the residues, so a stray `{:?}` never logs a query.
impl fmt::Debug for Validated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Validated")
            .field("program", &self.program)
            .field("database", &self.database)
            .field("species", &self.species)
            .field("residues_len", &self.residues.len())
            .field("evalue", &self.evalue)
            .field("max_hits", &self.max_hits)
            .field("megablast", &self.megablast)
            .field("matrix", &self.matrix)
            .field("genetic_code", &self.genetic_code)
            .field("advanced", &self.advanced)
            .field("resolved", &self.resolved)
            .field("client", &self.client)
            .finish()
    }
}

/// Validate `req` against the config and catalogue. Checks run in this order: program
/// (`invalid_program`), database category and molecule type (`invalid_database`), species
/// (`invalid_species`), sequence (`invalid_sequence`), then `evalue`, `max_hits`, `matrix`,
/// `megablast`, `genetic_code`, `advanced` and `client` (`invalid_option`, message naming the field).
pub fn check(req: &SubmitRequest, cfg: &Config, cat: &Catalogue) -> Result<Validated, Rejection> {
    let program = Program::parse(&req.program).ok_or_else(|| {
        Rejection::program(format!(
            "program {} is not one of blastn, blastp, blastx, tblastn, tblastx",
            quoted(&req.program)
        ))
    })?;

    // Category and molecule type are checked before the species, so a wrong database is reported as
    // such rather than as every species lacking it.
    match cat.category_protein(&req.database) {
        None => return Err(cat.unknown_database(&req.database)),
        Some(protein) if protein != program.protein_database() => {
            let (want, have) = if program.protein_database() {
                ("a protein", "nucleotide")
            } else {
                ("a nucleotide", "protein")
            };
            return Err(Rejection::database(format!(
                "{program} searches {want} database; {} is {have}",
                quoted(&req.database)
            )));
        }
        Some(_) => {}
    }

    let species = species(&req.species)?;
    let resolved = cat.resolve(&species, &req.database)?;
    let residues = sequence(&req.sequence, program, cfg.residue_cap(program))?;

    let evalue = match req.evalue {
        None => cfg.default_evalue,
        Some(e) if e.is_finite() && e > 0.0 && e <= 1000.0 => e,
        Some(e) => {
            return Err(Rejection::option(format!(
                "evalue {e} must be greater than 0 and at most 1000"
            )));
        }
    };
    let max_hits = match req.max_hits {
        None => cfg.default_max_hits,
        Some(n) if (1..=1000).contains(&n) => n,
        Some(n) => {
            return Err(Rejection::option(format!("max_hits {n} must be 1 to 1000")));
        }
    };
    let matrix = match req.matrix.as_deref() {
        None => None,
        Some(_) if program == Program::Blastn => {
            return Err(Rejection::option("matrix does not apply to blastn"));
        }
        Some(m) => {
            let up = m.trim().to_ascii_uppercase();
            if !MATRICES.contains(&up.as_str()) {
                return Err(Rejection::option(format!(
                    "matrix {} is not one of {}",
                    quoted(m),
                    MATRICES.join(" ")
                )));
            }
            Some(up)
        }
    };
    let megablast = req.megablast == Some(true);
    if megablast && program != Program::Blastn {
        return Err(Rejection::option("megablast applies to blastn only"));
    }
    let genetic_code = match req.genetic_code {
        None => None,
        Some(g) if !genetic_code_known(g) => {
            return Err(Rejection::option(format!(
                "genetic_code {g} is not an NCBI genetic code (1-6, 9-16, 21-31, 33)"
            )));
        }
        Some(g) => program.translated().then_some(g),
    };
    let advanced = match req.advanced.as_deref() {
        None => Vec::new(),
        Some(a) => advanced::parse(program, a)?,
    };
    if megablast
        && let Some((_, Some(task))) = advanced.iter().find(|(f, _)| f == "task")
        && task != "megablast"
    {
        return Err(Rejection::option(format!(
            "advanced: -task {task} conflicts with megablast: true"
        )));
    }
    let client = match req.client.as_deref() {
        None => None,
        Some(c)
            if !c.is_empty()
                && c.len() <= CLIENT_MAX
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) =>
        {
            Some(c.to_string())
        }
        Some(_) => {
            return Err(Rejection::option(format!(
                "client must be 1 to {CLIENT_MAX} characters of [A-Za-z0-9_.-]"
            )));
        }
    };

    Ok(Validated {
        program,
        database: req.database.clone(),
        species,
        residues,
        evalue,
        max_hits,
        megablast,
        matrix,
        genetic_code,
        advanced,
        resolved,
        client,
    })
}

/// `check` with the argument order the API handler uses.
pub fn validate(cfg: &Config, cat: &Catalogue, req: SubmitRequest) -> Result<Validated, Rejection> {
    check(&req, cfg, cat)
}

fn species(raw: &[String]) -> Result<Vec<String>, Rejection> {
    if raw.is_empty() {
        return Err(Rejection::species("give at least one species"));
    }
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for s in raw {
        let ok = !s.is_empty()
            && s.len() <= 32
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !ok {
            return Err(Rejection::species(format!(
                "species {} must be a lowercase code such as \"dmel\"",
                quoted(s)
            )));
        }
        if seen.insert(s.as_str()) {
            out.push(s.clone());
            // Stop at the first species over the cap, so a huge list costs no more than 21 entries.
            if out.len() > MAX_SPECIES {
                return Err(Rejection::species(format!(
                    "at most {MAX_SPECIES} species per search"
                )));
            }
        }
    }
    Ok(out)
}

/// One FASTA record (optional `>` header line) to uppercase residues.
fn sequence(raw: &str, program: Program, cap: usize) -> Result<String, Rejection> {
    let mut lines = raw.lines().skip_while(|l| l.trim().is_empty()).peekable();
    if lines
        .peek()
        .is_some_and(|l| l.trim_start().starts_with('>'))
    {
        lines.next();
    }
    let mut residues = String::new();
    for line in lines {
        if line.trim_start().starts_with('>') {
            return Err(Rejection::sequence(
                "give exactly one FASTA record; found a second '>' header",
            ));
        }
        residues.extend(
            line.chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| c.to_ascii_uppercase()),
        );
    }
    let n = residues.chars().count();
    if n == 0 {
        return Err(Rejection::sequence("the query has no residues"));
    }
    if n > cap {
        return Err(Rejection::sequence(format!(
            "the query has {n} residues; {program} accepts at most {cap}"
        )));
    }
    let (alphabet, kind) = if program.protein_query() {
        (PROTEIN, "amino-acid")
    } else {
        (NUCLEOTIDE, "nucleotide")
    };
    let mut bad: Vec<char> = Vec::new();
    for c in residues.chars() {
        if !alphabet.contains(c) && !bad.contains(&c) {
            bad.push(c);
        }
    }
    if !bad.is_empty() {
        let shown: Vec<String> = bad.iter().take(10).map(|c| format!("{c:?}")).collect();
        return Err(Rejection::sequence(format!(
            "{program} takes a {kind} query; these characters are not {kind} codes: {}",
            shown.join(" ")
        )));
    }
    Ok(residues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::tests::{fixture, fixture_dir};
    use crate::types::Code;

    fn cfg() -> Config {
        serde_json::from_value(crate::config::tests::sample()).unwrap()
    }

    fn req(program: &str, database: &str, sequence: &str) -> SubmitRequest {
        SubmitRequest {
            program: program.into(),
            database: database.into(),
            species: vec!["dmel".into()],
            sequence: sequence.into(),
            client: Some("flybase-mcp".into()),
            ..Default::default()
        }
    }

    fn run(r: &SubmitRequest) -> Result<Validated, Rejection> {
        check(r, &cfg(), &fixture())
    }

    fn rejected(r: &SubmitRequest, code: Code) -> String {
        match run(r) {
            Ok(v) => panic!("expected {code:?}, got {v:?}"),
            Err(e) => {
                assert_eq!(e.code, code, "{}", e.message);
                e.message
            }
        }
    }

    #[test]
    fn happy_path_cleans_and_defaults() {
        let v = run(&req("blastn", "assembly", ">query\r\nacgt nn\n\nACGU-\n")).unwrap();
        assert_eq!(v.program, Program::Blastn);
        assert_eq!(v.residues, "ACGTNNACGU-");
        assert_eq!(v.evalue, 10.0);
        assert_eq!(v.max_hits, 100);
        assert!(!v.megablast && v.matrix.is_none() && v.genetic_code.is_none());
        assert!(v.advanced.is_empty());
        assert_eq!(v.client.as_deref(), Some("flybase-mcp"));
        assert_eq!(
            v.resolved.prefixes,
            vec![fixture_dir().join("db/dmel/dmel-assembly")]
        );
        assert!(!format!("{v:?}").contains("ACGTNN"), "Debug hides residues");
        // No header line at all is fine too.
        assert_eq!(
            run(&req("blastn", "assembly", "ACGT")).unwrap().residues,
            "ACGT"
        );
    }

    #[test]
    fn validate_alias_matches_check() {
        let r = req("blastp", "translation", "MKV");
        assert_eq!(
            validate(&cfg(), &fixture(), r.clone()).unwrap(),
            run(&r).unwrap()
        );
    }

    #[test]
    fn invalid_program() {
        for p in ["psiblast", "BLASTN", ""] {
            assert!(rejected(&req(p, "assembly", "ACGT"), Code::InvalidProgram).contains("blastn"));
        }
    }

    #[test]
    fn invalid_database() {
        let m = rejected(&req("blastn", "est", "ACGT"), Code::InvalidDatabase);
        assert!(m.contains("est"), "{m}");
        let m = rejected(&req("blastp", "assembly", "MKV"), Code::InvalidDatabase);
        assert!(m.contains("protein database"), "{m}");
        let m = rejected(&req("blastn", "translation", "ACGT"), Code::InvalidDatabase);
        assert!(m.contains("nucleotide database"), "{m}");
        rejected(&req("tblastn", "genpept", "MKV"), Code::InvalidDatabase);
        rejected(&req("blastx", "genbank", "ACGT"), Code::InvalidDatabase);
    }

    #[test]
    fn invalid_species() {
        let mut r = req("blastn", "assembly", "ACGT");
        r.species = vec![];
        assert!(rejected(&r, Code::InvalidSpecies).contains("at least one"));
        r.species = vec!["Dmel".into()];
        assert!(rejected(&r, Code::InvalidSpecies).contains("lowercase"));
        r.species = vec!["dmel".into(), "dsim".into()];
        r.database = "transcript".into();
        assert!(rejected(&r, Code::InvalidSpecies).contains("dsim"));
        r.database = "assembly".into();
        r.species = (0..21).map(|i| format!("sp{i}")).collect();
        assert!(rejected(&r, Code::InvalidSpecies).contains("at most 20"));
        r.species = vec!["dmel".into(), "dyak".into()];
        assert!(rejected(&r, Code::InvalidSpecies).contains("dyak"));
    }

    #[test]
    fn echoed_strings_are_clipped() {
        let huge = "x".repeat(500_000);
        let m = rejected(&req(&huge, "assembly", "ACGT"), Code::InvalidProgram);
        assert!(m.len() < 200, "message is {} bytes", m.len());
        assert!(m.contains(&format!("{:?}…", "x".repeat(ECHO_MAX))), "{m}");
        let m = rejected(&req("blastn", &huge, "ACGT"), Code::InvalidDatabase);
        assert!(m.len() < 300, "message is {} bytes", m.len());
        let mut r = req("blastn", "assembly", "ACGT");
        r.species = vec![huge.clone()];
        assert!(rejected(&r, Code::InvalidSpecies).len() < 200);
        let mut r = req("blastp", "translation", "MKV");
        r.matrix = Some(huge.clone());
        assert!(rejected(&r, Code::InvalidOption).len() < 200);
        // Cut on a char boundary for multi-byte input; short strings are untouched.
        assert_eq!(
            quoted(&"é".repeat(50)),
            format!("{:?}…", "é".repeat(ECHO_MAX))
        );
        assert_eq!(quoted("blastq"), "\"blastq\"");
        assert_eq!(
            quoted(&"y".repeat(ECHO_MAX)),
            format!("{:?}", "y".repeat(ECHO_MAX))
        );
    }

    #[test]
    fn huge_species_list_is_rejected_fast() {
        let mut r = req("blastn", "assembly", "ACGT");
        r.species = (0..100_000).map(|i| format!("sp{i}")).collect();
        let t = std::time::Instant::now();
        assert!(rejected(&r, Code::InvalidSpecies).contains("at most 20"));
        assert!(
            t.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            t.elapsed()
        );
        // Repeats of one code collapse to one species and pass.
        r.species = vec!["dmel".to_string(); 100_000];
        assert_eq!(run(&r).unwrap().species, vec!["dmel"]);
    }

    #[test]
    fn species_are_deduplicated_in_order() {
        let mut r = req("blastn", "assembly", "ACGT");
        r.species = vec!["dsim".into(), "dmel".into(), "dsim".into()];
        let v = run(&r).unwrap();
        assert_eq!(v.species, vec!["dsim", "dmel"]);
        assert_eq!(v.resolved.prefixes.len(), 2);
    }

    #[test]
    fn invalid_sequence() {
        for s in ["", "   \n", ">query\n", ">query only header"] {
            assert!(
                rejected(&req("blastn", "assembly", s), Code::InvalidSequence)
                    .contains("no residues")
            );
        }
        let m = rejected(
            &req("blastn", "assembly", ">a\nACGT\n>b\nACGT\n"),
            Code::InvalidSequence,
        );
        assert!(m.contains("exactly one FASTA record"), "{m}");
        let m = rejected(
            &req("blastn", "assembly", ">q\nMKLEFPQ\n"),
            Code::InvalidSequence,
        );
        assert!(
            m.contains("not nucleotide codes") && m.contains("'E'") && m.contains("'L'"),
            "{m}"
        );
        assert!(!m.contains("'K'"), "K is an IUPAC nucleotide code: {m}");
        let m = rejected(&req("blastp", "translation", "MKV1"), Code::InvalidSequence);
        assert!(
            m.contains("not amino-acid codes") && m.contains("'1'"),
            "{m}"
        );
        rejected(&req("blastn", "assembly", "ACGT*"), Code::InvalidSequence);
        rejected(&req("blastn", "assembly", "ACGTé"), Code::InvalidSequence);
    }

    #[test]
    fn protein_alphabet_extras_are_accepted() {
        let v = run(&req("blastp", "translation", "mkvBJOUXZ*-")).unwrap();
        assert_eq!(v.residues, "MKVBJOUXZ*-");
        run(&req("tblastn", "assembly", "MKV*")).unwrap();
        run(&req("blastx", "translation", "ACGTRYKMSWBDHVN")).unwrap();
    }

    #[test]
    fn residue_cap_per_program() {
        let cases = [
            ("blastn", "assembly", 100_000),
            ("blastp", "translation", 100_000),
            ("blastx", "translation", 30_000),
            ("tblastn", "assembly", 30_000),
            ("tblastx", "assembly", 10_000),
        ];
        for (p, db, cap) in cases {
            run(&req(p, db, &"A".repeat(cap))).unwrap();
            let m = rejected(&req(p, db, &"A".repeat(cap + 1)), Code::InvalidSequence);
            assert!(
                m.contains(&format!("at most {cap}")) && m.contains(p),
                "{m}"
            );
        }
        // Whitespace and the header do not count toward the cap.
        let wrapped = format!(">query\n{}\n", "A\n".repeat(10_000));
        run(&req("tblastx", "assembly", &wrapped)).unwrap();
    }

    #[test]
    fn evalue_and_max_hits() {
        let mut r = req("blastn", "assembly", "ACGT");
        for e in [0.0, -1.0, 1000.5, f64::NAN, f64::INFINITY] {
            r.evalue = Some(e);
            assert!(rejected(&r, Code::InvalidOption).contains("evalue"));
        }
        r.evalue = Some(1000.0);
        assert_eq!(run(&r).unwrap().evalue, 1000.0);
        r.evalue = Some(1e-30);
        assert_eq!(run(&r).unwrap().evalue, 1e-30);
        r.evalue = None;
        for n in [0, 1001] {
            r.max_hits = Some(n);
            assert!(rejected(&r, Code::InvalidOption).contains("max_hits"));
        }
        r.max_hits = Some(1000);
        assert_eq!(run(&r).unwrap().max_hits, 1000);
    }

    #[test]
    fn matrix_rules() {
        let mut r = req("blastn", "assembly", "ACGT");
        r.matrix = Some("BLOSUM62".into());
        assert!(rejected(&r, Code::InvalidOption).contains("matrix does not apply to blastn"));
        let mut r = req("blastp", "translation", "MKV");
        r.matrix = Some("pam30".into());
        assert_eq!(run(&r).unwrap().matrix.as_deref(), Some("PAM30"));
        r.matrix = Some("BLOSUM90".into());
        assert!(rejected(&r, Code::InvalidOption).contains("matrix"));
    }

    #[test]
    fn megablast_rules() {
        let mut r = req("blastp", "translation", "MKV");
        r.megablast = Some(true);
        assert!(rejected(&r, Code::InvalidOption).contains("megablast"));
        r.megablast = Some(false);
        run(&r).unwrap();
        let mut r = req("blastn", "assembly", "ACGT");
        r.megablast = Some(true);
        assert!(run(&r).unwrap().megablast);
        r.advanced = Some("-task megablast".into());
        run(&r).unwrap();
        r.advanced = Some("-task blastn-short".into());
        assert!(rejected(&r, Code::InvalidOption).contains("conflicts with megablast"));
        // megablast:false with -task megablast is an allowed override (Task 3 argv tests).
        r.megablast = Some(false);
        assert_eq!(
            run(&r).unwrap().advanced,
            vec![("task".to_string(), Some("blastn-short".to_string()))]
        );
    }

    #[test]
    fn genetic_code_set() {
        let mut r = req("tblastx", "assembly", "ACGT");
        for g in 0..=40u32 {
            r.genetic_code = Some(g);
            let known = matches!(g, 1..=6 | 9..=16 | 21..=31 | 33);
            match run(&r) {
                Ok(v) => {
                    assert!(known, "{g} accepted");
                    assert_eq!(v.genetic_code, Some(g));
                }
                Err(e) => {
                    assert!(!known, "{g} rejected");
                    assert_eq!(e.code, Code::InvalidOption);
                    assert!(e.message.contains("genetic_code"));
                }
            }
        }
        // Valid but ignored for programs that do not translate.
        let mut r = req("blastn", "assembly", "ACGT");
        r.genetic_code = Some(2);
        assert_eq!(run(&r).unwrap().genetic_code, None);
        let mut r = req("blastp", "translation", "MKV");
        r.genetic_code = Some(2);
        assert_eq!(run(&r).unwrap().genetic_code, None);
        let mut r = req("blastx", "translation", "ACGT");
        r.genetic_code = Some(2);
        assert_eq!(run(&r).unwrap().genetic_code, Some(2));
    }

    #[test]
    fn advanced_is_parsed_and_rejections_pass_through() {
        let mut r = req("blastn", "assembly", "ACGT");
        r.advanced = Some("-penalty -3 -lcase_masking".into());
        assert_eq!(
            run(&r).unwrap().advanced,
            vec![
                ("penalty".to_string(), Some("-3".to_string())),
                ("lcase_masking".to_string(), None)
            ]
        );
        r.advanced = Some("-remote".into());
        assert!(rejected(&r, Code::InvalidOption).contains("-remote"));
        r.advanced = Some("".into());
        assert!(run(&r).unwrap().advanced.is_empty());
    }

    #[test]
    fn client_is_checked() {
        let mut r = req("blastn", "assembly", "ACGT");
        r.client = None;
        assert_eq!(run(&r).unwrap().client, None);
        for c in ["", "has space", &"x".repeat(65), "new\nline"] {
            r.client = Some(c.to_string());
            assert!(rejected(&r, Code::InvalidOption).contains("client"));
        }
    }

    #[test]
    fn validated_round_trips_through_json() {
        let mut r = req("tblastn", "assembly", "MKV");
        r.genetic_code = Some(11);
        r.advanced = Some("-seg no -ungapped".into());
        let v = run(&r).unwrap();
        let back: Validated = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back, v);
    }
}
