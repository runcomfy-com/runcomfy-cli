use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::cmd;

#[derive(Parser, Debug)]
#[command(
    name = "runcomfy",
    version,
    about = "RunComfy CLI — run AI media models on RunComfy",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Authenticate with RunComfy (opens browser, device-code flow)
    Login {
        /// Override the web auth base URL
        #[arg(long, env = "RUNCOMFY_WEB_BASE")]
        web_base: Option<String>,
    },

    /// Log out and remove the local token
    Logout,

    /// Show the currently authenticated user
    Whoami,

    /// Run a Model API model on RunComfy
    ///
    /// `model_id` is a slash-separated identifier from the RunComfy
    /// Models catalog, e.g. `blackforestlabs/flux-1-kontext/pro/edit`.
    /// The CLI submits the request, polls until terminal, and prints
    /// the result URLs.
    Run {
        /// model_id (e.g. blackforestlabs/flux-1-kontext/pro/edit)
        model_id: String,

        /// JSON payload matching the model's Input schema
        #[arg(long, value_name = "JSON")]
        input: Option<String>,

        /// Read JSON input from a file
        #[arg(long, value_name = "PATH", conflicts_with = "input")]
        input_file: Option<String>,

        /// Submit and return immediately without waiting for completion
        #[arg(long)]
        no_wait: bool,

        /// Polling interval seconds while waiting (default 2)
        #[arg(long, default_value_t = 2)]
        poll_secs: u64,
    },

    /// Poll the status of a Model API request
    Status {
        /// request_id returned by `runcomfy run`
        request_id: String,
    },

    /// Cancel a queued Model API request
    Cancel {
        /// request_id returned by `runcomfy run`
        request_id: String,
    },
}

pub async fn dispatch(args: Cli) -> Result<()> {
    match args.command {
        Command::Login { web_base } => cmd::login::run(web_base).await,
        Command::Logout => cmd::login::logout().await,
        Command::Whoami => cmd::whoami::run().await,
        Command::Run {
            model_id,
            input,
            input_file,
            no_wait,
            poll_secs,
        } => cmd::run::run(model_id, input, input_file, !no_wait, poll_secs).await,
        Command::Status { request_id } => cmd::status::run(request_id).await,
        Command::Cancel { request_id } => cmd::cancel::run(request_id).await,
    }
}
