# @runcomfy/cli

Command-line tool for [RunComfy](https://www.runcomfy.com) — run AI media models, manage requests, download outputs.

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
runcomfy run blackforestlabs/flux-2-klein/9b/text-to-image \
  --input '{"prompt": "ukiyo-e mountain"}'
```

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
