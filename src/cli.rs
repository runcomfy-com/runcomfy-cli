use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::cmd;

#[derive(Parser, Debug)]
#[command(name = "runcomfy", version, about = "RunComfy CLI — run AI media models, deploy ComfyUI workflows, train LoRAs", long_about = None)]
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

    /// Run a model or auto-deployed workflow
    ///
    /// `target` may be either a `model_id` (Model API, e.g.
    /// `blackforestlabs/flux-2-klein/9b/text-to-image`) or a registered
    /// official skill name (e.g. `face-swap`, auto-deploys on first use).
    Run {
        /// model_id or skill name
        target: String,

        /// JSON payload for the model/workflow inputs
        #[arg(long, value_name = "JSON")]
        input: Option<String>,

        /// Read JSON input from a file
        #[arg(long, value_name = "PATH", conflicts_with = "input")]
        input_file: Option<String>,

        /// Wait for completion and print the result (default: true)
        #[arg(long, default_value_t = true)]
        wait: bool,

        /// Polling interval seconds while waiting
        #[arg(long, default_value_t = 2)]
        poll_secs: u64,
    },

    /// Manage Serverless deployments (workflow endpoints)
    #[command(subcommand)]
    Deploy(DeployCommand),

    /// Poll the status of a request
    Status {
        /// request_id returned by `runcomfy run`
        request_id: String,

        /// deployment_id (required if not run via a registered skill)
        #[arg(long)]
        deployment_id: Option<String>,
    },

    /// Inspect or manage the request queue
    #[command(subcommand)]
    Queue(QueueCommand),

    /// Train a LoRA via the Trainer API
    Train {
        /// base model: flux-2-klein | wan-2-2 | ltx-2-3 | qwen-image | qwen-image-edit | z-image | flux-2-dev
        #[arg(long)]
        base: String,

        /// Path to a local dataset directory (image + caption pairs)
        #[arg(long)]
        dataset: String,

        /// Use case template: character | style | likeness | relight | line-art
        #[arg(long, default_value = "character")]
        use_case: String,

        /// GPU type: H100 | H200
        #[arg(long, default_value = "H100")]
        gpu: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DeployCommand {
    /// Auto-deploy an official workflow under your account
    Create {
        /// Skill name (e.g. `face-swap`, `lipsync`); maps to a RunComfy official workflow
        skill: String,
    },
    /// List your existing deployments
    List,
    /// Delete a deployment
    Delete {
        deployment_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum QueueCommand {
    /// List queued/running requests
    List,
    /// Cancel a request
    Cancel {
        request_id: String,
        #[arg(long)]
        deployment_id: Option<String>,
    },
}

pub async fn dispatch(args: Cli) -> Result<()> {
    match args.command {
        Command::Login { web_base } => cmd::login::run(web_base).await,
        Command::Logout => cmd::login::logout().await,
        Command::Whoami => cmd::whoami::run().await,
        Command::Run {
            target,
            input,
            input_file,
            wait,
            poll_secs,
        } => cmd::run::run(target, input, input_file, wait, poll_secs).await,
        Command::Deploy(sub) => cmd::deploy::dispatch(sub).await,
        Command::Status {
            request_id,
            deployment_id,
        } => cmd::status::run(request_id, deployment_id).await,
        Command::Queue(sub) => cmd::queue::dispatch(sub).await,
        Command::Train {
            base,
            dataset,
            use_case,
            gpu,
        } => cmd::train::run(base, dataset, use_case, gpu).await,
    }
}
