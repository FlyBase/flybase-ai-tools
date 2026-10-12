use rmcp::{
    RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService,
    transport::StreamableHttpClientTransport,
};
use serde_json::Value;
use std::sync::OnceLock;

static BASE: OnceLock<String> = OnceLock::new();
static DB_READY: OnceLock<()> = OnceLock::new();

/// Apply migrations at most once per test binary: when the startup check fails or FLYBASE_TEST_MIGRATE=1.
/// Rebuilding the derived tables takes about a minute and takes exclusive locks, so servers never migrate
/// on their own; every `start` goes through here first.
pub fn ensure_db() {
    // On a separate thread: callers may be inside a #[tokio::test] runtime, where building and blocking on a
    // second runtime panics.
    DB_READY.get_or_init(|| {
        std::thread::spawn(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let cfg = super::test_config();
                let chado = flybase_mcp::backends::chado::Chado::connect(&cfg)
                    .await
                    .unwrap();
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
            });
        })
        .join()
        .expect("database check thread");
    });
}

/// Start a server for `cfg` on its own thread and runtime; returns its base URL. Never migrates.
pub fn start(cfg: flybase_mcp::config::Config) -> String {
    // SAFETY: Rust's env lock serialises set_var/var; other test threads may run but only read via Config::from_lookup.
    unsafe { std::env::set_var("FLYBASE_MCP_TEST_SECTIONS", "1") };
    ensure_db();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let chado = flybase_mcp::backends::chado::Chado::connect(&cfg)
                .await
                .unwrap();
            let app = flybase_mcp::server::app(&cfg, chado);
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(format!("http://{}", l.local_addr().unwrap()))
                .unwrap();
            axum::serve(l, app).await.unwrap();
        });
    });
    rx.recv().unwrap()
}

/// One default server for the whole test binary, on its own thread and runtime so it outlives individual
/// #[tokio::test] runtimes.
pub fn base() -> &'static str {
    BASE.get_or_init(|| start(super::test_config()))
}

pub async fn client_at(base: &str) -> RunningService<RoleClient, ()> {
    ().serve(StreamableHttpClientTransport::from_uri(format!(
        "{base}/mcp"
    )))
    .await
    .unwrap()
}

pub async fn client() -> RunningService<RoleClient, ()> {
    client_at(base()).await
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
