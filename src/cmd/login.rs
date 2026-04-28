use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::{http, DEFAULT_WEB_BASE};
use crate::config::{self, Token};
use crate::error::CliError;
use crate::output;

/// Response from POST /api/cli-auth/start
#[derive(Debug, Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    verify_url: String,
    expires_in: u64,
    poll_interval: u64,
}

/// Response from POST /api/cli-auth/poll. The shape varies by status:
///   - { "status": "pending" }
///   - { "status": "authorized", "access_token": "..." }
///   - { "status": "denied" } | { "status": "expired" }
#[derive(Debug, Deserialize)]
struct PollResponse {
    status: String,
    access_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct PollRequest<'a> {
    device_code: &'a str,
}

/// `runcomfy login` — start a device-code OAuth flow against the RunComfy
/// web auth endpoints.
///
/// Flow:
///   1. POST {web_base}/api/cli-auth/start     → device_code, user_code, verify_url
///   2. open(verify_url) in the user's browser
///   3. poll {web_base}/api/cli-auth/poll every `poll_interval` seconds
///   4. on `authorized`, persist the access_token via config::save_token
pub async fn run(web_base: Option<String>) -> Result<()> {
    let base = web_base.unwrap_or_else(|| DEFAULT_WEB_BASE.to_string());
    let client = http()?;

    // Step 1: start the session.
    let start_url = format!("{}/api/cli-auth/start", base.trim_end_matches('/'));
    let start: StartResponse = client
        .post(&start_url)
        .send()
        .await
        .with_context(|| format!("POST {}", start_url))?
        .error_for_status()
        .context("cli-auth/start returned non-2xx")?
        .json()
        .await
        .context("parse cli-auth/start response")?;

    output::progress("🔑", "auth", "Opening browser to authorize this device");
    output::detail(format!(
        "If your browser doesn't open, visit: {}",
        start.verify_url
    ));
    output::detail(format!("Confirm the code: {}", start.user_code));

    if let Err(e) = open::that_detached(&start.verify_url) {
        output::detail(format!(
            "(could not auto-open browser: {}; open the URL manually)",
            e
        ));
    }

    // Step 2 & 3: poll until authorized / denied / expired / timeout.
    let poll_url = format!("{}/api/cli-auth/poll", base.trim_end_matches('/'));
    let interval = Duration::from_secs(start.poll_interval.max(1));
    let deadline = Instant::now() + Duration::from_secs(start.expires_in);

    output::progress("⏳", "wait", "Waiting for authorization (Ctrl-C to abort)...");

    let mut sigint = sigint_stream();

    loop {
        if Instant::now() >= deadline {
            return Err(anyhow!(CliError::AuthTimeout(start.expires_in)));
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = sigint.recv() => {
                return Err(anyhow!("login aborted by user"));
            }
        }

        let resp = client
            .post(&poll_url)
            .json(&PollRequest {
                device_code: &start.device_code,
            })
            .send()
            .await
            .with_context(|| format!("POST {}", poll_url))?;

        // 410 Gone is used for denied/expired with a JSON body; parse first.
        let body: PollResponse = resp
            .json()
            .await
            .context("parse cli-auth/poll response")?;

        match body.status.as_str() {
            "pending" => continue,
            "authorized" => {
                let access_token = body.access_token.ok_or_else(|| {
                    anyhow!("server returned authorized status without access_token")
                })?;
                let token = Token {
                    access_token,
                    refresh_token: None,
                    expires_at: None,
                    user_email: None,
                };
                config::save_token(&token).context("save token")?;
                output::progress("✅", "ok", "Authorized; token saved");
                return Ok(());
            }
            "denied" => return Err(anyhow!(CliError::AuthDenied)),
            "expired" => {
                return Err(anyhow!(CliError::AuthTimeout(start.expires_in)));
            }
            other => return Err(anyhow!("unexpected poll status `{}`", other)),
        }
    }
}

pub async fn logout() -> Result<()> {
    config::clear_token()?;
    output::progress("✅", "ok", "Logged out");
    Ok(())
}

/// Cross-platform SIGINT receiver. Same shape as `cmd::run::sigint_stream`
/// (kept duplicated for now; consider extracting if a third caller appears).
fn sigint_stream() -> tokio::sync::mpsc::UnboundedReceiver<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(_) => return,
        };
        while sig.recv().await.is_some() {
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    #[cfg(not(unix))]
    tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    rx
}
