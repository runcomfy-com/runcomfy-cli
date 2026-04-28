//! Thin HTTP wrappers for the RunComfy public APIs.
//!
//! Endpoint layout (from docs.runcomfy.com):
//! - `https://model-api.runcomfy.net/v1`        — Model API (catalog inference)
//! - `https://api.runcomfy.net/prod/v2`         — Serverless API (deployments)
//! - `https://trainer-api.runcomfy.net/prod/v1` — Trainer
//! - `https://www.runcomfy.com/api/cli-auth`    — Device-code OAuth (web)

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config;
use crate::error::CliError;

pub const DEFAULT_MODEL_API_BASE: &str = "https://model-api.runcomfy.net/v1";
pub const DEFAULT_SERVERLESS_API_BASE: &str = "https://api.runcomfy.net/prod/v2";
pub const DEFAULT_TRAINER_BASE: &str = "https://trainer-api.runcomfy.net/prod/v1";
pub const DEFAULT_WEB_BASE: &str = "https://www.runcomfy.com";

/// Resolve the Model API base URL: `RUNCOMFY_MODEL_API_BASE` env var if set,
/// else the production default. Used by `run` / `status` / `cancel`.
pub fn model_api_base() -> String {
    std::env::var("RUNCOMFY_MODEL_API_BASE")
        .unwrap_or_else(|_| DEFAULT_MODEL_API_BASE.to_string())
}

/// Resolve the web base URL: `RUNCOMFY_WEB_BASE` env var if set, else
/// the production default. Used by commands that don't take an explicit
/// `--web-base` flag (e.g. `whoami`).
pub fn web_base() -> String {
    std::env::var("RUNCOMFY_WEB_BASE").unwrap_or_else(|_| DEFAULT_WEB_BASE.to_string())
}

/// Build a reqwest client with sensible defaults (timeouts, user-agent).
pub fn http() -> Result<Client> {
    let ua = format!("runcomfy-cli/{}", env!("CARGO_PKG_VERSION"));
    Ok(Client::builder()
        .user_agent(ua)
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?)
}

/// Read the saved bearer token, error out if missing.
pub fn require_token() -> Result<String> {
    let tok = config::load_token()?.ok_or(CliError::NotAuthenticated)?;
    Ok(tok.access_token)
}

/// Helper: GET <base><path> with bearer token, deserialize JSON.
pub async fn get_json<T: DeserializeOwned>(base: &str, path: &str) -> Result<T> {
    let client = http()?;
    let token = require_token()?;
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;
    handle_response(resp).await
}

/// Helper: POST <base><path> with bearer token, JSON body, deserialize JSON.
pub async fn post_json<T: DeserializeOwned>(base: &str, path: &str, body: &Value) -> Result<T> {
    let client = http()?;
    let token = require_token()?;
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {}", url))?;
    handle_response(resp).await
}

/// Helper: DELETE <base><path> with bearer token. Returns parsed JSON if any.
pub async fn delete_json<T: DeserializeOwned>(base: &str, path: &str) -> Result<T> {
    let client = http()?;
    let token = require_token()?;
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let resp = client
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("DELETE {}", url))?;
    handle_response(resp).await
}

async fn handle_response<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp.json::<T>().await.context("parse JSON response")?);
    }

    if status.as_u16() == 401 {
        return Err(CliError::TokenRejected.into());
    }

    let body = resp.text().await.unwrap_or_default();
    let message = if body.is_empty() {
        status.canonical_reason().unwrap_or("unknown").to_string()
    } else {
        truncate_error_body(&body)
    };
    Err(anyhow!(CliError::Api {
        status: status.as_u16(),
        message,
    }))
}

/// Cap an error response body so a multi-KB HTML 404 from a CDN doesn't
/// flood the user's terminal. Shared with `cmd::run`; kept here so it can
/// be reused by other call sites.
pub fn truncate_error_body(s: &str) -> String {
    const MAX: usize = 200;
    let trimmed = s.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let mut t = trimmed.chars().take(MAX).collect::<String>();
        t.push_str(" … (body truncated)");
        t
    }
}
