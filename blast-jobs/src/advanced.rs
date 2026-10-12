//! The `advanced` option string (design 3.4): a typed, per-program allow-list of BLAST+ flags. The MCP's
//! own filter only limits length and characters, so this table is the only barrier between a client and
//! the BLAST+ command line.
use crate::types::{Program, Rejection};
use std::collections::HashSet;

/// Longest `advanced` string accepted (the MCP's limit).
pub const MAX_LEN: usize = 200;
/// Most flags (a Switch counts as one) in one `advanced` string.
pub const MAX_PAIRS: usize = 8;

/// How a flag's value is checked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Integer matching `^-?[0-9]+$`, within the inclusive bounds.
    Int { min: Option<i64>, max: Option<i64> },
    /// Number matching `^-?[0-9.]+$`, finite, within the inclusive bounds.
    Float { min: f64, max: Option<f64> },
    /// Exactly one of two literals (`yes`/`no` or `true`/`false`, as BLAST+ spells them for the flag).
    Bool { yes: &'static str, no: &'static str },
    /// No value; rendered as a single argv element.
    Switch,
    /// One of a fixed set of literals.
    Enum(&'static [&'static str]),
}

/// One allowed flag (`name` without the leading `-`) and the programs it applies to. A name may appear
/// in several rows with disjoint program sets when its kind differs per program (`task`, `word_size`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlagSpec {
    pub name: &'static str,
    pub kind: Kind,
    pub programs: &'static [Program],
}

use Program::{Blastn, Blastp, Blastx, Tblastn, Tblastx};

const ALL: &[Program] = &[Blastn, Blastp, Blastx, Tblastn, Tblastx];
const NOT_TBLASTX: &[Program] = &[Blastn, Blastp, Blastx, Tblastn];
const PROTEIN_SCORED: &[Program] = &[Blastp, Blastx, Tblastn];
const SEG: &[Program] = &[Blastp, Blastx, Tblastn, Tblastx];
const NUCLEOTIDE_QUERY: &[Program] = &[Blastn, Blastx, Tblastx];

/// BLAST+ parses integer flags as a C `int`; a larger value fails the job, so it is refused here.
const INT_MAX: i64 = i32::MAX as i64;
const INT_MIN: i64 = i32::MIN as i64;

const fn int_min(min: i64) -> Kind {
    Kind::Int {
        min: Some(min),
        max: Some(INT_MAX),
    }
}

const YES_NO: Kind = Kind::Bool {
    yes: "yes",
    no: "no",
};

/// The allow-list, exactly design 3.4. tblastx has no `task` row: BLAST+ offers it no task choice.
pub const TABLE: &[FlagSpec] = &[
    FlagSpec {
        name: "task",
        kind: Kind::Enum(&["blastn", "blastn-short", "megablast", "dc-megablast"]),
        programs: &[Blastn],
    },
    FlagSpec {
        name: "task",
        kind: Kind::Enum(&["blastp", "blastp-short", "blastp-fast"]),
        programs: &[Blastp],
    },
    FlagSpec {
        name: "task",
        kind: Kind::Enum(&["blastx", "blastx-fast"]),
        programs: &[Blastx],
    },
    FlagSpec {
        name: "task",
        kind: Kind::Enum(&["tblastn", "tblastn-fast"]),
        programs: &[Tblastn],
    },
    FlagSpec {
        name: "word_size",
        kind: int_min(4),
        programs: &[Blastn],
    },
    FlagSpec {
        name: "word_size",
        kind: int_min(2),
        programs: &[Blastp, Blastx, Tblastn, Tblastx],
    },
    FlagSpec {
        name: "gapopen",
        kind: int_min(0),
        programs: NOT_TBLASTX,
    },
    FlagSpec {
        name: "gapextend",
        kind: int_min(0),
        programs: NOT_TBLASTX,
    },
    FlagSpec {
        name: "penalty",
        kind: Kind::Int {
            min: Some(INT_MIN),
            max: Some(0),
        },
        programs: &[Blastn],
    },
    FlagSpec {
        name: "reward",
        kind: int_min(0),
        programs: &[Blastn],
    },
    FlagSpec {
        name: "dust",
        kind: YES_NO,
        programs: &[Blastn],
    },
    FlagSpec {
        name: "seg",
        kind: YES_NO,
        programs: SEG,
    },
    FlagSpec {
        name: "soft_masking",
        kind: Kind::Bool {
            yes: "true",
            no: "false",
        },
        programs: ALL,
    },
    FlagSpec {
        name: "lcase_masking",
        kind: Kind::Switch,
        programs: ALL,
    },
    FlagSpec {
        name: "ungapped",
        kind: Kind::Switch,
        programs: NOT_TBLASTX,
    },
    FlagSpec {
        name: "strand",
        kind: Kind::Enum(&["both", "plus", "minus"]),
        programs: NUCLEOTIDE_QUERY,
    },
    FlagSpec {
        name: "perc_identity",
        kind: Kind::Float {
            min: 0.0,
            max: Some(100.0),
        },
        programs: &[Blastn],
    },
    FlagSpec {
        name: "qcov_hsp_perc",
        kind: Kind::Float {
            min: 0.0,
            max: Some(100.0),
        },
        programs: ALL,
    },
    FlagSpec {
        name: "culling_limit",
        kind: int_min(0),
        programs: ALL,
    },
    FlagSpec {
        name: "max_hsps",
        // No upper bound: `command::build` clamps the value to the service cap, so it never reaches
        // BLAST+ above it.
        kind: Kind::Int {
            min: Some(1),
            max: None,
        },
        programs: ALL,
    },
    FlagSpec {
        name: "threshold",
        kind: Kind::Float {
            min: 0.0,
            max: None,
        },
        programs: PROTEIN_SCORED,
    },
    FlagSpec {
        name: "comp_based_stats",
        kind: Kind::Enum(&["0", "1", "2", "3", "D", "F", "T", "t", "f"]),
        programs: PROTEIN_SCORED,
    },
    FlagSpec {
        name: "window_size",
        kind: int_min(0),
        programs: ALL,
    },
];

/// Flags the service sets from request fields or its own config; named in the rejection so the caller
/// knows where the setting lives. `command::build` also drops them as defence in depth.
pub const SERVICE_OWNED: &[&str] = &[
    "evalue",
    "max_target_seqs",
    "matrix",
    "num_threads",
    "query_gencode",
    "db_gencode",
    "outfmt",
    "db",
    "query",
    "out",
];

/// The design's deny list (3.4), each with the program it is tested against. Every entry is rejected;
/// Task 1 tests them through `parse` and Task 4 through the API.
pub const DENY_LIST: &[(Program, &str)] = &[
    (Blastn, "-db x"),
    (Blastn, "-query x"),
    (Blastn, "-out x"),
    (Blastn, "-remote"),
    (Blastn, "-import_search_strategy x"),
    (Blastn, "-export_search_strategy x"),
    (Blastn, "-outfmt 5"),
    (Blastn, "-num_threads 16"),
    (Blastn, "-subject x"),
    (Blastn, "-in_pssm x"),
    (Blastn, "-gilist x"),
    (Blastn, "-seqidlist x"),
    (Blastn, "-negative_seqidlist x"),
    (Blastn, "-taxids 7227"),
    (Blastn, "-entrez_query x"),
    (Blastn, "-logfile x"),
    (Blastn, "-window_masker_db x"),
    (Blastn, "-db_soft_mask x"),
    (Blastn, "-db_hard_mask x"),
    (Blastn, "-use_index true"),
    (Blastn, "-index_name x"),
    (Blastn, "-html"),
    (Blastn, "-version"),
    (Blastn, "-help"),
    (Blastn, "--word_size 11"),
    (Blastn, "-word_size 11 -word_size 7"),
    (Blastn, "-ungapped yes"),
    (Blastn, "-penalty 3"),
    (Blastn, "-seg yes"),
    (Blastp, "-dust yes"),
    (Blastn, "-task blastp"),
];

fn reject(m: impl std::fmt::Display) -> Rejection {
    Rejection::option(format!("advanced: {m}"))
}

fn allowed_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '=' | '-')
}

/// The row for `name` under `program`, or the rejection naming why there is none.
fn lookup(program: Program, name: &str) -> Result<&'static FlagSpec, Rejection> {
    let mut known = false;
    for spec in TABLE.iter().filter(|s| s.name == name) {
        known = true;
        if spec.programs.contains(&program) {
            return Ok(spec);
        }
    }
    Err(if known {
        reject(format_args!("-{name} does not apply to {program}"))
    } else if SERVICE_OWNED.contains(&name) {
        reject(format_args!(
            "-{name} is set by the service; use the request field or leave it to the service"
        ))
    } else {
        reject(format_args!("-{name} is not an allowed option"))
    })
}

fn check_value(name: &str, kind: Kind, v: &str) -> Result<(), Rejection> {
    let numeric = |extra: &[u8]| {
        let digits = v.strip_prefix('-').unwrap_or(v);
        !digits.is_empty()
            && digits
                .bytes()
                .all(|b| b.is_ascii_digit() || extra.contains(&b))
    };
    let bad = || reject(format_args!("-{name} value {v:?} is not valid"));
    match kind {
        Kind::Int { min, max } => {
            if !numeric(b"") {
                return Err(reject(format_args!("-{name} takes an integer, not {v:?}")));
            }
            let n: i64 = v.parse().map_err(|_| bad())?;
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                return Err(reject(format_args!(
                    "-{name} {n} is out of range ({})",
                    // The C-int bounds are implicit; name them only when they are the limit hit.
                    range(
                        min.filter(|&m| m != INT_MIN || n < m).map(|m| m as f64),
                        max.filter(|&m| m != INT_MAX || n > m).map(|m| m as f64)
                    )
                )));
            }
        }
        Kind::Float { min, max } => {
            if !numeric(b".") {
                return Err(reject(format_args!("-{name} takes a number, not {v:?}")));
            }
            let n: f64 = v.parse().map_err(|_| bad())?;
            if !n.is_finite() || n < min || max.is_some_and(|m| n > m) {
                return Err(reject(format_args!(
                    "-{name} {v} is out of range ({})",
                    range(Some(min), max)
                )));
            }
        }
        Kind::Bool { yes, no } => {
            if v != yes && v != no {
                return Err(reject(format_args!(
                    "-{name} takes {yes} or {no}, not {v:?}"
                )));
            }
        }
        Kind::Enum(values) => {
            if !values.contains(&v) {
                return Err(reject(format_args!(
                    "-{name} takes one of {}, not {v:?}",
                    values.join(" ")
                )));
            }
        }
        Kind::Switch => unreachable!("switches take no value"),
    }
    Ok(())
}

fn range(min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(a), Some(b)) => format!("{a} to {b}"),
        (Some(a), None) => format!("at least {a}"),
        (None, Some(b)) => format!("at most {b}"),
        (None, None) => "any".into(),
    }
}

/// Parse `advanced` for `program` into `(flag, value)` pairs in the order given. `flag` is the table
/// name without the leading `-`; `value` is `None` for a Switch. Accepts `-flag value` and
/// `-flag=value`; values of Int/Float flags may be negative (`-penalty -3`), no other value may start
/// with `-`. Rejects (all `invalid_option`): more than 200 characters or a character outside
/// `[A-Za-z0-9 _.=-]`; a token that is not a `-flag`; a `--` token; a flag not in `TABLE` or not for
/// this program (including the service-owned flags); a flag given twice; a Switch with a value; a
/// missing or invalid value; more than 8 flags.
pub fn parse(program: Program, s: &str) -> Result<Vec<(String, Option<String>)>, Rejection> {
    if s.len() > MAX_LEN {
        return Err(reject(format_args!("at most {MAX_LEN} characters")));
    }
    if let Some(c) = s.chars().find(|&c| !allowed_char(c)) {
        return Err(reject(format_args!(
            "character {c:?} is not allowed (letters, digits, space, _ . = -)"
        )));
    }
    let tokens: Vec<&str> = s.split(' ').filter(|t| !t.is_empty()).collect();
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut seen = HashSet::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        i += 1;
        if tok.starts_with("--") {
            return Err(reject(format_args!(
                "{tok:?}: options start with a single '-'"
            )));
        }
        let Some(body) = tok.strip_prefix('-') else {
            return Err(reject(format_args!("expected a -flag, found {tok:?}")));
        };
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (body, None),
        };
        if name.is_empty() {
            return Err(reject(format_args!("{tok:?} has no flag name")));
        }
        let spec = lookup(program, name)?;
        if !seen.insert(name) {
            return Err(reject(format_args!("-{name} is given more than once")));
        }
        if out.len() == MAX_PAIRS {
            return Err(reject(format_args!("at most {MAX_PAIRS} options")));
        }
        let value = if spec.kind == Kind::Switch {
            let trailing = tokens.get(i).filter(|t| !t.starts_with('-'));
            if inline.is_some() || trailing.is_some() {
                return Err(reject(format_args!("-{name} takes no value")));
            }
            None
        } else {
            let v = match inline {
                Some(v) => v,
                None => {
                    // The next token is the value unless it is another flag; a negative number is a
                    // value only for numeric kinds, and check_value refuses anything else that starts
                    // with '-'.
                    let next = tokens.get(i).copied();
                    let numeric = matches!(spec.kind, Kind::Int { .. } | Kind::Float { .. });
                    match next {
                        Some(n) if !n.starts_with('-') || numeric => {
                            i += 1;
                            n
                        }
                        _ => "",
                    }
                }
            };
            if v.is_empty() {
                return Err(reject(format_args!("-{name} needs a value")));
            }
            check_value(name, spec.kind, v)?;
            Some(v.to_string())
        };
        out.push((name.to_string(), value));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Code;

    fn ok(p: Program, s: &str) -> Vec<(String, Option<String>)> {
        parse(p, s).unwrap_or_else(|e| panic!("{p} {s:?} should parse: {e}"))
    }

    fn err(p: Program, s: &str) -> String {
        match parse(p, s) {
            Ok(v) => panic!("{p} {s:?} should be rejected, parsed {v:?}"),
            Err(e) => {
                assert_eq!(e.code, Code::InvalidOption, "{p} {s:?}");
                assert!(e.message.starts_with("advanced: "), "{}", e.message);
                e.message
            }
        }
    }

    fn pair(f: &str, v: Option<&str>) -> (String, Option<String>) {
        (f.to_string(), v.map(str::to_string))
    }

    /// One row per allowed flag other than `task`, with the programs the design (3.4) gives it. The
    /// program sets are written out from the design, not read from TABLE, so a table edit that drifts
    /// from the design fails here. `bad` is None for a Switch.
    struct Row {
        flag: &'static str,
        valid: &'static str,
        bad: Option<&'static str>,
        programs: &'static [Program],
    }

    const ROWS: &[Row] = &[
        Row {
            flag: "word_size",
            valid: "11",
            bad: Some("1"),
            programs: ALL,
        },
        Row {
            flag: "gapopen",
            valid: "11",
            bad: Some("-1"),
            programs: &[Blastn, Blastp, Blastx, Tblastn],
        },
        Row {
            flag: "gapextend",
            valid: "1",
            bad: Some("-1"),
            programs: &[Blastn, Blastp, Blastx, Tblastn],
        },
        Row {
            flag: "penalty",
            valid: "-3",
            bad: Some("3"),
            programs: &[Blastn],
        },
        Row {
            flag: "reward",
            valid: "2",
            bad: Some("-2"),
            programs: &[Blastn],
        },
        Row {
            flag: "dust",
            valid: "no",
            bad: Some("true"),
            programs: &[Blastn],
        },
        Row {
            flag: "seg",
            valid: "no",
            bad: Some("true"),
            programs: &[Blastp, Blastx, Tblastn, Tblastx],
        },
        Row {
            flag: "soft_masking",
            valid: "false",
            bad: Some("no"),
            programs: ALL,
        },
        Row {
            flag: "lcase_masking",
            valid: "",
            bad: None,
            programs: ALL,
        },
        Row {
            flag: "ungapped",
            valid: "",
            bad: None,
            programs: &[Blastn, Blastp, Blastx, Tblastn],
        },
        Row {
            flag: "strand",
            valid: "minus",
            bad: Some("up"),
            programs: &[Blastn, Blastx, Tblastx],
        },
        Row {
            flag: "perc_identity",
            valid: "90.5",
            bad: Some("100.1"),
            programs: &[Blastn],
        },
        Row {
            flag: "qcov_hsp_perc",
            valid: "50",
            bad: Some("101"),
            programs: ALL,
        },
        Row {
            flag: "culling_limit",
            valid: "5",
            bad: Some("-1"),
            programs: ALL,
        },
        Row {
            flag: "max_hsps",
            valid: "10",
            bad: Some("0"),
            programs: ALL,
        },
        Row {
            flag: "threshold",
            valid: "11.5",
            bad: Some("-1"),
            programs: &[Blastp, Blastx, Tblastn],
        },
        Row {
            flag: "comp_based_stats",
            valid: "D",
            bad: Some("4"),
            programs: &[Blastp, Blastx, Tblastn],
        },
        Row {
            flag: "window_size",
            valid: "40",
            bad: Some("-1"),
            programs: ALL,
        },
    ];

    #[test]
    fn rows_cover_every_table_flag() {
        for spec in TABLE {
            assert!(
                spec.name == "task" || ROWS.iter().any(|r| r.flag == spec.name),
                "no test row for -{}",
                spec.name
            );
        }
    }

    #[test]
    fn each_allowed_flag_valid_wrong_program_bad_value() {
        for row in ROWS {
            for p in Program::ALL {
                let f = row.flag;
                if !row.programs.contains(&p) {
                    let s = if row.bad.is_none() {
                        format!("-{f}")
                    } else {
                        format!("-{f} {}", row.valid)
                    };
                    assert!(err(p, &s).contains("does not apply"), "{p} {s}");
                    continue;
                }
                match row.bad {
                    None => {
                        assert_eq!(ok(p, &format!("-{f}")), vec![pair(f, None)]);
                        assert!(err(p, &format!("-{f} yes")).contains("takes no value"));
                        assert!(err(p, &format!("-{f}=yes")).contains("takes no value"));
                    }
                    Some(bad) => {
                        let want = vec![pair(f, Some(row.valid))];
                        assert_eq!(ok(p, &format!("-{f} {}", row.valid)), want);
                        assert_eq!(ok(p, &format!("-{f}={}", row.valid)), want);
                        err(p, &format!("-{f} {bad}"));
                        assert!(err(p, &format!("-{f}")).contains("needs a value"));
                        assert!(err(p, &format!("-{f}=")).contains("needs a value"));
                    }
                }
            }
        }
    }

    #[test]
    fn task_values_per_program() {
        let cases: &[(Program, &[&str], &str)] = &[
            (
                Blastn,
                &["blastn", "blastn-short", "megablast", "dc-megablast"],
                "blastp",
            ),
            (Blastp, &["blastp", "blastp-short", "blastp-fast"], "blastn"),
            (Blastx, &["blastx", "blastx-fast"], "blastp"),
            (Tblastn, &["tblastn", "tblastn-fast"], "tblastx"),
        ];
        for (p, values, foreign) in cases {
            for v in *values {
                assert_eq!(ok(*p, &format!("-task {v}")), vec![pair("task", Some(v))]);
            }
            assert!(err(*p, &format!("-task {foreign}")).contains("takes one of"));
        }
        assert!(err(Tblastx, "-task tblastx").contains("does not apply to tblastx"));
    }

    #[test]
    fn word_size_minimum_differs_by_program() {
        assert!(err(Blastn, "-word_size 3").contains("at least 4"));
        assert_eq!(
            ok(Blastp, "-word_size 3"),
            vec![pair("word_size", Some("3"))]
        );
        assert!(err(Blastp, "-word_size 1").contains("at least 2"));
    }

    #[test]
    fn deny_list_is_rejected() {
        for (p, s) in DENY_LIST {
            err(*p, s);
        }
        // The program-independent entries are rejected for every program.
        for (_, s) in DENY_LIST
            .iter()
            .filter(|(_, s)| !["-seg yes", "-dust yes", "-task blastp"].contains(s))
        {
            for p in Program::ALL {
                err(p, s);
            }
        }
    }

    #[test]
    fn deny_list_messages_name_the_flag() {
        assert!(err(Blastn, "-remote").contains("-remote is not an allowed option"));
        assert!(err(Blastn, "-outfmt 5").contains("-outfmt is set by the service"));
        assert!(err(Blastn, "-num_threads 16").contains("set by the service"));
        assert!(err(Blastn, "-seg yes").contains("-seg does not apply to blastn"));
        assert!(err(Blastp, "-dust yes").contains("-dust does not apply to blastp"));
        assert!(err(Blastn, "--word_size 11").contains("single '-'"));
        assert!(err(Blastn, "-word_size 11 -word_size 7").contains("more than once"));
        assert!(err(Blastn, "-ungapped yes").contains("takes no value"));
        assert!(err(Blastn, "-penalty 3").contains("at most 0"));
    }

    #[test]
    fn negative_numeric_values() {
        assert_eq!(ok(Blastn, "-penalty -3"), vec![pair("penalty", Some("-3"))]);
        assert_eq!(ok(Blastn, "-penalty=-3"), vec![pair("penalty", Some("-3"))]);
        assert_eq!(ok(Blastn, "-penalty 0"), vec![pair("penalty", Some("0"))]);
        err(Blastn, "-penalty 3");
        // A flag-looking value for a numeric kind is refused, never taken as a separate flag.
        assert!(err(Blastn, "-word_size -remote").contains("integer"));
        assert!(err(Blastn, "-word_size -3").contains("at least 4"));
        // Non-numeric kinds never take a dash value; the dash token is parsed as a (missing) value.
        assert!(err(Blastn, "-strand -plus").contains("needs a value"));
        assert!(err(Blastn, "-strand=-plus").contains("takes one of"));
    }

    #[test]
    fn malformed_numbers() {
        err(Blastn, "-perc_identity 1.2.3");
        err(Blastn, "-perc_identity .");
        err(Blastn, "-perc_identity -");
        err(Blastn, "-word_size 1.5");
        err(Blastn, "-word_size 99999999999999999999");
        // Integers are bounded to a C int, so an oversized value is a 400 rather than a failed job.
        assert!(
            err(Blastn, "-culling_limit 9999999999").contains("out of range (0 to 2147483647)")
        );
        assert!(err(Blastn, "-penalty -9999999999").contains("out of range (-2147483648 to 0)"));
        assert_eq!(
            ok(Blastn, "-culling_limit 2147483647"),
            vec![pair("culling_limit", Some("2147483647"))]
        );
        assert_eq!(
            ok(Blastn, "-perc_identity 100"),
            vec![pair("perc_identity", Some("100"))]
        );
    }

    #[test]
    fn duplicates_and_double_dash() {
        err(Blastn, "-dust no -dust yes");
        err(Blastn, "-dust=no -dust no");
        err(Blastn, "-lcase_masking -lcase_masking");
        err(Blastn, "--remote");
        err(Blastn, "-dust no --");
    }

    #[test]
    fn at_most_eight_pairs() {
        let eight = "-word_size 11 -gapopen 5 -gapextend 2 -penalty -3 -reward 2 -dust no -soft_masking false -lcase_masking";
        assert_eq!(ok(Blastn, eight).len(), 8);
        assert!(err(Blastn, &format!("{eight} -ungapped")).contains("at most 8"));
    }

    #[test]
    fn length_charset_and_shape() {
        assert!(ok(Blastn, "").is_empty());
        assert!(ok(Blastn, "   ").is_empty());
        assert_eq!(
            ok(Blastn, "  -dust no   -lcase_masking  "),
            vec![pair("dust", Some("no")), pair("lcase_masking", None)]
        );
        let long = format!("-word_size {}", "1".repeat(MAX_LEN));
        assert!(err(Blastn, &long).contains("at most 200"));
        assert!(err(Blastn, "-dust no;rm").contains("not allowed"));
        assert!(err(Blastn, "-out /tmp/x").contains("not allowed"));
        assert!(err(Blastn, "-dust\tno").contains("not allowed"));
        assert!(err(Blastn, "dust no").contains("expected a -flag"));
        assert!(err(Blastn, "-dust no extra").contains("expected a -flag"));
        assert!(err(Blastn, "-").contains("no flag name"));
        assert!(err(Blastn, "-=3").contains("no flag name"));
    }

    #[test]
    fn order_is_preserved() {
        assert_eq!(
            ok(Blastp, "-seg no -task blastp-fast -max_hsps 3"),
            vec![
                pair("seg", Some("no")),
                pair("task", Some("blastp-fast")),
                pair("max_hsps", Some("3"))
            ]
        );
    }
}
