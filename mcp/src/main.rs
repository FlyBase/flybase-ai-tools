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
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cfg = flybase_mcp::config::Config::from_env()?;
    match Cli::parse().cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => flybase_mcp::server::run(cfg).await,
        Cmd::Migrate { diopt } => flybase_mcp::migrate::run(&cfg, diopt).await,
    }
}
