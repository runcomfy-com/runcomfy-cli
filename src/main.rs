use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

mod api;
mod cli;
mod cmd;
mod config;
mod error;

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let args = cli::Cli::parse();
    cli::dispatch(args).await
}
