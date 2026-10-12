use rmcp::{
    RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService,
    transport::StreamableHttpClientTransport,
};
use serde_json::Value;
use std::sync::OnceLock;

static BASE: OnceLock<String> = OnceLock::new();

/// One server for the whole test binary, on its own thread and runtime so it outlives individual #[tokio::test] runtimes.
pub fn base() -> &'static str {
    BASE.get_or_init(|| {
        // SAFETY: Rust's env lock serialises set_var/var; other test threads may run but only read via Config::from_lookup.
        unsafe { std::env::set_var("FLYBASE_MCP_TEST_SECTIONS", "1") };
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let cfg = super::test_config();
                let chado = flybase_mcp::backends::chado::Chado::connect(&cfg)
                    .await
                    .unwrap();
                // Rebuilding the derived tables takes about a minute and takes exclusive locks,
                // so only migrate when the database is not ready or FLYBASE_TEST_MIGRATE=1 asks for it.
                let force = std::env::var("FLYBASE_TEST_MIGRATE").is_ok_and(|v| v == "1");
                if force
                    || chado
                        .startup_check(&flybase_mcp::migrate::names())
                        .await
                        .is_err()
                {
                    flybase_mcp::migrate::run(&cfg, None)
                        .await
                        .expect("migrations apply to the fixture");
                }
                let app = flybase_mcp::server::app(&cfg, chado);
                let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(format!("http://{}", l.local_addr().unwrap()))
                    .unwrap();
                axum::serve(l, app).await.unwrap();
            });
        });
        rx.recv().unwrap()
    })
}

pub async fn client() -> RunningService<RoleClient, ()> {
    ().serve(StreamableHttpClientTransport::from_uri(format!(
        "{}/mcp",
        base()
    )))
    .await
    .unwrap()
}

/// Call a tool and return its structured content; panics if the tool reported an error.
pub async fn call(c: &RunningService<RoleClient, ()>, tool: &str, args: Value) -> Value {
    let r = c
        .call_tool(
            CallToolRequestParams::new(tool.to_string())
                .with_arguments(args.as_object().cloned().unwrap_or_default()),
        )
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true), "tool error: {:?}", r.content);
    r.structured_content.unwrap()
}

/// Call a tool and return the raw result (for asserting on errors).
pub async fn call_raw(
    c: &RunningService<RoleClient, ()>,
    tool: &str,
    args: Value,
) -> Result<rmcp::model::CallToolResult, rmcp::ServiceError> {
    c.call_tool(
        CallToolRequestParams::new(tool.to_string())
            .with_arguments(args.as_object().cloned().unwrap_or_default()),
    )
    .await
}
