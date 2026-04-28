use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::api::{http, model_api_base, require_token};
use crate::error::CliError;
use crate::output;

#[derive(Debug, Deserialize, serde::Serialize)]
struct StatusResponse {
    request_id: String,
    status: String,
    queue_position: Option<i64>,
    status_url: Option<String>,
    result_url: Option<String>,
}

/// `runcomfy status <request_id>` (alias: `runcomfy requests get <id>`)
pub async fn run(request_id: String) -> Result<()> {
    let token = require_token()?;
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
        return Err(anyhow!(CliError::AuthExpired));
    }
    if !code.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(CliError::Api {
            status: code.as_u16(),
            message: if body.is_empty() {
                code.canonical_reason().unwrap_or("unknown").to_string()
            } else {
                body
            },
        }));
    }

    let s: StatusResponse = resp.json().await.context("parse status response")?;

    if output::is_json() {
        // Machine-readable: stable JSON to stdout, no decoration.
        output::payload(&serde_json::to_value(&s)?)?;
    } else {
        // Human: aligned key/value
        println!("request_id: {}", s.request_id);
        println!("status:     {}", s.status);
        if let Some(p) = s.queue_position {
            println!("queue:      position {}", p);
        }
        if let Some(u) = &s.status_url {
            println!("status_url: {}", u);
        }
        if let Some(u) = &s.result_url {
            println!("result_url: {}", u);
        }
    }
    Ok(())
}
