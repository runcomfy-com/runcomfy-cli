# @runcomfy/cli

Command-line tool for [RunComfy](https://www.runcomfy.com) — run hosted AI media models, manage Serverless API (ComfyUI) deployments, and train LoRAs.

## Install

```bash
# One-shot, no install
npx -y @runcomfy/cli login

# Or globally
npm i -g @runcomfy/cli
```

A native binary (Rust) is downloaded from [GitHub Releases](https://github.com/runcomfy-com/runcomfy-cli/releases) during `postinstall` and verified via SHA-256.

To skip the download (when vendoring or using a pre-installed binary), set `RUNCOMFY_SKIP_POSTINSTALL=1` before installing.

## Quick start

```bash
runcomfy login
runcomfy models list --search klein
runcomfy run blackforestlabs/flux-2-klein/9b/text-to-image \
  --input '{"prompt": "ukiyo-e mountain"}'

runcomfy deployments list          # Serverless API (ComfyUI) deployments
runcomfy datasets list             # LoRA training datasets
runcomfy train status <job_id>     # AI Toolkit training jobs
runcomfy balance
```

Run `runcomfy --help` for the full command list (`models`, `run`, `result`,
`deployments`, `datasets`, `train`, ...).

## Docs

Full documentation: <https://docs.runcomfy.com/cli/introduction>

## Supported platforms

| OS    | Arch  |
| ----- | ----- |
| macOS | arm64 |
| macOS | x64   |
| Linux | x64   |
| Linux | arm64 |

## License

MIT
