use std::collections::HashMap;
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Cache {
    /// skill_name → deployment_id, populated by `runcomfy deploy create <skill>`
    /// and read by `runcomfy run <skill>`.
    #[serde(default)]
    pub skill_deployments: HashMap<String, String>,

    /// request_id → deployment_id, populated when run wait=true polls and
    /// stored briefly so `runcomfy status <request_id>` can find the deployment.
    #[serde(default)]
    pub recent_requests: HashMap<String, String>,
}

fn config_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not resolve OS config dir")?;
    Ok(base.join(APP_DIR))
}

fn token_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("token.json"))
}

fn cache_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("cache.json"))
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

pub fn load_token() -> Result<Option<Token>> {
    let path = token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read token at {}", path.display()))?;
    let token: Token = serde_json::from_str(&raw).context("parse token.json")?;
    Ok(Some(token))
}

pub fn clear_token() -> Result<()> {
    let path = token_path()?;
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("remove token at {}", path.display()))?;
    }
    Ok(())
}

pub fn load_cache() -> Result<Cache> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(Cache::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn save_cache(cache: &Cache) -> Result<()> {
    ensure_dir()?;
    let path = cache_path()?;
    let json = serde_json::to_string_pretty(cache)?;
    std::fs::write(&path, json)?;
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
