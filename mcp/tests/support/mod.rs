#![allow(dead_code)]
pub mod server;
use flybase_mcp::config::Config;

pub fn test_config() -> Config {
    Config::from_lookup(|k| match k {
        "FLYBASE_MCP_BIND" => Some("127.0.0.1:0".into()),
        "FLYBASE_MCP_PUBLIC_URL" => Some("http://127.0.0.1".into()),
        "FLYBASE_RELEASE" => Some("FB_TEST".into()),
        "CHADO_URL" => Some(std::env::var("CHADO_URL").unwrap_or_else(|_| {
            "postgres://flybase_mcp:flybase_mcp@127.0.0.1:5498/flybase".into()
        })),
        "CHADO_POOL_SIZE" => Some("4".into()),
        "FLYBASE_MCP_RESPONSE_CACHE_MB" => std::env::var(k).ok(),
        // The wiki stays unconfigured (as does BLAST); wiki tests set `wiki_base_url` to their own mock.
        "WIKI_BASE_URL" | "WIKI_HOST_HEADER" => None,
        _ => None,
    })
    .unwrap()
}

/// True when the tests run against the full chado-mcp-dev restore (port 5498) rather than the CI fixture.
pub fn full_db() -> bool {
    std::env::var("FLYBASE_TEST_FULL_DB").is_ok_and(|v| v == "1")
        || test_config().chado_url.contains(":5498/")
}
