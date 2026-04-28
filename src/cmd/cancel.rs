use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::api::{http, model_api_base, require_token};
use crate::error::CliError;

#[derive(Debug, Deserialize)]
struct CancelResponse {
    request_id: String,
    status: Option<String>,
    outcome: Option<String>,
}

/// `runcomfy cancel <request_id>` — call
/// `POST {model_api_base}/requests/{request_id}/cancel`.
///
/// Per the Model API spec the server returns 202 Accepted with
/// `{ outcome: "cancelled" | "not_cancellable" }`.
pub async fn run(request_id: String) -> Result<()> {
    let token = require_token().map_err(|e| {
        if let Some(CliError::NotAuthenticated) = e.downcast_ref::<CliError>() {
            anyhow!("not signed in. Run `runcomfy login` first.")
        } else {
            e
        }
    })?;

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
        return Err(anyhow!(
            "token rejected — run `runcomfy login` to refresh"
        ));
    }
    // The spec uses 202 Accepted; treat any 2xx as success.
    if !code.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {}: {}", code, body));
    }

    let r: CancelResponse = resp.json().await.context("parse cancel response")?;
    let outcome = r.outcome.as_deref().unwrap_or("(no outcome)");
    let status = r.status.as_deref().unwrap_or("(no status)");
    match outcome {
        "cancelled" => println!("✅ Cancelled {} (final status: {})", r.request_id, status),
        "not_cancellable" => println!(
            "ℹ  {} is already terminal ({}); cancel is a no-op",
            r.request_id, status
        ),
        other => println!(
            "request_id: {}\noutcome:    {}\nstatus:     {}",
            r.request_id, other, status
        ),
    }
    Ok(())
}
