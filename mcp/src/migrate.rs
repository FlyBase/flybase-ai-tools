//! Migration runner. Stub until Task 5 adds the `mcp` schema migrations.
use crate::config::Config;

pub const MIGRATIONS: &[(&str, &str)] = &[];

pub fn names() -> Vec<&'static str> {
    MIGRATIONS.iter().map(|(n, _)| *n).collect()
}

pub async fn run(_cfg: &Config, _diopt: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    Ok(())
}
