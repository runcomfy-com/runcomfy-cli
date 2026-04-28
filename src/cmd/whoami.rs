use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::api::{http, require_token, DEFAULT_WEB_BASE};
use crate::error::CliError;

#[derive(Debug, Deserialize)]
struct MeResponse {
    id: String,
    email: Option<String>,
    token_type: Option<String>,
    #[allow(dead_code)]
    token_created_at: Option<String>,
}

/// `runcomfy whoami` — call GET {web_base}/api/auth/me with the saved bearer
/// token and print the authenticated user.
pub async fn run() -> Result<()> {
    let token = require_token().map_err(|e| {
        // Surface a friendly hint instead of the raw not-authenticated error.
        if let Some(CliError::NotAuthenticated) = e.downcast_ref::<CliError>() {
            anyhow!("not signed in. Run `runcomfy login` first.")
        } else {
            e
        }
    })?;

    let url = format!("{}/api/auth/me", DEFAULT_WEB_BASE.trim_end_matches('/'));
    let client = http()?;

    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err(anyhow!(
            "token rejected — run `runcomfy login` to refresh"
        ));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {}: {}", status, body));
    }

    let me: MeResponse = resp.json().await.context("parse /api/auth/me response")?;

    println!(
        "📛 {}",
        me.email.as_deref().unwrap_or(&me.id)
    );
    if let Some(t) = me.token_type.as_deref() {
        println!("   token type: {}", t);
    }
    println!("   user id: {}", me.id);
    Ok(())
}
