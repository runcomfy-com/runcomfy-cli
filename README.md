# runcomfy CLI

Command-line tool for [RunComfy](https://www.runcomfy.com): run hosted AI media
models, manage Serverless API (ComfyUI) deployments, and train LoRAs — the same
surface as the [RunComfy MCP server](https://github.com/runcomfy-com/runcomfy-mcp),
from a terminal or a script.

## Install

```bash
# Easiest — via npx (no install)
npx -y @runcomfy/cli login

# Or install globally via npm
npm i -g @runcomfy/cli

# Or curl install (Rust binary directly)
curl -fsSL https://runcomfy.com/install.sh | sh
```

## Quick start

```bash
# Login (opens browser for OAuth); or export RUNCOMFY_TOKEN in CI
runcomfy login
runcomfy whoami
runcomfy balance

# Find a hosted model, read its input schema, run it
runcomfy models list --search kontext
runcomfy models get blackforestlabs/flux-1-kontext/pro/edit
runcomfy run blackforestlabs/flux-1-kontext/pro/edit \
  --input '{"prompt": "she is now holding an umbrella", "image_url": "https://..."}'

# Submit without waiting; fetch the result (and files) later
runcomfy --output json run openai/gpt-image-2/text-to-image \
  --input '{"prompt": "ukiyo-e mountain"}' --no-wait | jq -r .request_id
runcomfy status <request_id>
runcomfy result <request_id> --output-dir ./out

# Serverless API (ComfyUI) deployments
runcomfy deployments list
runcomfy deployments get <deployment_id> --include-payload   # node IDs for overrides
runcomfy deployments run <deployment_id> \
  --overrides '{"6": {"inputs": {"text": "a futuristic city"}}}'

# LoRA training: dataset -> job -> checkpoints
runcomfy datasets create --name my-dataset
runcomfy datasets upload <dataset_id> ./my-dataset/ --wait
runcomfy train submit --config ./config.yaml
runcomfy train status <job_id>
runcomfy train result <job_id> --download --output-dir ./lora
```

Every command accepts `--output json` for machine-readable output, `-q`, and
`-v`. Exit codes follow `sysexits(3)`; see `runcomfy --help`.

## Commands

| Group | Commands | Backs |
| --- | --- | --- |
| Auth | `login`, `logout`, `whoami` | `www.runcomfy.com/api/cli-auth`, `/api/auth/me` |
| Account | `balance` | `GET /prod/v2/balance` |
| Model catalog | `models list`, `models categories`, `models get` | Model API `GET /v1/models…` |
| Model API requests | `run`, `status`, `result`, `cancel` (aliases: `requests get/result/cancel`) | Model API `POST /v1/models/{id}`, `GET/POST /v1/requests/{id}/…` |
| Serverless deployments | `deployments list/get/create/update/delete` | `GET/POST/PATCH/DELETE /prod/v2/deployments…` |
| Serverless inference | `deployments run/status/result/cancel/proxy` | `POST …/inference`, `…/requests/{id}/…`, `…/instances/{id}/proxy/…` |
| Trainer datasets | `datasets create/list/status/delete/upload` | `…/trainers/datasets…` (multipart ≤150 MB, signed URLs above) |
| Trainer jobs | `train submit/status/result/cancel/resume/edit` | `…/trainers/ai-toolkit/jobs…` |

Waiting commands (`run`, `deployments run`) cancel the remote request on
Ctrl-C so an abandoned run stops billing. `train submit --wait` and
`datasets upload --wait` only stop watching on Ctrl-C — the job keeps running.
Destructive commands (`deployments delete`, `datasets delete`) prompt, or need
`--yes` when not attached to a terminal.

Generated files are downloaded only from RunComfy CDN hosts
(`*.runcomfy.net`, `*.runcomfy.com`); other URLs are listed but not fetched.

## Build from source

```bash
cargo build --release
./target/release/runcomfy --help
```

## Architecture

```
cli/
├── Cargo.toml
├── src/
│   ├── main.rs        Entry point + tokio runtime
│   ├── cli.rs         clap subcommand definitions
│   ├── config.rs      Local config + token persistence (~/.config/runcomfy/)
│   ├── api.rs         HTTP client + base URLs for the RunComfy APIs
│   ├── poll.rs        Wait-until-terminal loop (Ctrl-C cancel / detach)
│   ├── download.rs    Trusted-host asset downloads
│   ├── input.rs       JSON / file / stdin argument parsing
│   ├── output.rs      --output pretty|json, progress, tables
│   ├── error.rs       Error types
│   ├── exit.rs        sysexits-style exit codes
│   └── cmd/           Subcommand implementations
│       ├── login.rs / whoami.rs / balance.rs
│       ├── models.rs
│       ├── run.rs / status.rs / result.rs / cancel.rs
│       ├── deployments.rs
│       ├── datasets.rs
│       └── train.rs
```

## Endpoints

- Model API: `https://model-api.runcomfy.net/v1/...` (`RUNCOMFY_MODEL_API_BASE`)
- Serverless API: `https://api.runcomfy.net/prod/v2/...` (`RUNCOMFY_SERVERLESS_API_BASE`)
- Trainer API: `https://trainer-api.runcomfy.net/prod/v1/...` (`RUNCOMFY_TRAINER_API_BASE`)
- Web auth: `https://www.runcomfy.com/api/cli-auth/...` (`RUNCOMFY_WEB_BASE`)

Docs: <https://docs.runcomfy.com/cli/introduction>
