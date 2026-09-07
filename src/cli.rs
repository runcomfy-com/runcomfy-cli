use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

use crate::cmd;
use crate::output::OutputMode;

const ROOT_AFTER_HELP: &str = "\
EXAMPLES:
  # Sign in (opens a browser)
  runcomfy login

  # Find a hosted model, inspect its input schema, run it
  runcomfy models list --search kontext
  runcomfy models get blackforestlabs/flux-1-kontext/pro/edit
  runcomfy run blackforestlabs/flux-1-kontext/pro/edit \\
    --input '{\"prompt\": \"a small purple cat\", \"image_url\": \"https://...\"}'

  # Submit without waiting; fetch the result later
  runcomfy --output json run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"hi\"}' --no-wait | jq -r .request_id
  runcomfy result <request_id>

  # Serverless API (ComfyUI) deployments
  runcomfy deployments list
  runcomfy deployments run <deployment_id> \\
    --overrides '{\"6\": {\"inputs\": {\"text\": \"a cat\"}}}'

  # LoRA training: dataset -> job -> checkpoints
  runcomfy datasets create --name my-dataset
  runcomfy datasets upload <dataset_id> ./images/ --wait
  runcomfy train submit --config ./config.yaml
  runcomfy train result <job_id> --download

  # Account balance
  runcomfy balance

ENVIRONMENT:
  RUNCOMFY_TOKEN                Auth bearer token; takes precedence over the
                                token file. Use this in CI / containers.
  RUNCOMFY_WEB_BASE             Override the web auth base URL
                                (default https://www.runcomfy.com)
  RUNCOMFY_MODEL_API_BASE       Override the Model API base URL
                                (default https://model-api.runcomfy.net/v1)
  RUNCOMFY_SERVERLESS_API_BASE  Override the Serverless API base URL
                                (default https://api.runcomfy.net/prod/v2)
  RUNCOMFY_TRAINER_API_BASE     Override the Trainer API base URL
                                (default https://trainer-api.runcomfy.net/prod/v1)
  RUNCOMFY_CONFIG_DIR           Override the config directory
                                (default $XDG_CONFIG_HOME/runcomfy or
                                ~/.config/runcomfy)
  XDG_CONFIG_HOME               Standard XDG base directory
  NO_COLOR                      Disable color and emoji in output

EXIT CODES:
  0   success
  64  bad CLI arguments (EX_USAGE)
  65  bad input data, e.g. malformed JSON (EX_DATAERR)
  66  a local input file does not exist (EX_NOINPUT)
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
  runcomfy status <request_id>
  runcomfy result <request_id>

  # Don't download files (only print result JSON)
  runcomfy run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"...\"}' --no-download

  # Save outputs into a specific directory
  runcomfy run openai/gpt-image-2/text-to-image \\
    --input '{\"prompt\":\"...\"}' --output-dir ./out

  # Run a Trainer LoRA without deploying it: call the base model and pass
  # the LoRA (a name from your LoRA Assets, or a checkpoint URL) as input
  runcomfy run <base_model_id> \\
    --input '{\"prompt\":\"...\", \"lora\": {\"path\": \"my_lora_3000.safetensors\"}}'
";

const DEPLOYMENTS_RUN_AFTER_HELP: &str = "\
EXAMPLES:
  # Discover node IDs / input names first
  runcomfy deployments get <deployment_id> --include-payload

  # Override inputs by node ID (file inputs: public URL or data: URI)
  runcomfy deployments run <deployment_id> \\
    --overrides '{\"6\": {\"inputs\": {\"text\": \"a futuristic city\"}},
                  \"189\": {\"inputs\": {\"image\": \"https://example.com/in.jpg\"}}}'

  # Overrides from a file, results into ./out, webhook instead of polling
  runcomfy deployments run <deployment_id> --overrides-file ./inputs.json \\
    --output-dir ./out
  runcomfy deployments run <deployment_id> --overrides-file ./inputs.json \\
    --webhook-url https://example.com/hook --no-wait

  # Run a different workflow_api.json inline without updating the deployment
  runcomfy deployments run <deployment_id> --workflow-file ./workflow_api.json
";

const DATASETS_UPLOAD_AFTER_HELP: &str = "\
Each image / video needs a caption .txt with the same base name
(img_0001.jpg <-> img_0001.txt). Files up to 150 MB are uploaded directly;
larger files go through signed upload URLs automatically. Re-uploading a
filename overwrites the previous copy.

EXAMPLES:
  # Every file in a folder (top level only), then wait until READY
  runcomfy datasets upload <dataset_id> ./my-dataset/ --wait

  # Specific files
  runcomfy datasets upload <dataset_id> img_0001.jpg img_0001.txt

  # A file the server should fetch from a public URL
  runcomfy datasets upload <dataset_id> --from-url https://example.com/a.jpg \\
    --filename img_0002.jpg
";

const TRAIN_SUBMIT_AFTER_HELP: &str = "\
The config is a complete AI Toolkit YAML file. Two paths in it are fixed
by the platform:
  training_folder: /app/ai-toolkit/output
  folder_path:     /app/ai-toolkit/datasets/<dataset_name>
where <dataset_name> is the dataset's *name* (not its id) from
`runcomfy datasets list`, and the dataset must be READY.

EXAMPLES:
  runcomfy train submit --config ./config.yaml
  runcomfy train submit --config ./config.yaml --gpu-type HOPPER_141
  runcomfy train submit --config ./config.yaml --gpu-count 8 --wait
  runcomfy train status <job_id>
  runcomfy train result <job_id> --download --output-dir ./lora
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
    about = "RunComfy CLI — run hosted models, ComfyUI deployments, and LoRA training on RunComfy",
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

/// Flags shared by every command that can download generated assets.
#[derive(Args, Debug, Clone)]
pub struct DownloadOpts {
    /// Directory to download generated files into (default: current directory)
    #[arg(long, value_name = "DIR", default_value = ".")]
    pub output_dir: String,

    /// Skip downloading; only print the result JSON
    #[arg(long)]
    pub no_download: bool,
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

    /// Show the account's remaining balance
    ///
    /// One wallet funds every product: Model API requests, Serverless
    /// deployments, and training jobs all draw down this figure.
    Balance,

    /// Browse the hosted model catalog (model_ids, input schemas, prices)
    #[command(subcommand)]
    Models(ModelsCommand),

    /// Run a Model API model on RunComfy
    ///
    /// `model_id` is a slash-separated identifier from the RunComfy
    /// Models catalog, e.g. `blackforestlabs/flux-1-kontext/pro/edit`.
    /// Find model_ids with `runcomfy models list` or at
    /// https://www.runcomfy.com/models
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

        #[command(flatten)]
        download: DownloadOpts,
    },

    /// Poll the status of a Model API request (alias for `requests get`)
    Status {
        /// request_id returned by `runcomfy run`
        request_id: String,
    },

    /// Fetch the result of a Model API request (alias for `requests result`)
    ///
    /// Prints the result record (status, output, timestamps) and downloads
    /// the generated files when the request has succeeded.
    #[command(name = "result")]
    FetchResult {
        /// request_id returned by `runcomfy run`
        request_id: String,

        #[command(flatten)]
        download: DownloadOpts,
    },

    /// Cancel a queued Model API request (alias for `requests cancel`)
    Cancel {
        /// request_id returned by `runcomfy run`
        request_id: String,
    },

    /// Manage Model API requests
    #[command(subcommand)]
    Requests(RequestsCommand),

    /// Manage Serverless API (ComfyUI) deployments and run inference on them
    #[command(subcommand)]
    Deployments(DeploymentsCommand),

    /// Manage LoRA training datasets (Trainer API)
    #[command(subcommand)]
    Datasets(DatasetsCommand),

    /// Submit and manage AI Toolkit LoRA training jobs (Trainer API)
    #[command(subcommand)]
    Train(TrainCommand),

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
    Get { request_id: String },

    /// Fetch the result of a request (same as `runcomfy result`)
    #[command(name = "result")]
    GetResult {
        request_id: String,

        #[command(flatten)]
        download: DownloadOpts,
    },

    /// Cancel a request (same as `runcomfy cancel`)
    Cancel { request_id: String },
}

#[derive(Subcommand, Debug)]
pub enum ModelsCommand {
    /// List hosted models, optionally filtered
    ///
    /// Prints a table in pretty mode. `--output json` prints the raw
    /// catalog payload (inputs, required_inputs, prices, model_url).
    List {
        /// Case-insensitive match on model_id, display name, or description
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,

        /// Capability filter, e.g. text-to-image, image-to-video (see `models categories`)
        #[arg(long, value_name = "CATEGORY")]
        category: Option<String>,

        /// Execution kind filter: model, workflow, or inference
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,

        /// Include each model's full input_schema (large; prints JSON)
        #[arg(long)]
        include_schema: bool,

        /// Page size (1-500)
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: u32,

        /// Rows to skip
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },

    /// List the capability categories models are grouped into
    Categories,

    /// Show one model's details, price, and input schema
    Get {
        /// model_id (e.g. blackforestlabs/flux-1-kontext/pro/edit)
        model_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DeploymentsCommand {
    /// List deployments in your account
    List {
        /// Only these deployment IDs (comma-separated or repeated)
        #[arg(long, value_delimiter = ',', value_name = "ID")]
        ids: Vec<String>,

        /// Include workflow_api_json, overrides, and object_info_url (prints JSON)
        #[arg(long)]
        include_payload: bool,

        /// Include each deployment's README (prints JSON)
        #[arg(long)]
        include_readme: bool,
    },

    /// Show one deployment
    ///
    /// Use --include-payload to see the deployed workflow graph: its node
    /// IDs and input names are what `deployments run --overrides` targets.
    Get {
        deployment_id: String,

        /// Include workflow_api_json, overrides, and object_info_url
        #[arg(long)]
        include_payload: bool,

        /// Include the deployment's README
        #[arg(long)]
        include_readme: bool,
    },

    /// Create a Serverless API (ComfyUI) deployment from a cloud-saved workflow
    ///
    /// For LoRA deployments, create via the RunComfy UI
    /// (Trainer > LoRA Assets > Deploy) instead.
    Create {
        /// Human-readable name
        #[arg(long)]
        name: String,

        /// UUID of the cloud-saved ComfyUI workflow
        #[arg(long, value_name = "UUID")]
        workflow_id: String,

        /// Workflow version label, e.g. v1
        #[arg(long, value_name = "VERSION")]
        workflow_version: String,

        /// GPU SKU: TURING_16, AMPERE_24, AMPERE_48, ADA_48_PLUS, AMPERE_80, ADA_80_PLUS, HOPPER_141
        #[arg(long, default_value = "AMPERE_48", value_name = "SKU")]
        hardware: String,

        /// Warm instance floor (0-30); billable when > 0
        #[arg(long, default_value_t = 0)]
        min_instances: u32,

        /// Concurrency ceiling (1-60)
        #[arg(long, default_value_t = 1)]
        max_instances: u32,

        /// Pending requests per instance before scaling out
        #[arg(long, default_value_t = 1)]
        queue_size: u32,

        /// Idle seconds before an instance scales down
        #[arg(long, default_value_t = 60, value_name = "SECS")]
        keep_warm_secs: u64,
    },

    /// Update a deployment (only the flags you pass are changed)
    Update {
        deployment_id: String,

        #[arg(long)]
        name: Option<String>,

        #[arg(long, value_name = "VERSION")]
        workflow_version: Option<String>,

        /// GPU SKU (see `deployments create --help`)
        #[arg(long, value_name = "SKU")]
        hardware: Option<String>,

        #[arg(long)]
        min_instances: Option<u32>,

        #[arg(long)]
        max_instances: Option<u32>,

        #[arg(long)]
        queue_size: Option<u32>,

        #[arg(long, value_name = "SECS")]
        keep_warm_secs: Option<u64>,

        /// Resume a paused deployment
        #[arg(long, conflicts_with = "disable")]
        enable: bool,

        /// Pause the deployment (keeps its configuration)
        #[arg(long)]
        disable: bool,
    },

    /// Permanently delete a deployment (cannot be undone)
    ///
    /// Consider `deployments update <id> --disable` to pause instead.
    Delete {
        deployment_id: String,

        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Submit an inference request to a deployment and (by default) wait for it
    #[command(after_help = DEPLOYMENTS_RUN_AFTER_HELP)]
    Run {
        deployment_id: String,

        /// Partial workflow graph keyed by node ID, e.g. '{"6": {"inputs": {"text": "a cat"}}}'
        #[arg(long, value_name = "JSON")]
        overrides: Option<String>,

        /// Read overrides JSON from a file (use `-` for stdin)
        #[arg(long, value_name = "PATH", conflicts_with = "overrides")]
        overrides_file: Option<String>,

        /// Run a full workflow_api.json inline instead of the deployed one (advanced)
        #[arg(long, value_name = "PATH")]
        workflow_file: Option<String>,

        /// Extra data JSON, e.g. '{"api_key_comfy_org": "comfyui-..."}' for ComfyUI Core API nodes
        #[arg(long, value_name = "JSON")]
        extra_data: Option<String>,

        /// Webhook URL for push-based status updates
        #[arg(long, value_name = "URL")]
        webhook_url: Option<String>,

        /// Fire the webhook on every status change, not just terminal ones
        #[arg(long, requires = "webhook_url")]
        webhook_intermediate: bool,

        /// Submit and return immediately without waiting for completion
        #[arg(long)]
        no_wait: bool,

        /// Polling interval seconds while waiting (default 2)
        #[arg(long, default_value_t = 2)]
        poll_secs: u64,

        /// Stop waiting after this many seconds (the request keeps running)
        #[arg(long, value_name = "SECS")]
        timeout: Option<u64>,

        #[command(flatten)]
        download: DownloadOpts,
    },

    /// Poll the status of a deployment request
    Status {
        deployment_id: String,
        request_id: String,
    },

    /// Fetch the outputs of a deployment request (hosted for 7 days)
    #[command(name = "result")]
    GetResult {
        deployment_id: String,
        request_id: String,

        #[command(flatten)]
        download: DownloadOpts,
    },

    /// Cancel a queued or running deployment request
    Cancel {
        deployment_id: String,
        request_id: String,
    },

    /// Call a ComfyUI backend endpoint on a live instance
    ///
    /// Get the instance_id from `deployments status` while the request is
    /// in_progress. Example: `proxy <dep> <inst> api/free --body
    /// '{"unload_models": true, "free_memory": true}'` unloads models to
    /// free GPU memory.
    Proxy {
        deployment_id: String,
        instance_id: String,

        /// ComfyUI backend route, e.g. api/free
        path: String,

        /// JSON body to send (default: {})
        #[arg(long, value_name = "JSON")]
        body: Option<String>,

        /// Read the JSON body from a file (use `-` for stdin)
        #[arg(long, value_name = "PATH", conflicts_with = "body")]
        body_file: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum DatasetsCommand {
    /// Create an empty training dataset
    Create {
        /// Human-readable name, unique within your account (generated if omitted).
        /// This is the name referenced by the AI Toolkit config's folder_path.
        #[arg(long)]
        name: Option<String>,
    },

    /// List datasets in your account
    List,

    /// Show a dataset's status and its uploaded files
    ///
    /// Lifecycle: DRAFT -> UPLOADING -> READY (or FAILED). Only READY
    /// datasets can be mounted by a training job.
    Status { dataset_id: String },

    /// Permanently delete a dataset (cannot be undone)
    Delete {
        dataset_id: String,

        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Upload local files (or a folder) into a dataset
    #[command(after_help = DATASETS_UPLOAD_AFTER_HELP)]
    Upload {
        dataset_id: String,

        /// Files or directories to upload (directories: top-level files only)
        #[arg(value_name = "PATH")]
        paths: Vec<String>,

        /// Fetch one file from a public http(s) URL and upload it
        #[arg(long, value_name = "URL")]
        from_url: Option<String>,

        /// Filename to store the --from-url file under (default: URL basename)
        #[arg(long, requires = "from_url", value_name = "NAME")]
        filename: Option<String>,

        /// After uploading, poll until the dataset is READY (or FAILED)
        #[arg(long)]
        wait: bool,

        /// Polling interval seconds for --wait (default 5)
        #[arg(long, default_value_t = 5)]
        poll_secs: u64,
    },
}

#[derive(Subcommand, Debug)]
pub enum TrainCommand {
    /// Submit an AI Toolkit training job (returns as soon as it is queued)
    #[command(after_help = TRAIN_SUBMIT_AFTER_HELP)]
    Submit {
        /// AI Toolkit YAML config file (use `-` for stdin)
        #[arg(long, value_name = "PATH")]
        config: String,

        /// GPU type: ADA_80_PLUS (H100) or HOPPER_141 (H200)
        #[arg(long, default_value = "ADA_80_PLUS", value_name = "TYPE")]
        gpu_type: String,

        /// Number of GPUs: 1 (default) or 8 (multi-GPU; ADA_80_PLUS only)
        #[arg(long)]
        gpu_count: Option<u32>,

        /// Specific GPU selector, e.g. "#1"
        #[arg(long, value_name = "ID")]
        gpu_id: Option<String>,

        /// Poll until the job finishes and then print its result.
        /// Ctrl-C stops watching but does NOT cancel the job.
        #[arg(long)]
        wait: bool,

        /// Polling interval seconds for --wait (default 10)
        #[arg(long, default_value_t = 10)]
        poll_secs: u64,

        /// Stop waiting after this many seconds (the job keeps running)
        #[arg(long, value_name = "SECS", requires = "wait")]
        timeout: Option<u64>,
    },

    /// Poll a training job's status and step progress
    ///
    /// Lifecycle: IN_QUEUE -> RUNNING -> STOPPED (finished or preempted),
    /// FAILED (see `error`), or CANCELED.
    Status { job_id: String },

    /// Fetch a training job's artifacts (checkpoints, config, samples)
    ///
    /// Safe to call while the job is RUNNING — the artifact list grows over
    /// time — and after FAILED / CANCELED to recover what was produced.
    #[command(name = "result")]
    GetResult {
        job_id: String,

        /// Download the artifacts (checkpoints can be large, so this is opt-in)
        #[arg(long)]
        download: bool,

        /// Directory to download artifacts into (default: current directory)
        #[arg(long, value_name = "DIR", default_value = ".")]
        output_dir: String,
    },

    /// Cancel a queued or running training job
    Cancel { job_id: String },

    /// Resume a stopped job from its latest checkpoint (same job_id)
    Resume { job_id: String },

    /// Replace the config of a STOPPED / CANCELED / FAILED job, then `resume` it
    ///
    /// `config.name` in the new YAML must match the original job's name.
    Edit {
        job_id: String,

        /// Updated AI Toolkit YAML config file (use `-` for stdin)
        #[arg(long, value_name = "PATH")]
        config: String,
    },
}

pub async fn dispatch(args: Cli) -> Result<()> {
    match args.command {
        Command::Login { web_base } => cmd::login::run(web_base).await,
        Command::Logout => cmd::login::logout().await,
        Command::Whoami => cmd::whoami::run().await,
        Command::Balance => cmd::balance::run().await,
        Command::Models(sub) => match sub {
            ModelsCommand::List {
                search,
                category,
                kind,
                include_schema,
                limit,
                offset,
            } => cmd::models::list(search, category, kind, include_schema, limit, offset).await,
            ModelsCommand::Categories => cmd::models::categories().await,
            ModelsCommand::Get { model_id } => cmd::models::get(model_id).await,
        },
        Command::Run {
            model_id,
            input,
            input_file,
            no_wait,
            poll_secs,
            download,
        } => cmd::run::run(model_id, input, input_file, !no_wait, poll_secs, download).await,
        Command::Status { request_id } => cmd::status::run(request_id).await,
        Command::FetchResult {
            request_id,
            download,
        } => cmd::result::run(request_id, download).await,
        Command::Cancel { request_id } => cmd::cancel::run(request_id).await,
        Command::Requests(sub) => match sub {
            RequestsCommand::Get { request_id } => cmd::status::run(request_id).await,
            RequestsCommand::GetResult {
                request_id,
                download,
            } => cmd::result::run(request_id, download).await,
            RequestsCommand::Cancel { request_id } => cmd::cancel::run(request_id).await,
        },
        Command::Deployments(sub) => match sub {
            DeploymentsCommand::List {
                ids,
                include_payload,
                include_readme,
            } => cmd::deployments::list(ids, include_payload, include_readme).await,
            DeploymentsCommand::Get {
                deployment_id,
                include_payload,
                include_readme,
            } => cmd::deployments::get(deployment_id, include_payload, include_readme).await,
            DeploymentsCommand::Create {
                name,
                workflow_id,
                workflow_version,
                hardware,
                min_instances,
                max_instances,
                queue_size,
                keep_warm_secs,
            } => {
                cmd::deployments::create(cmd::deployments::CreateArgs {
                    name,
                    workflow_id,
                    workflow_version,
                    hardware,
                    min_instances,
                    max_instances,
                    queue_size,
                    keep_warm_secs,
                })
                .await
            }
            DeploymentsCommand::Update {
                deployment_id,
                name,
                workflow_version,
                hardware,
                min_instances,
                max_instances,
                queue_size,
                keep_warm_secs,
                enable,
                disable,
            } => {
                cmd::deployments::update(
                    deployment_id,
                    cmd::deployments::UpdateArgs {
                        name,
                        workflow_version,
                        hardware,
                        min_instances,
                        max_instances,
                        queue_size,
                        keep_warm_secs,
                        is_enabled: if enable {
                            Some(true)
                        } else if disable {
                            Some(false)
                        } else {
                            None
                        },
                    },
                )
                .await
            }
            DeploymentsCommand::Delete { deployment_id, yes } => {
                cmd::deployments::delete(deployment_id, yes).await
            }
            DeploymentsCommand::Run {
                deployment_id,
                overrides,
                overrides_file,
                workflow_file,
                extra_data,
                webhook_url,
                webhook_intermediate,
                no_wait,
                poll_secs,
                timeout,
                download,
            } => {
                cmd::deployments::run(
                    deployment_id,
                    cmd::deployments::RunArgs {
                        overrides,
                        overrides_file,
                        workflow_file,
                        extra_data,
                        webhook_url,
                        webhook_intermediate,
                        wait: !no_wait,
                        poll_secs,
                        timeout,
                        download,
                    },
                )
                .await
            }
            DeploymentsCommand::Status {
                deployment_id,
                request_id,
            } => cmd::deployments::status(deployment_id, request_id).await,
            DeploymentsCommand::GetResult {
                deployment_id,
                request_id,
                download,
            } => cmd::deployments::result(deployment_id, request_id, download).await,
            DeploymentsCommand::Cancel {
                deployment_id,
                request_id,
            } => cmd::deployments::cancel(deployment_id, request_id).await,
            DeploymentsCommand::Proxy {
                deployment_id,
                instance_id,
                path,
                body,
                body_file,
            } => cmd::deployments::proxy(deployment_id, instance_id, path, body, body_file).await,
        },
        Command::Datasets(sub) => match sub {
            DatasetsCommand::Create { name } => cmd::datasets::create(name).await,
            DatasetsCommand::List => cmd::datasets::list().await,
            DatasetsCommand::Status { dataset_id } => cmd::datasets::status(dataset_id).await,
            DatasetsCommand::Delete { dataset_id, yes } => {
                cmd::datasets::delete(dataset_id, yes).await
            }
            DatasetsCommand::Upload {
                dataset_id,
                paths,
                from_url,
                filename,
                wait,
                poll_secs,
            } => {
                cmd::datasets::upload(dataset_id, paths, from_url, filename, wait, poll_secs).await
            }
        },
        Command::Train(sub) => match sub {
            TrainCommand::Submit {
                config,
                gpu_type,
                gpu_count,
                gpu_id,
                wait,
                poll_secs,
                timeout,
            } => {
                cmd::train::submit(cmd::train::SubmitArgs {
                    config,
                    gpu_type,
                    gpu_count,
                    gpu_id,
                    wait,
                    poll_secs,
                    timeout,
                })
                .await
            }
            TrainCommand::Status { job_id } => cmd::train::status(job_id).await,
            TrainCommand::GetResult {
                job_id,
                download,
                output_dir,
            } => cmd::train::result(job_id, download, output_dir).await,
            TrainCommand::Cancel { job_id } => cmd::train::cancel(job_id).await,
            TrainCommand::Resume { job_id } => cmd::train::resume(job_id).await,
            TrainCommand::Edit { job_id, config } => cmd::train::edit(job_id, config).await,
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
