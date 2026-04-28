# runcomfy CLI

Command-line tool for running RunComfy AI media models, deploying ComfyUI workflows, and training LoRAs.

## Install

```bash
# Easiest — via npx (no install)
npx -y @runcomfy/cli run flux-2 --prompt "ukiyo-e mountain"

# Or install globally via npm
npm i -g @runcomfy/cli

# Or curl install (Rust binary directly)
curl -fsSL https://runcomfy.com/install.sh | sh
```

## Quick start

```bash
# Login (opens browser for OAuth)
runcomfy login

# Run a Model API model
runcomfy run blackforestlabs/flux-2-klein/9b/text-to-image \
  --input '{"prompt": "ukiyo-e mountain"}'

# Auto-deploy and run a Serverless workflow
runcomfy run face-swap --source a.jpg --target b.jpg

# Check job status
runcomfy status <request_id>

# List queued/running jobs
runcomfy queue list

# Train a LoRA
runcomfy train --base flux-2-klein --dataset ./my-dataset \
  --use-case character

# Show current user
runcomfy whoami
```

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
│   ├── api.rs         HTTP client wrappers for RunComfy APIs
│   ├── error.rs       Error types
│   └── cmd/           Subcommand implementations
│       ├── login.rs
│       ├── run.rs
│       ├── deploy.rs
│       ├── status.rs
│       ├── queue.rs
│       ├── train.rs
│       └── whoami.rs
```

## Endpoints

- Model API: `https://api.runcomfy.net/prod/v2/...`
- Trainer API: `https://trainer-api.runcomfy.net/prod/v1/...`
- Web auth: `https://www.runcomfy.com/api/cli-auth/...`
