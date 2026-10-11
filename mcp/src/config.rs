use anyhow::Context;
use std::net::SocketAddr;
use url::Url;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub public_url: Url,
    pub release: String,
    pub chado_url: String,
    pub pool_size: usize,
    pub blast_base_url: Option<Url>,
    pub blast_token: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let req = |k: &str| get(k).with_context(|| format!("missing required env var {k}"));
        Ok(Self {
            bind: req("FLYBASE_MCP_BIND")?
                .parse()
                .context("FLYBASE_MCP_BIND")?,
            public_url: req("FLYBASE_MCP_PUBLIC_URL")?
                .parse()
                .context("FLYBASE_MCP_PUBLIC_URL")?,
            release: req("FLYBASE_RELEASE")?,
            chado_url: req("CHADO_URL")?,
            pool_size: get("CHADO_POOL_SIZE")
                .map(|v| v.parse())
                .transpose()
                .context("CHADO_POOL_SIZE")?
                .unwrap_or(32),
            blast_base_url: get("BLAST_BASE_URL")
                .map(|v| v.parse())
                .transpose()
                .context("BLAST_BASE_URL")?,
            blast_token: get("BLAST_SERVICE_TOKEN"),
        })
    }

    /// Hostnames accepted by the Streamable HTTP host check. rmcp matches a bare hostname on any port.
    pub fn allowed_hosts(&self) -> Vec<String> {
        let host = self
            .public_url
            .host_str()
            .unwrap_or("localhost")
            .to_string();
        vec![host, "127.0.0.1".into(), "localhost".into()]
    }

    pub fn allowed_origins(&self) -> Vec<String> {
        vec![self.public_url.origin().ascii_serialization()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn env(k: &str) -> Option<String> {
        match k {
            "FLYBASE_MCP_BIND" => Some("127.0.0.1:7200".into()),
            "FLYBASE_MCP_PUBLIC_URL" => Some("https://mcp.flybase.org".into()),
            "FLYBASE_RELEASE" => Some("FB2026_05".into()),
            "CHADO_URL" => Some("postgres://localhost:5432/flybase".into()),
            _ => None,
        }
    }
    #[test]
    fn defaults_and_hosts() {
        let c = Config::from_lookup(env).unwrap();
        assert_eq!(c.pool_size, 32);
        assert!(c.allowed_hosts().contains(&"mcp.flybase.org".to_string()));
        assert_eq!(c.allowed_origins(), vec!["https://mcp.flybase.org"]);
    }
    #[test]
    fn missing_release_is_error() {
        let e = Config::from_lookup(|k| if k == "FLYBASE_RELEASE" { None } else { env(k) })
            .unwrap_err();
        assert!(e.to_string().contains("FLYBASE_RELEASE"));
    }
}
