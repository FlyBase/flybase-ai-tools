use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "flybase-mcp", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve the MCP endpoint (default).
    Serve,
    /// Apply migrations to the chado-mcp database (owner role).
    Migrate {
        #[arg(long)]
        diopt: Option<std::path::PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let cli = Cli::parse();
    let cfg = flybase_mcp::config::Config::from_env()?;
    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => flybase_mcp::server::run(cfg).await,
        Cmd::Migrate { diopt } => flybase_mcp::migrate::run(&cfg, diopt).await,
    }
}
