use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

use crate::cmd;
use crate::output::OutputMode;

const ROOT_AFTER_HELP: &str = "\
EXAMPLES:
  # Sign in (opens a browser)
  runcomfy login

  # Run a Model API model and download the result
  runcomfy run blackforestlabs/flux-1-kontext/pro/edit \\
    --input '{\"prompt\": \"a small purple cat\"}'

  # Submit without waiting; pipe-friendly JSON output
  runcomfy --output json run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"hi\"}' --no-wait | jq -r .request_id

  # Check status / cancel a queued request
  runcomfy requests get    req_xxx
  runcomfy requests cancel req_xxx

ENVIRONMENT:
  RUNCOMFY_TOKEN          Auth bearer token; takes precedence over the
                          token file. Use this in CI / containers.
  RUNCOMFY_WEB_BASE       Override the web auth base URL
                          (default https://www.runcomfy.com)
  RUNCOMFY_MODEL_API_BASE Override the Model API base URL
                          (default https://model-api.runcomfy.net/v1)
  RUNCOMFY_CONFIG_DIR     Override the config directory
                          (default $XDG_CONFIG_HOME/runcomfy or
                          ~/.config/runcomfy)
  XDG_CONFIG_HOME         Standard XDG base directory
  NO_COLOR                Disable color and emoji in output

EXIT CODES:
  0   success
  64  bad CLI arguments (EX_USAGE)
  65  bad input data, e.g. malformed JSON (EX_DATAERR)
  69  upstream service error 5xx (EX_UNAVAILABLE)
  75  retryable transient error: timeout / 429 (EX_TEMPFAIL)
  77  authentication error 401 / 403 (EX_NOPERM)
";

const RUN_AFTER_HELP: &str = "\
EXAMPLES:
  # Inline JSON
  runcomfy run blackforestlabs/flux-1-kontext/pro/edit \\
    --input '{\"prompt\": \"a small purple cat\", \"aspect_ratio\": \"16:9\"}'

  # JSON from a file (or '-' for stdin)
  runcomfy run openai/gpt-image-2/text-to-image --input-file ./req.json
  cat req.json | runcomfy run openai/gpt-image-2/text-to-image --input-file -

  # Submit and exit immediately; check later
  runcomfy run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"...\"}' --no-wait

  # Don't download files (only print result JSON)
  runcomfy run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"...\"}' --no-download

  # Save outputs into a specific directory
  runcomfy run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"...\"}' --output-dir ./out
";

/// `runcomfy --version` output, including the git sha and commit date
/// captured by `build.rs`. Falls back to `unknown` outside a git
/// checkout (cargo install, docker build, etc.).
const VERSION_FULL: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("RUNCOMFY_GIT_SHA"),
    ", ",
    env!("RUNCOMFY_BUILD_DATE"),
    ")"
);

#[derive(Parser, Debug)]
#[command(
    name = "runcomfy",
    version = VERSION_FULL,
    about = "RunComfy CLI — run AI media models on RunComfy",
    after_help = ROOT_AFTER_HELP,
    long_about = None,
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug)]
pub struct GlobalOpts {
    /// Output format
    #[arg(long, value_enum, global = true, default_value_t = OutputMode::Pretty)]
    pub output: OutputMode,

    /// Suppress non-error output
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Verbose output. Pass twice (-vv) for trace-level detail.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
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
    /// Find model_ids at https://www.runcomfy.com/models
    #[command(after_help = RUN_AFTER_HELP)]
    Run {
        /// model_id (e.g. blackforestlabs/flux-1-kontext/pro/edit)
        model_id: String,

        /// JSON payload matching the model's Input schema
        #[arg(long, value_name = "JSON")]
        input: Option<String>,

        /// Read JSON input from a file (use `-` to read from stdin)
        #[arg(long, value_name = "PATH", conflicts_with = "input")]
        input_file: Option<String>,

        /// Submit and return immediately without waiting for completion
        #[arg(long)]
        no_wait: bool,

        /// Polling interval seconds while waiting (default 2)
        #[arg(long, default_value_t = 2)]
        poll_secs: u64,

        /// Directory to download generated files into (default: current directory)
        #[arg(long, value_name = "DIR", default_value = ".")]
        output_dir: String,

        /// Skip downloading; only print the result JSON
        #[arg(long)]
        no_download: bool,
    },

    /// Poll the status of a Model API request (alias for `requests get`)
    Status {
        /// request_id returned by `runcomfy run`
        request_id: String,
    },

    /// Cancel a queued Model API request (alias for `requests cancel`)
    Cancel {
        /// request_id returned by `runcomfy run`
        request_id: String,
    },

    /// Manage Model API requests
    #[command(subcommand)]
    Requests(RequestsCommand),

    /// Generate shell completion script
    ///
    /// Pipe the output into your shell's completion path. For example:
    ///
    ///   runcomfy completion zsh > "${fpath[1]}/_runcomfy"
    ///   runcomfy completion bash > /etc/bash_completion.d/runcomfy
    Completion {
        /// Target shell
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
pub enum RequestsCommand {
    /// Get the status of a request (same as `runcomfy status`)
    Get {
        request_id: String,
    },
    /// Cancel a request (same as `runcomfy cancel`)
    Cancel {
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
            output_dir,
            no_download,
        } => {
            cmd::run::run(
                model_id,
                input,
                input_file,
                !no_wait,
                poll_secs,
                output_dir,
                !no_download,
            )
            .await
        }
        Command::Status { request_id } => cmd::status::run(request_id).await,
        Command::Cancel { request_id } => cmd::cancel::run(request_id).await,
        Command::Requests(sub) => match sub {
            RequestsCommand::Get { request_id } => cmd::status::run(request_id).await,
            RequestsCommand::Cancel { request_id } => cmd::cancel::run(request_id).await,
        },
        Command::Completion { shell } => {
            print_completion(shell);
            Ok(())
        }
    }
}

fn print_completion(shell: Shell) {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
}
