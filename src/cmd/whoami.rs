use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::api::{http, require_token, truncate_error_body, web_base};
use crate::error::CliError;
use crate::output;

#[derive(Debug, Deserialize, serde::Serialize)]
struct MeResponse {
    id: String,
    email: Option<String>,
    display_name: Option<String>,
    username: Option<String>,
    token_type: Option<String>,
    token_created_at: Option<String>,
}

/// `runcomfy whoami` — call GET {web_base}/api/auth/me with the saved bearer
/// token and print the authenticated user.
pub async fn run() -> Result<()> {
    let token = require_token()?;

    let url = format!("{}/api/auth/me", web_base().trim_end_matches('/'));
    let client = http()?;

    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err(anyhow!(CliError::TokenRejected));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(CliError::Api {
            status: status.as_u16(),
            message: if body.is_empty() {
                status.canonical_reason().unwrap_or("unknown").to_string()
            } else {
                truncate_error_body(&body)
            },
        }));
    }

    let me: MeResponse = resp.json().await.context("parse /api/auth/me response")?;

    if output::is_json() {
        output::payload(&serde_json::to_value(&me)?)?;
    } else {
        let label = me
            .email
            .as_deref()
            .or(me.display_name.as_deref())
            .or(me.username.as_deref())
            .unwrap_or(&me.id);
        output::progress("📛", "user", label);
        if let Some(t) = me.token_type.as_deref() {
            output::detail(format!("token type: {}", t));
        }
        output::detail(format!("user id: {}", me.id));
    }
    Ok(())
}
