use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "runcomfy";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>, // unix seconds
    pub user_email: Option<String>,
}

/// Resolve the config directory.
///
/// Precedence:
///   1. `$RUNCOMFY_CONFIG_DIR`     (explicit override; useful for tests / CI)
///   2. `$XDG_CONFIG_HOME/runcomfy`
///   3. `~/.config/runcomfy`       (cross-platform default — same path on
///      every OS, not the macOS `~/Library/Application Support`)
fn config_dir() -> Result<PathBuf> {
    if let Ok(custom) = std::env::var("RUNCOMFY_CONFIG_DIR") {
        if !custom.is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join(APP_DIR));
        }
    }
    let home = dirs::home_dir().context("could not resolve home dir")?;
    Ok(home.join(".config").join(APP_DIR))
}

/// Legacy macOS path (`~/Library/Application Support/runcomfy/`) older
/// versions of this CLI wrote to. Read-only fallback for migration.
#[cfg(target_os = "macos")]
fn legacy_macos_token_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join("Library")
            .join("Application Support")
            .join(APP_DIR)
            .join("token.json"),
    )
}

fn token_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("token.json"))
}

fn ensure_dir() -> Result<()> {
    let dir = config_dir()?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create config dir {}", dir.display()))?;
    }
    Ok(())
}

pub fn save_token(token: &Token) -> Result<()> {
    ensure_dir()?;
    let path = token_path()?;
    let json = serde_json::to_string_pretty(token)?;
    write_secure(&path, &json)?;
    Ok(())
}

/// Load the saved token.
///
/// Precedence:
///   1. `$RUNCOMFY_TOKEN` env var (any non-empty value) — wraps a
///      synthetic `Token`. Useful for CI / containers where the
///      device-code login flow can't run.
///   2. `<config_dir>/token.json` written by `runcomfy login`.
///   3. (macOS only) Legacy `~/Library/Application Support/runcomfy/token.json`
///      from older CLI builds.
pub fn load_token() -> Result<Option<Token>> {
    if let Ok(env_token) = std::env::var("RUNCOMFY_TOKEN") {
        let trimmed = env_token.trim();
        if !trimmed.is_empty() {
            return Ok(Some(Token {
                access_token: trimmed.to_string(),
                refresh_token: None,
                expires_at: None,
                user_email: None,
            }));
        }
    }

    let path = token_path()?;
    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read token at {}", path.display()))?;
        let token: Token = serde_json::from_str(&raw).context("parse token.json")?;
        return Ok(Some(token));
    }

    // macOS-only legacy fallback (read, don't write).
    #[cfg(target_os = "macos")]
    if let Some(legacy) = legacy_macos_token_path() {
        if legacy.exists() {
            let raw = std::fs::read_to_string(&legacy)
                .with_context(|| format!("read legacy token at {}", legacy.display()))?;
            let token: Token = serde_json::from_str(&raw)
                .context("parse legacy token.json")?;
            return Ok(Some(token));
        }
    }

    Ok(None)
}

pub fn clear_token() -> Result<()> {
    let path = token_path()?;
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("remove token at {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn write_secure(path: &PathBuf, contents: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    use std::io::Write;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_secure(path: &PathBuf, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}
