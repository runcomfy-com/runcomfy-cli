#!/usr/bin/env sh
# RunComfy CLI installer.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/InceptionsAI/runcomfy-cli/main/install.sh | sh
#
# Environment:
#   RUNCOMFY_VERSION   Pin a specific version, e.g. v0.1.0 (default: latest)
#   RUNCOMFY_PREFIX    Install dir (default: /usr/local/bin if writable, else ~/.local/bin)

set -eu

REPO="InceptionsAI/runcomfy-cli"

err() { printf 'install: %s\n' "$1" >&2; exit 1; }
log() { printf 'install: %s\n' "$1"; }

require() { command -v "$1" >/dev/null 2>&1 || err "missing required tool: $1"; }
require curl
require tar
require uname
require mktemp

# ---- detect OS / arch -------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) os_id="apple-darwin" ;;
  Linux)  os_id="unknown-linux-gnu" ;;
  *) err "unsupported OS: $os" ;;
esac
case "$arch" in
  arm64|aarch64) arch_id="aarch64" ;;
  x86_64|amd64)  arch_id="x86_64" ;;
  *) err "unsupported arch: $arch" ;;
esac
target="${arch_id}-${os_id}"

# ---- resolve version --------------------------------------------------------
version="${RUNCOMFY_VERSION:-}"
if [ -z "$version" ]; then
  log "resolving latest release"
  version="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -m1 '"tag_name":' \
    | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
  [ -n "$version" ] || err "could not resolve latest version (rate-limited? set RUNCOMFY_VERSION)"
fi
case "$version" in v*) ;; *) version="v${version}" ;; esac

tarball="runcomfy-${target}.tar.gz"
url="https://github.com/${REPO}/releases/download/${version}/${tarball}"
sum_url="${url}.sha256"

# ---- pick install prefix ----------------------------------------------------
prefix="${RUNCOMFY_PREFIX:-}"
if [ -z "$prefix" ]; then
  if [ -w /usr/local/bin ] 2>/dev/null; then
    prefix="/usr/local/bin"
  else
    prefix="${HOME}/.local/bin"
  fi
fi
mkdir -p "$prefix"

# ---- download + verify ------------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

log "downloading $url"
curl -fsSL "$url" -o "${tmp}/${tarball}"

log "verifying checksum"
curl -fsSL "$sum_url" -o "${tmp}/${tarball}.sha256"
expected="$(awk '{print $1}' "${tmp}/${tarball}.sha256")"
if command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "${tmp}/${tarball}" | awk '{print $1}')"
elif command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${tmp}/${tarball}" | awk '{print $1}')"
else
  err "missing required tool: shasum or sha256sum"
fi
[ -n "$expected" ] || err "could not parse expected checksum"
[ "$expected" = "$actual" ] || err "checksum mismatch (expected $expected, got $actual)"

# ---- extract + install ------------------------------------------------------
log "extracting"
tar -xzf "${tmp}/${tarball}" -C "$tmp"

if install -m 0755 "${tmp}/runcomfy" "${prefix}/runcomfy" 2>/dev/null; then
  :
else
  cp "${tmp}/runcomfy" "${prefix}/runcomfy"
  chmod 0755 "${prefix}/runcomfy"
fi

log "installed: ${prefix}/runcomfy"

case ":${PATH}:" in
  *":${prefix}:"*) ;;
  *)
    printf '\nNOTE: %s is not in PATH. Add it with:\n  export PATH="%s:$PATH"\n' \
      "$prefix" "$prefix" >&2
    ;;
esac

"${prefix}/runcomfy" --version 2>/dev/null || true
