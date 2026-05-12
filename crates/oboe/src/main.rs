//! oboe — adaptive DNS posture orchestrator.

mod cli;
mod effector;
mod probe;
mod state;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("oboe=info")),
        )
        .init();

    let args = cli::Cli::parse();
    cli::run(args).await
}
