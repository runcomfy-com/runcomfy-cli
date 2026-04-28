use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::api::{http, model_api_base, require_token, truncate_error_body};
use crate::error::CliError;
use crate::output;

#[derive(Debug, Deserialize, serde::Serialize)]
struct CancelResponse {
    request_id: String,
    status: Option<String>,
    outcome: Option<String>,
}

/// `runcomfy cancel <request_id>` (alias: `runcomfy requests cancel <id>`)
pub async fn run(request_id: String) -> Result<()> {
    let token = require_token()?;
    let url = format!(
        "{}/requests/{}/cancel",
        model_api_base().trim_end_matches('/'),
        request_id
    );
    let client = http()?;

    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("POST {}", url))?;

    let code = resp.status();
    if code.as_u16() == 401 {
        return Err(anyhow!(CliError::TokenRejected));
    }
    if !code.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(CliError::Api {
            status: code.as_u16(),
            message: if body.is_empty() {
                code.canonical_reason().unwrap_or("unknown").to_string()
            } else {
                truncate_error_body(&body)
            },
        }));
    }

    let r: CancelResponse = resp.json().await.context("parse cancel response")?;
    let outcome = r.outcome.as_deref().unwrap_or("(no outcome)");
    let status = r.status.as_deref().unwrap_or("(no status)");

    if output::is_json() {
        output::payload(&serde_json::to_value(&r)?)?;
    } else {
        match outcome {
            "cancelled" => output::progress(
                "✅",
                "ok",
                format!("Cancelled {} (final status: {})", r.request_id, status),
            ),
            "not_cancellable" => output::progress(
                "ℹ",
                "info",
                format!(
                    "{} is already terminal ({}); cancel is a no-op",
                    r.request_id, status
                ),
            ),
            other => {
                println!("request_id: {}", r.request_id);
                println!("outcome:    {}", other);
                println!("status:     {}", status);
            }
        }
    }
    Ok(())
}
