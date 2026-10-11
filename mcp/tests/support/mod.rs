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
        _ => None,
    })
    .unwrap()
}
