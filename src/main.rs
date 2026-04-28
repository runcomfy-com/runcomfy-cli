use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

mod api;
mod cli;
mod cmd;
mod config;
mod error;
mod exit;
mod output;

#[tokio::main]
async fn main() -> ExitCode {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let args = cli::Cli::parse();
    output::init(args.global.output, args.global.verbose, args.global.quiet);

    match cli::dispatch(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Single-line error in JSON mode (machine-parsable),
            // chained pretty error otherwise.
            if output::is_json() {
                let payload = serde_json::json!({
                    "error": format!("{}", e),
                });
                eprintln!("{}", serde_json::to_string(&payload).unwrap_or_default());
            } else {
                eprintln!("Error: {:#}", e);
            }
            ExitCode::from(exit::classify(&e) as u8)
        }
    }
}
