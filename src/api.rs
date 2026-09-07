//! Thin HTTP wrappers for the RunComfy public APIs.
//!
//! Endpoint layout (from docs.runcomfy.com):
//! - `https://model-api.runcomfy.net/v1`        — Model API (hosted catalog)
//! - `https://api.runcomfy.net/prod/v2`         — Serverless API (deployments) + balance
//! - `https://trainer-api.runcomfy.net/prod/v1` — Trainer API (datasets, LoRA jobs)
//! - `https://www.runcomfy.com/api/cli-auth`    — Device-code OAuth (web)
//!
//! Every base can be overridden through an environment variable so the
//! CLI can be pointed at staging / preview stacks without a rebuild.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Method, Response, StatusCode};
use serde_json::Value;

use crate::config;
use crate::error::CliError;
use crate::output;

pub const DEFAULT_MODEL_API_BASE: &str = "https://model-api.runcomfy.net/v1";
pub const DEFAULT_SERVERLESS_API_BASE: &str = "https://api.runcomfy.net/prod/v2";
pub const DEFAULT_TRAINER_API_BASE: &str = "https://trainer-api.runcomfy.net/prod/v1";
pub const DEFAULT_WEB_BASE: &str = "https://www.runcomfy.com";

fn base_from_env(var: &str, default: &str) -> String {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => default.to_string(),
    }
}

/// Model API base: `RUNCOMFY_MODEL_API_BASE` or the production default.
/// Used by `run` / `status` / `result` / `cancel` / `models`.
pub fn model_api_base() -> String {
    base_from_env("RUNCOMFY_MODEL_API_BASE", DEFAULT_MODEL_API_BASE)
}

/// Serverless API base: `RUNCOMFY_SERVERLESS_API_BASE` or the production
/// default. Used by `deployments` and `balance`.
pub fn serverless_api_base() -> String {
    base_from_env("RUNCOMFY_SERVERLESS_API_BASE", DEFAULT_SERVERLESS_API_BASE)
}

/// Trainer API base: `RUNCOMFY_TRAINER_API_BASE` or the production
/// default. Used by `datasets` and `train`.
pub fn trainer_api_base() -> String {
    base_from_env("RUNCOMFY_TRAINER_API_BASE", DEFAULT_TRAINER_API_BASE)
}

/// Web base: `RUNCOMFY_WEB_BASE` or the production default. Used by
/// commands that don't take an explicit `--web-base` flag (e.g. `whoami`).
pub fn web_base() -> String {
    base_from_env("RUNCOMFY_WEB_BASE", DEFAULT_WEB_BASE)
}

fn user_agent() -> String {
    format!("runcomfy-cli/{}", env!("CARGO_PKG_VERSION"))
}

/// Client for API calls: 60s overall timeout, 10s connect timeout.
pub fn http() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(user_agent())
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .build()?)
}

/// Client for large transfers (asset downloads, dataset uploads): no
/// overall deadline, only a connect timeout and a read-inactivity
/// timeout, so a multi-hundred-MB file isn't cut off mid-stream.
pub fn http_long() -> Result<Client> {
    Ok(http_long_builder()?.build()?)
}

/// The [`http_long`] configuration before `build()`, so a caller can layer
/// on its own policy — the downloader adds a redirect check.
pub fn http_long_builder() -> Result<reqwest::ClientBuilder> {
    Ok(Client::builder()
        .user_agent(user_agent())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(120)))
}

/// Read the saved bearer token, error out if missing.
pub fn require_token() -> Result<String> {
    let tok = config::load_token()?.ok_or(CliError::NotAuthenticated)?;
    Ok(tok.access_token)
}

/// Join a base URL and a path with exactly one slash between them.
pub fn url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Authenticated JSON request against a RunComfy API.
///
/// Sends `method <base>/<path>?<query>` with the bearer token and an
/// optional JSON body, and returns the parsed response. Empty 2xx bodies
/// become `Value::Null`; non-JSON 2xx bodies (the instance proxy can
/// return plain text) become `Value::String`. Non-2xx responses map to
/// [`CliError`] so exit codes stay meaningful.
pub async fn request(
    method: Method,
    base: &str,
    path: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
) -> Result<Value> {
    request_inner(method, base, path, query, body, false).await
}

/// Same as [`request`], but a non-JSON 2xx body comes back as a string
/// instead of an error. Only the ComfyUI instance proxy needs this: every
/// other endpoint must return JSON, and quietly accepting text there would
/// let a malformed response read as an empty list or a successful delete.
pub async fn request_allow_text(
    method: Method,
    base: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    request_inner(method, base, path, &[], body, true).await
}

async fn request_inner(
    method: Method,
    base: &str,
    path: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
    allow_text: bool,
) -> Result<Value> {
    let client = http()?;
    let token = require_token()?;
    let full = url(base, path);
    let label = format!("{} {}", method, path);

    let mut req = client.request(method, &full).bearer_auth(&token);
    if !query.is_empty() {
        req = req.query(query);
    }
    if let Some(b) = body {
        req = req.json(b);
    }
    output::verbose(format!("{} {}", label, full));

    let resp = req
        .send()
        .await
        .with_context(|| format!("{} ({})", label, full))?;
    read_body(resp, &label, allow_text).await
}

/// Turn a response into JSON, or into a classified error. `label` names
/// the call in messages, e.g. `GET /deployments/{id}` or `status`.
pub async fn read_json_value(resp: Response, label: &str) -> Result<Value> {
    read_body(resp, label, false).await
}

async fn read_body(resp: Response, label: &str, allow_text: bool) -> Result<Value> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .with_context(|| format!("read response body ({})", label))?;

    if status.is_success() {
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        return match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok(v),
            Err(_) if allow_text => Ok(Value::String(text)),
            Err(e) => Err(anyhow!(
                "{}: expected JSON, got {}: {}",
                label,
                e,
                truncate_error_body(&text)
            )),
        };
    }
    Err(api_error(status, &text, label))
}

/// Build the error for a non-2xx response.
pub fn api_error(status: StatusCode, body: &str, label: &str) -> anyhow::Error {
    let code = status.as_u16();
    if code == 401 {
        return CliError::TokenRejected.into();
    }
    let detail = if body.trim().is_empty() {
        status.canonical_reason().unwrap_or("unknown").to_string()
    } else {
        truncate_error_body(body)
    };
    let message = match code {
        403 => format!(
            "{}: forbidden — the token may not have access to this resource: {}",
            label, detail
        ),
        404 => format!("{}: not found: {}", label, detail),
        429 => format!("{}: rate limited; retry in a moment: {}", label, detail),
        _ => format!("{}: {}", label, detail),
    };
    anyhow!(CliError::Api {
        status: code,
        message,
    })
}

/// Cap an error response body so a multi-KB HTML 404 from a CDN doesn't
/// flood the user's terminal.
pub fn truncate_error_body(s: &str) -> String {
    const MAX: usize = 300;
    let trimmed = s.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let mut t = trimmed.chars().take(MAX).collect::<String>();
        t.push_str(" … (body truncated)");
        t
    }
}
