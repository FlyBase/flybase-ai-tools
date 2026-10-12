//! Service configuration: one JSON file (design 3.2). Nothing site-specific is compiled in.
use crate::types::Program;
use anyhow::{Context, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// The MCP's hard limit on the `results.tsv` body (`Content-Length`), and so the ceiling for
/// `max_results_bytes`.
pub const MAX_RESULTS_CEILING: u64 = 16 * 1024 * 1024;

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub bind: SocketAddr,
    pub blast_bin_dir: PathBuf,
    pub database_root: PathBuf,
    pub catalogue: PathBuf,
    /// Returned verbatim in the `202` and shown to users.
    pub database_release: String,
    pub state_root: PathBuf,
    #[serde(default = "defaults::concurrency")]
    pub concurrency: usize,
    #[serde(default = "defaults::threads_per_job")]
    pub threads_per_job: u32,
    #[serde(default = "defaults::max_queued")]
    pub max_queued: usize,
    #[serde(default = "defaults::wall_seconds")]
    pub wall_seconds: u64,
    #[serde(default = "defaults::translated_wall_seconds")]
    pub translated_wall_seconds: u64,
    #[serde(default = "defaults::retention_seconds")]
    pub retention_seconds: u64,
    #[serde(default = "defaults::max_results_bytes")]
    pub max_results_bytes: u64,
    #[serde(default = "defaults::default_evalue")]
    pub default_evalue: f64,
    #[serde(default = "defaults::default_max_hits")]
    pub default_max_hits: u32,
    #[serde(default = "defaults::max_hsps_per_subject")]
    pub max_hsps_per_subject: u32,
    /// Residue cap per program; when the key is given it must name all five programs.
    #[serde(default = "defaults::max_residues")]
    pub max_residues: HashMap<Program, usize>,
}

mod defaults {
    use crate::types::Program;
    use std::collections::HashMap;

    pub fn concurrency() -> usize {
        2
    }
    pub fn threads_per_job() -> u32 {
        2
    }
    pub fn max_queued() -> usize {
        20
    }
    pub fn wall_seconds() -> u64 {
        600
    }
    pub fn translated_wall_seconds() -> u64 {
        300
    }
    pub fn retention_seconds() -> u64 {
        86_400
    }
    pub fn max_results_bytes() -> u64 {
        super::MAX_RESULTS_CEILING
    }
    pub fn default_evalue() -> f64 {
        10.0
    }
    pub fn default_max_hits() -> u32 {
        100
    }
    pub fn max_hsps_per_subject() -> u32 {
        50
    }
    pub fn max_residues() -> HashMap<Program, usize> {
        HashMap::from([
            (Program::Blastn, 100_000),
            (Program::Blastp, 100_000),
            (Program::Blastx, 30_000),
            (Program::Tblastn, 30_000),
            (Program::Tblastx, 10_000),
        ])
    }
}

impl Config {
    /// Read and validate the config file; any problem refuses to start the service.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Config> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = serde_json::from_str(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.make_paths_absolute()
            .with_context(|| format!("invalid config {}", path.display()))?;
        cfg.validate()
            .with_context(|| format!("invalid config {}", path.display()))?;
        Ok(cfg)
    }

    /// Relative paths are resolved against the process working directory (as `Catalogue::load` does
    /// for the database root), never the config file's directory. The runner starts BLAST+ with the
    /// job directory as its working directory, so a relative program or job path would otherwise
    /// resolve against that directory and every spawn would fail.
    fn make_paths_absolute(&mut self) -> anyhow::Result<()> {
        for (key, p) in [
            ("blast_bin_dir", &mut self.blast_bin_dir),
            ("database_root", &mut self.database_root),
            ("catalogue", &mut self.catalogue),
            ("state_root", &mut self.state_root),
        ] {
            *p = std::path::absolute(&*p).with_context(|| format!("{key} {}", p.display()))?;
        }
        Ok(())
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.database_release.trim().is_empty() {
            bail!("database_release must not be empty");
        }
        if self.concurrency < 1 {
            bail!("concurrency must be at least 1");
        }
        if self.threads_per_job < 1 {
            bail!("threads_per_job must be at least 1");
        }
        if self.max_queued < 1 {
            bail!("max_queued must be at least 1");
        }
        if self.wall_seconds < 1 || self.translated_wall_seconds < 1 {
            bail!("wall_seconds and translated_wall_seconds must be at least 1");
        }
        if self.retention_seconds < 1 {
            bail!("retention_seconds must be at least 1");
        }
        if self.max_results_bytes > MAX_RESULTS_CEILING {
            bail!(
                "max_results_bytes is {}; the MCP reads at most {MAX_RESULTS_CEILING} bytes",
                self.max_results_bytes
            );
        }
        // The served file always starts with the header line, so a smaller cap could not hold it.
        if self.max_results_bytes < 1024 {
            bail!("max_results_bytes must be at least 1024");
        }
        if !(self.default_evalue.is_finite()
            && self.default_evalue > 0.0
            && self.default_evalue <= 1000.0)
        {
            bail!("default_evalue must be in (0, 1000]");
        }
        if !(1..=1000).contains(&self.default_max_hits) {
            bail!("default_max_hits must be 1 to 1000");
        }
        if self.max_hsps_per_subject < 1 {
            bail!("max_hsps_per_subject must be at least 1");
        }
        for p in Program::ALL {
            match self.max_residues.get(&p) {
                None => bail!("max_residues has no cap for {p}"),
                Some(0) => bail!("max_residues for {p} must be at least 1"),
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// Residue cap for `program` (present for every program once `load` succeeded).
    pub fn residue_cap(&self, program: Program) -> usize {
        self.max_residues[&program]
    }

    /// Wall-clock limit for `program`: translated searches get `translated_wall_seconds`.
    pub fn wall_seconds_for(&self, program: Program) -> u64 {
        if program.translated() {
            self.translated_wall_seconds
        } else {
            self.wall_seconds
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};

    pub(crate) fn sample() -> Value {
        json!({
            "bind": "0.0.0.0:8800",
            "blast_bin_dir": "/opt/ncbi-blast/bin",
            "database_root": "/db",
            "catalogue": "/db/catalogue.json",
            "database_release": "FB2023_04",
            "state_root": "/state",
            "concurrency": 2,
            "threads_per_job": 2,
            "max_queued": 20,
            "wall_seconds": 600,
            "translated_wall_seconds": 300,
            "retention_seconds": 86400,
            "max_results_bytes": 16777216,
            "default_evalue": 10,
            "default_max_hits": 100,
            "max_hsps_per_subject": 50,
            "max_residues": { "blastn": 100000, "blastp": 100000, "blastx": 30000, "tblastn": 30000, "tblastx": 10000 }
        })
    }

    fn load(v: &Value) -> anyhow::Result<Config> {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), v.to_string()).unwrap();
        Config::load(f.path())
    }

    fn err(v: &Value) -> String {
        format!("{:#}", load(v).unwrap_err())
    }

    #[test]
    fn sample_loads() {
        let c = load(&sample()).unwrap();
        assert_eq!(c.database_release, "FB2023_04");
        assert_eq!(c.residue_cap(Program::Tblastx), 10_000);
        assert_eq!(c.wall_seconds_for(Program::Blastn), 600);
        assert_eq!(c.wall_seconds_for(Program::Tblastn), 300);
    }

    #[test]
    fn defaults_fill_omitted_numbers() {
        let mut v = sample();
        let o = v.as_object_mut().unwrap();
        for k in [
            "concurrency",
            "max_results_bytes",
            "max_residues",
            "default_evalue",
        ] {
            o.remove(k);
        }
        let c = load(&v).unwrap();
        assert_eq!(c.concurrency, 2);
        assert_eq!(c.max_results_bytes, MAX_RESULTS_CEILING);
        assert_eq!(c.residue_cap(Program::Blastx), 30_000);
        assert_eq!(c.default_evalue, 10.0);
    }

    #[test]
    fn results_cap_above_16_mib_is_refused() {
        let mut v = sample();
        v["max_results_bytes"] = json!(16_777_217);
        assert!(err(&v).contains("max_results_bytes"));
        v["max_results_bytes"] = json!(16_777_216);
        load(&v).unwrap();
    }

    #[test]
    fn zero_concurrency_is_refused() {
        let mut v = sample();
        v["concurrency"] = json!(0);
        assert!(err(&v).contains("concurrency"));
    }

    #[test]
    fn residue_caps_must_cover_all_five_programs() {
        let mut v = sample();
        v["max_residues"].as_object_mut().unwrap().remove("tblastx");
        assert!(err(&v).contains("no cap for tblastx"));
        let mut v = sample();
        v["max_residues"]["blastp"] = json!(0);
        assert!(err(&v).contains("blastp"));
        let mut v = sample();
        v["max_residues"]["psiblast"] = json!(10);
        assert!(load(&v).is_err(), "an unknown program key is refused");
    }

    #[test]
    fn unknown_key_and_bad_values_are_refused() {
        let mut v = sample();
        v["concurency"] = json!(4);
        assert!(err(&v).contains("concurency"));
        let mut v = sample();
        v["bind"] = json!("not-an-address");
        assert!(load(&v).is_err());
        let mut v = sample();
        v["database_release"] = json!(" ");
        assert!(err(&v).contains("database_release"));
        let mut v = sample();
        v["default_max_hits"] = json!(5000);
        assert!(err(&v).contains("default_max_hits"));
        let mut v = sample();
        v["default_evalue"] = json!(0);
        assert!(err(&v).contains("default_evalue"));
        let mut v = sample();
        v.as_object_mut().unwrap().remove("state_root");
        assert!(err(&v).contains("state_root"));
    }

    #[test]
    fn relative_paths_become_absolute_against_the_working_directory() {
        let mut v = sample();
        v["blast_bin_dir"] = json!("./blast/bin");
        v["state_root"] = json!("state");
        v["catalogue"] = json!("db/catalogue.json");
        v["database_root"] = json!("db");
        let c = load(&v).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(c.blast_bin_dir, cwd.join("blast/bin"));
        assert_eq!(c.state_root, cwd.join("state"));
        assert_eq!(c.catalogue, cwd.join("db/catalogue.json"));
        assert_eq!(c.database_root, cwd.join("db"));
        // Absolute paths are kept as written.
        let c = load(&sample()).unwrap();
        assert_eq!(c.blast_bin_dir, PathBuf::from("/opt/ncbi-blast/bin"));
        assert_eq!(c.state_root, PathBuf::from("/state"));
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(Config::load("/nonexistent/blast-jobs.json").is_err());
    }
}
