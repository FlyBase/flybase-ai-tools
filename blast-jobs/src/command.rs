//! The BLAST+ command line for one job (design 3.5). Pure: no IO, no environment.
//!
//! The argv is built from an ordered map `flag -> value` seeded with the service defaults and then
//! updated by the validated `advanced` pairs: a pair whose flag is already in the map replaces the value
//! in place (so `-dust no` replaces the default `-dust yes` and never adds a second `-dust`), any other
//! pair is appended in the order given. A Switch is rendered as one element. Nothing goes through a
//! shell: the `-db` list of prefixes is one argv element with spaces, as BLAST+ expects.
use crate::advanced;
use crate::catalogue::Resolved;
use crate::config::Config;
use crate::types::Program;
use crate::validate::Validated;
use std::ffi::OsString;
use std::path::Path;

/// The query file the store writes into each job directory.
pub const QUERY_FILE: &str = "query.fa";
/// The raw BLAST+ output, replaced by `results.tsv` once the search ends.
pub const RAW_FILE: &str = "raw.tsv";
/// Tabular output with the subject title appended; `runner::HEADER` names these columns.
pub const OUTFMT: &str = "6 std stitle";

/// Flags whose value only the service sets: paths, output format, resources and the request-field
/// flags. An `advanced` pair with one of these names is never rendered, even if it reached `build`
/// some way other than `advanced::parse` (which already rejects them).
pub const SERVICE_OWNED: &[&str] = &[
    "db",
    "query",
    "out",
    "outfmt",
    "evalue",
    "max_target_seqs",
    "num_threads",
    "matrix",
    "query_gencode",
    "db_gencode",
];

/// Whether an `advanced` pair may reach the argv: not service-owned, and in `advanced::TABLE` for
/// `program`. Defence in depth behind `advanced::parse`.
fn allowed(program: Program, flag: &str) -> bool {
    !SERVICE_OWNED.contains(&flag)
        && advanced::TABLE
            .iter()
            .any(|spec| spec.name == flag && spec.programs.contains(&program))
}

/// An ordered `flag -> value` map; `None` is a Switch.
#[derive(Default)]
struct Argv(Vec<(String, Option<OsString>)>);

impl Argv {
    /// Replace the value of `flag` where it already stands, else append it.
    fn set(&mut self, flag: &str, value: Option<OsString>) {
        match self.0.iter_mut().find(|(f, _)| f == flag) {
            Some(slot) => slot.1 = value,
            None => self.0.push((flag.to_string(), value)),
        }
    }

    fn set_str(&mut self, flag: &str, value: impl Into<OsString>) {
        self.set(flag, Some(value.into()));
    }
}

/// `evalue` as BLAST+ reads it: plain decimal for ordinary values, exponent form for tiny ones so the
/// argument stays short (`1e-300`, not 300 zeros).
fn format_evalue(e: f64) -> String {
    if e < 1e-4 {
        format!("{e:e}")
    } else {
        format!("{e}")
    }
}

/// The full argv for `req`, element 0 being the program binary under `cfg.blast_bin_dir`:
///
/// ```text
/// <bin>/<program> -db "<prefix> [<prefix> ...]" -query <job>/query.fa -out <job>/raw.tsv
///   -outfmt "6 std stitle" -evalue <e> -max_target_seqs <max_hits> -max_hsps <cap or override>
///   -num_threads <threads_per_job> [-task <t> (always for blastn)] -dust yes | -seg yes
///   [-matrix <m>] [-query_gencode <g> (blastx, tblastx)] [-db_gencode <g> (tblastn, tblastx)]
///   [remaining advanced flags]
/// ```
///
/// `-task` is explicit for blastn (BLAST+'s own default there is megablast): `megablast` when the
/// request says so, else `blastn`, unless `advanced` sets it. Other programs get `-task` only from
/// `advanced`. An `advanced` `-max_hsps` is clamped to `max_hsps_per_subject`. A pair that is
/// service-owned or not in the allow-list for the program is dropped (and logged), never rendered.
///
/// Disk use before truncation is bounded only by this argv: at most 1000 subjects
/// (`-max_target_seqs`) x `max_hsps_per_subject` HSPs (50) rows. Recheck the state volume before raising
/// either cap.
pub fn build(cfg: &Config, req: &Validated, resolved: &Resolved, job_dir: &Path) -> Vec<OsString> {
    let program = req.program;
    let mut a = Argv::default();

    let mut db = OsString::new();
    for (i, prefix) in resolved.prefixes.iter().enumerate() {
        if i > 0 {
            db.push(" ");
        }
        db.push(prefix.as_os_str());
    }
    a.set("db", Some(db));
    a.set_str("query", job_dir.join(QUERY_FILE));
    a.set_str("out", job_dir.join(RAW_FILE));
    a.set_str("outfmt", OUTFMT);
    a.set_str("evalue", format_evalue(req.evalue));
    a.set_str("max_target_seqs", req.max_hits.to_string());
    a.set_str("max_hsps", cfg.max_hsps_per_subject.to_string());
    a.set_str("num_threads", cfg.threads_per_job.to_string());
    if program == Program::Blastn {
        a.set_str("task", if req.megablast { "megablast" } else { "blastn" });
        a.set_str("dust", "yes");
    } else {
        a.set_str("seg", "yes");
    }
    if let Some(m) = &req.matrix {
        a.set_str("matrix", m.as_str());
    }
    if let Some(g) = req.genetic_code {
        if matches!(program, Program::Blastx | Program::Tblastx) {
            a.set_str("query_gencode", g.to_string());
        }
        if matches!(program, Program::Tblastn | Program::Tblastx) {
            a.set_str("db_gencode", g.to_string());
        }
    }

    for (flag, value) in &req.advanced {
        if !allowed(program, flag) {
            tracing::error!(%program, flag = %flag, "advanced flag not allowed here; dropped from argv");
            continue;
        }
        let value = match (flag.as_str(), value) {
            // Validated as an integer >= 1; anything that does not fit a u64 is above the cap anyway.
            ("max_hsps", Some(v)) => Some(
                v.parse::<u64>()
                    .map_or(cfg.max_hsps_per_subject.into(), |n| {
                        n.min(cfg.max_hsps_per_subject.into())
                    })
                    .to_string()
                    .into(),
            ),
            (_, v) => v.as_ref().map(OsString::from),
        };
        a.set(flag, value);
    }

    let mut argv = Vec::with_capacity(1 + 2 * a.0.len());
    argv.push(cfg.blast_bin_dir.join(program.as_str()).into_os_string());
    for (flag, value) in a.0 {
        argv.push(OsString::from(format!("-{flag}")));
        if let Some(v) = value {
            argv.push(v);
        }
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advanced::{Kind, TABLE};
    use crate::catalogue::Catalogue;
    use crate::types::SubmitRequest;
    use crate::validate;
    use std::path::PathBuf;

    const JOB: &str = "/state/jobs/AAAAAAAAAAAAAAAAAAAAAA";

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture")
    }

    fn cfg() -> Config {
        let mut v = crate::config::tests::sample();
        v["blast_bin_dir"] = serde_json::json!("/opt/ncbi-blast/bin");
        v["threads_per_job"] = serde_json::json!(3);
        serde_json::from_value(v).unwrap()
    }

    fn catalogue() -> Catalogue {
        let f = fixture();
        Catalogue::load(f.join("catalogue.json"), f.join("db")).unwrap()
    }

    fn db_for(p: Program) -> &'static str {
        if p.protein_database() {
            "translation"
        } else {
            "assembly"
        }
    }

    fn seq_for(p: Program) -> &'static str {
        if p.protein_query() {
            ">q\nMKLVQ\n"
        } else {
            ">q\nACGTACGT\n"
        }
    }

    fn request(p: Program) -> SubmitRequest {
        SubmitRequest {
            program: p.as_str().into(),
            database: db_for(p).into(),
            species: vec!["dmel".into()],
            sequence: seq_for(p).into(),
            ..Default::default()
        }
    }

    fn argv_for(req: SubmitRequest) -> Vec<String> {
        let c = cfg();
        let v = validate::check(&req, &c, &catalogue()).expect("request should validate");
        build(&c, &v, &v.resolved, Path::new(JOB))
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect()
    }

    fn with_advanced(p: Program, adv: &str) -> Vec<String> {
        argv_for(SubmitRequest {
            advanced: Some(adv.into()),
            ..request(p)
        })
    }

    fn prefix(category: &str, species: &str) -> String {
        let cat = catalogue();
        cat.resolve(&[species.to_string()], category)
            .unwrap()
            .prefixes[0]
            .display()
            .to_string()
    }

    /// The value after `flag`, asserting the flag appears exactly once.
    fn value_of<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
        let hits: Vec<usize> = (0..argv.len()).filter(|&i| argv[i] == flag).collect();
        assert_eq!(hits.len(), 1, "{flag} must appear exactly once in {argv:?}");
        argv.get(hits[0] + 1).map(String::as_str)
    }

    fn count(argv: &[String], flag: &str) -> usize {
        argv.iter().filter(|a| *a == flag).count()
    }

    fn expected_defaults(p: Program, extra: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = vec![
            format!("/opt/ncbi-blast/bin/{p}"),
            "-db".into(),
            prefix(db_for(p), "dmel"),
            "-query".into(),
            format!("{JOB}/query.fa"),
            "-out".into(),
            format!("{JOB}/raw.tsv"),
            "-outfmt".into(),
            "6 std stitle".into(),
            "-evalue".into(),
            "10".into(),
            "-max_target_seqs".into(),
            "100".into(),
            "-max_hsps".into(),
            "50".into(),
            "-num_threads".into(),
            "3".into(),
        ];
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    }

    #[test]
    fn exact_default_argv_per_program() {
        let cases: [(Program, &[&str]); 5] = [
            (Program::Blastn, &["-task", "blastn", "-dust", "yes"]),
            (Program::Blastp, &["-seg", "yes"]),
            (Program::Blastx, &["-seg", "yes"]),
            (Program::Tblastn, &["-seg", "yes"]),
            (Program::Tblastx, &["-seg", "yes"]),
        ];
        for (p, extra) in cases {
            assert_eq!(argv_for(request(p)), expected_defaults(p, extra), "{p}");
        }
    }

    #[test]
    fn request_fields_land_in_argv() {
        let argv = argv_for(SubmitRequest {
            evalue: Some(0.001),
            max_hits: Some(250),
            matrix: Some("pam30".into()),
            genetic_code: Some(5),
            ..request(Program::Tblastx)
        });
        assert_eq!(value_of(&argv, "-evalue"), Some("0.001"));
        assert_eq!(value_of(&argv, "-max_target_seqs"), Some("250"));
        assert_eq!(value_of(&argv, "-matrix"), Some("PAM30"));
        assert_eq!(value_of(&argv, "-query_gencode"), Some("5"));
        assert_eq!(value_of(&argv, "-db_gencode"), Some("5"));
        let tiny = argv_for(SubmitRequest {
            evalue: Some(1e-300),
            ..request(Program::Blastn)
        });
        assert_eq!(value_of(&tiny, "-evalue"), Some("1e-300"));
    }

    #[test]
    fn gencode_placement_per_program() {
        let gc = |p: Program| {
            argv_for(SubmitRequest {
                genetic_code: Some(11),
                ..request(p)
            })
        };
        let bx = gc(Program::Blastx);
        assert_eq!(value_of(&bx, "-query_gencode"), Some("11"));
        assert_eq!(count(&bx, "-db_gencode"), 0);
        let tn = gc(Program::Tblastn);
        assert_eq!(value_of(&tn, "-db_gencode"), Some("11"));
        assert_eq!(count(&tn, "-query_gencode"), 0);
        let tx = gc(Program::Tblastx);
        assert_eq!(value_of(&tx, "-query_gencode"), Some("11"));
        assert_eq!(value_of(&tx, "-db_gencode"), Some("11"));
        // Non-translated programs never get a genetic code (validate drops it).
        for p in [Program::Blastn, Program::Blastp] {
            let a = gc(p);
            assert_eq!(count(&a, "-query_gencode") + count(&a, "-db_gencode"), 0);
        }
    }

    #[test]
    fn two_species_share_one_db_element() {
        let argv = argv_for(SubmitRequest {
            species: vec!["dmel".into(), "dsim".into()],
            ..request(Program::Blastn)
        });
        let db = value_of(&argv, "-db").unwrap();
        assert_eq!(
            db,
            format!(
                "{} {}",
                prefix("assembly", "dmel"),
                prefix("assembly", "dsim")
            )
        );
        assert!(db.starts_with('/'), "prefixes are absolute");
    }

    #[test]
    fn blastn_task_both_ways_and_overrides() {
        let plain = argv_for(request(Program::Blastn));
        assert_eq!(value_of(&plain, "-task"), Some("blastn"));
        let mega = argv_for(SubmitRequest {
            megablast: Some(true),
            ..request(Program::Blastn)
        });
        assert_eq!(value_of(&mega, "-task"), Some("megablast"));
        let short = with_advanced(Program::Blastn, "-task blastn-short");
        assert_eq!(value_of(&short, "-task"), Some("blastn-short"));
        // Overridden in place: same position as the default.
        assert_eq!(
            short.iter().position(|a| a == "-task"),
            plain.iter().position(|a| a == "-task")
        );
        // Allowed-and-overridable (3.4): `-task megablast` with megablast false.
        let adv_mega = argv_for(SubmitRequest {
            megablast: Some(false),
            advanced: Some("-task megablast".into()),
            ..request(Program::Blastn)
        });
        assert_eq!(value_of(&adv_mega, "-task"), Some("megablast"));
        let mega_mega = argv_for(SubmitRequest {
            megablast: Some(true),
            advanced: Some("-task=megablast".into()),
            ..request(Program::Blastn)
        });
        assert_eq!(value_of(&mega_mega, "-task"), Some("megablast"));
    }

    #[test]
    fn other_programs_get_task_only_from_advanced() {
        for p in [
            Program::Blastp,
            Program::Blastx,
            Program::Tblastn,
            Program::Tblastx,
        ] {
            assert_eq!(count(&argv_for(request(p)), "-task"), 0, "{p}");
        }
        assert_eq!(
            value_of(
                &with_advanced(Program::Blastp, "-task blastp-short"),
                "-task"
            ),
            Some("blastp-short")
        );
        assert_eq!(
            value_of(
                &with_advanced(Program::Tblastn, "-task tblastn-fast"),
                "-task"
            ),
            Some("tblastn-fast")
        );
    }

    #[test]
    fn max_hsps_override_and_clamp() {
        let low = with_advanced(Program::Blastn, "-max_hsps 7");
        assert_eq!(value_of(&low, "-max_hsps"), Some("7"));
        let at = with_advanced(Program::Blastp, "-max_hsps=50");
        assert_eq!(value_of(&at, "-max_hsps"), Some("50"));
        let high = with_advanced(Program::Blastn, "-max_hsps 500");
        assert_eq!(value_of(&high, "-max_hsps"), Some("50"));
        let huge = with_advanced(Program::Tblastx, "-max_hsps 9223372036854775807");
        assert_eq!(value_of(&huge, "-max_hsps"), Some("50"));
        // In place, not appended.
        let plain = argv_for(request(Program::Blastn));
        assert_eq!(
            high.iter().position(|a| a == "-max_hsps"),
            plain.iter().position(|a| a == "-max_hsps")
        );
    }

    #[test]
    fn masking_defaults_and_their_override() {
        let n = argv_for(request(Program::Blastn));
        assert_eq!(value_of(&n, "-dust"), Some("yes"));
        assert_eq!(count(&n, "-seg"), 0);
        for p in [
            Program::Blastp,
            Program::Blastx,
            Program::Tblastn,
            Program::Tblastx,
        ] {
            let a = argv_for(request(p));
            assert_eq!(value_of(&a, "-seg"), Some("yes"), "{p}");
            assert_eq!(count(&a, "-dust"), 0, "{p}");
            let off = with_advanced(p, "-seg no");
            assert_eq!(value_of(&off, "-seg"), Some("no"), "{p}");
            assert!(!off.iter().any(|x| x == "yes"), "{p}: no stray yes");
            assert_eq!(off.len(), a.len(), "{p}: replaced, not appended");
        }
        let off = with_advanced(Program::Blastn, "-dust=no");
        assert_eq!(value_of(&off, "-dust"), Some("no"));
        assert_eq!(off.len(), n.len());
    }

    #[test]
    fn switches_are_one_element() {
        let a = with_advanced(Program::Blastn, "-lcase_masking -ungapped -penalty -3");
        let i = a.iter().position(|x| x == "-lcase_masking").unwrap();
        assert_eq!(a[i + 1], "-ungapped");
        assert_eq!(a[i + 2], "-penalty");
        assert_eq!(a[i + 3], "-3", "a negative numeric value stays one element");
        assert_eq!(a.len(), i + 4);
        assert_eq!(count(&a, "-lcase_masking"), 1);
    }

    #[test]
    fn advanced_appends_in_given_order_after_defaults() {
        let base = argv_for(request(Program::Blastn));
        let a = with_advanced(Program::Blastn, "-word_size 11 -reward 2 -strand=minus");
        assert_eq!(&a[..base.len()], &base[..]);
        assert_eq!(
            &a[base.len()..],
            &["-word_size", "11", "-reward", "2", "-strand", "minus"]
        );
    }

    /// A valid value for each kind, chosen from the table's own bounds.
    fn sample_value(kind: Kind) -> Option<String> {
        match kind {
            Kind::Int { min, max } => Some(match (min, max) {
                (Some(m), _) => (m + 1).to_string(),
                (None, Some(m)) => (m - 2).to_string(),
                (None, None) => "1".into(),
            }),
            Kind::Float { min, .. } => Some(format!("{}", min + 1.5)),
            Kind::Bool { no, .. } => Some(no.to_string()),
            Kind::Switch => None,
            Kind::Enum(values) => Some(values[values.len() - 1].to_string()),
        }
    }

    /// Every row of the allow-list, on every program it applies to, in both `-f v` and `-f=v` forms:
    /// the flag lands exactly once, with its value as the next element (a Switch alone).
    #[test]
    fn every_allowed_flag_lands_once_in_the_right_form() {
        let mut checked = 0;
        for spec in TABLE {
            for &p in spec.programs {
                let value = sample_value(spec.kind);
                let forms = match &value {
                    Some(v) => vec![format!("-{} {v}", spec.name), format!("-{}={v}", spec.name)],
                    None => vec![format!("-{}", spec.name)],
                };
                for adv in forms {
                    let argv = with_advanced(p, &adv);
                    let flag = format!("-{}", spec.name);
                    let got = value_of(&argv, &flag);
                    match &value {
                        None => assert!(
                            got.is_none_or(|next| next.starts_with('-')),
                            "{p} {adv}: a switch takes no value element, got {argv:?}"
                        ),
                        Some(v) if spec.name == "max_hsps" => {
                            let want = v.parse::<u32>().unwrap().min(50).to_string();
                            assert_eq!(got, Some(want.as_str()), "{p} {adv}")
                        }
                        Some(v) => assert_eq!(got, Some(v.as_str()), "{p} {adv}"),
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 60, "table sweep checked only {checked} cases");
    }

    #[test]
    fn hand_written_rows_per_flag() {
        use Program::*;
        let rows: &[(Program, &str, &str, Option<&str>)] = &[
            (Blastn, "-word_size 7", "-word_size", Some("7")),
            (Blastp, "-word_size 3", "-word_size", Some("3")),
            (Blastx, "-gapopen 11", "-gapopen", Some("11")),
            (Tblastn, "-gapextend 1", "-gapextend", Some("1")),
            (Blastn, "-penalty -3", "-penalty", Some("-3")),
            (Blastn, "-penalty=0", "-penalty", Some("0")),
            (Blastn, "-reward 1", "-reward", Some("1")),
            (Blastn, "-dust no", "-dust", Some("no")),
            (Tblastx, "-seg no", "-seg", Some("no")),
            (
                Blastp,
                "-soft_masking false",
                "-soft_masking",
                Some("false"),
            ),
            (Tblastx, "-lcase_masking", "-lcase_masking", None),
            (Blastx, "-ungapped", "-ungapped", None),
            (Tblastx, "-strand plus", "-strand", Some("plus")),
            (
                Blastn,
                "-perc_identity 97.5",
                "-perc_identity",
                Some("97.5"),
            ),
            (Blastp, "-qcov_hsp_perc 80", "-qcov_hsp_perc", Some("80")),
            (Tblastn, "-culling_limit 0", "-culling_limit", Some("0")),
            (Blastx, "-threshold 11", "-threshold", Some("11")),
            (
                Blastp,
                "-comp_based_stats F",
                "-comp_based_stats",
                Some("F"),
            ),
            (Tblastx, "-window_size 40", "-window_size", Some("40")),
            (Blastx, "-task blastx-fast", "-task", Some("blastx-fast")),
        ];
        for &(p, adv, flag, want) in rows {
            let argv = with_advanced(p, adv);
            let got = value_of(&argv, flag);
            match want {
                Some(w) => assert_eq!(got, Some(w), "{p} {adv}"),
                None => assert!(got.is_none_or(|n| n.starts_with('-')), "{p} {adv}"),
            }
        }
    }

    /// A `Validated` built by hand (bypassing `advanced::parse`) cannot move the service's paths,
    /// output format or resources, nor add a flag outside the allow-list.
    #[test]
    fn service_owned_and_unlisted_pairs_never_reach_argv() {
        let c = cfg();
        let mut v = validate::check(&request(Program::Blastn), &c, &catalogue()).unwrap();
        let clean = build(&c, &v, &v.resolved, Path::new(JOB));
        let pair = |f: &str, x: Option<&str>| (f.to_string(), x.map(String::from));
        v.advanced = vec![
            pair("db", Some("/etc/passwd")),
            pair("query", Some("/etc/shadow")),
            pair("out", Some("/tmp/evil")),
            pair("outfmt", Some("5")),
            pair("num_threads", Some("64")),
            pair("evalue", Some("1000")),
            pair("remote", None),
            pair("seg", Some("no")), // seg is not a blastn flag
            pair("dust", Some("no")),
        ];
        let argv: Vec<String> = build(&c, &v, &v.resolved, Path::new(JOB))
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect();
        assert_eq!(
            value_of(&argv, "-db"),
            Some(prefix("assembly", "dmel").as_str())
        );
        assert_eq!(
            value_of(&argv, "-query"),
            Some(format!("{JOB}/query.fa").as_str())
        );
        assert_eq!(
            value_of(&argv, "-out"),
            Some(format!("{JOB}/raw.tsv").as_str())
        );
        assert_eq!(value_of(&argv, "-outfmt"), Some("6 std stitle"));
        assert_eq!(value_of(&argv, "-num_threads"), Some("3"));
        assert_eq!(value_of(&argv, "-evalue"), Some("10"));
        assert_eq!(count(&argv, "-remote") + count(&argv, "-seg"), 0);
        // The one allowed pair still applies, in place.
        assert_eq!(value_of(&argv, "-dust"), Some("no"));
        assert_eq!(argv.len(), clean.len());
        for f in SERVICE_OWNED {
            assert!(
                !advanced::TABLE.iter().any(|s| s.name == *f),
                "{f} is service-owned and must not be in the allow-list"
            );
        }
    }

    #[test]
    fn program_binary_comes_from_config() {
        let mut c = cfg();
        c.blast_bin_dir = "/somewhere/else".into();
        let v = validate::check(&request(Program::Tblastn), &c, &catalogue()).unwrap();
        let argv = build(&c, &v, &v.resolved, Path::new(JOB));
        assert_eq!(argv[0], OsString::from("/somewhere/else/tblastn"));
    }
}
