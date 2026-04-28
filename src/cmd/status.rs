use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::api::{http, model_api_base, require_token};
use crate::error::CliError;

#[derive(Debug, Deserialize)]
struct StatusResponse {
    request_id: String,
    status: String,
    queue_position: Option<i64>,
    status_url: Option<String>,
    result_url: Option<String>,
}

/// `runcomfy status <request_id>` — call
/// `GET {model_api_base}/requests/{request_id}/status`.
pub async fn run(request_id: String) -> Result<()> {
    let token = require_token().map_err(|e| {
        if let Some(CliError::NotAuthenticated) = e.downcast_ref::<CliError>() {
            anyhow!("not signed in. Run `runcomfy login` first.")
        } else {
            e
        }
    })?;

    let url = format!(
        "{}/requests/{}/status",
        model_api_base().trim_end_matches('/'),
        request_id
    );
    let client = http()?;

    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;

    let code = resp.status();
    if code.as_u16() == 401 {
        return Err(anyhow!(
            "token rejected — run `runcomfy login` to refresh"
        ));
    }
    if !code.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {}: {}", code, body));
    }

    let s: StatusResponse = resp.json().await.context("parse status response")?;
    println!("request_id: {}", s.request_id);
    println!("status:     {}", s.status);
    if let Some(p) = s.queue_position {
        println!("queue:      position {}", p);
    }
    if let Some(u) = s.status_url {
        println!("status_url: {}", u);
    }
    if let Some(u) = s.result_url {
        println!("result_url: {}", u);
    }
    Ok(())
}
