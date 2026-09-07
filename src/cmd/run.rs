//! `runcomfy run <model_id>` — submit a Model API request, optionally poll
//! until terminal, print the result, and download generated files.
//!
//! Endpoints:
//!   POST {model_api_base}/models/{model_id}     → request_id
//!   GET  {model_api_base}/requests/{id}/status  → in_queue / in_progress / completed / cancelled
//!   GET  {model_api_base}/requests/{id}/result  → status + output (URLs)

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::api::{self, http, model_api_base, require_token, truncate_error_body};
use crate::cli::DownloadOpts;
use crate::cmd;
use crate::error::CliError;
use crate::input;
use crate::output;
use crate::poll::{self, Watch};

#[derive(Debug, Deserialize)]
struct SubmitResponse {
    request_id: String,
    result_url: Option<String>,
}

pub async fn run(
    model_id: String,
    input: Option<String>,
    input_file: Option<String>,
    wait: bool,
    poll_secs: u64,
    download: DownloadOpts,
) -> Result<()> {
    if !model_id.contains('/') {
        bail!(CliError::InvalidInput(format!(
            "`{}` is not a valid model_id (expected slash-separated, e.g. \
             `blackforestlabs/flux-1-kontext/pro/edit`). \
             Find model_ids with `runcomfy models list` or at https://www.runcomfy.com/models",
            model_id
        )));
    }

    let body = input::json_arg(input.as_deref(), input_file.as_deref(), "input")?
        .unwrap_or_else(|| Value::Object(Default::default()));
    input::require_object(&body, "--input")?;

    let token = require_token()?;
    let client = http()?;
    let base = model_api_base();

    output::verbose(format!("Model API base: {}", base));

    // 1. Submit
    let submit_url = api::url(&base, &format!("models/{}", model_id));
    output::progress(
        "⏳",
        "submit",
        format!("Submitting request to {}", model_id),
    );
    let submit_resp = client
        .post(&submit_url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {}", submit_url))?;

    let submit: SubmitResponse =
        read_json_actionable(submit_resp, "submit", Some(&model_id)).await?;
    output::detail(format!("request_id: {}", submit.request_id));

    if !wait {
        // Single-line stable JSON for non-waiting mode (script-friendly).
        output::payload(&serde_json::json!({
            "request_id": submit.request_id,
            "wait": false
        }))?;
        return Ok(());
    }

    // 2. Poll. Ctrl-C cancels the remote request so an abandoned run
    //    doesn't keep billing.
    let status_url = api::url(&base, &format!("requests/{}/status", submit.request_id));
    let result_url = submit
        .result_url
        .unwrap_or_else(|| api::url(&base, &format!("requests/{}/result", submit.request_id)));
    let cancel_url = api::url(&base, &format!("requests/{}/cancel", submit.request_id));

    output::progress(
        "⏳",
        "poll",
        format!("Polling status (every {}s)...", poll_secs.max(1)),
    );
    poll::wait_terminal(
        &client,
        &token,
        Watch {
            status_url: &status_url,
            cancel_url: Some(&cancel_url),
            interval: Duration::from_secs(poll_secs.max(1)),
            timeout: None,
            what: format!("request {}", submit.request_id),
            detach_hint: format!(
                "`runcomfy cancel {0}` to retry, `runcomfy result {0}` to fetch it later",
                submit.request_id
            ),
        },
        poll::inference_is_terminal,
        poll::describe_inference,
    )
    .await?;

    // 3. Fetch result
    let result_resp = client
        .get(&result_url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("GET {}", result_url))?;
    let result: Value = read_json_actionable(result_resp, "result", None).await?;

    cmd::finish_inference_result(&result, &["output"], &download, true).await
}

/// Read a JSON response, return a friendly error on non-2xx with hints
/// specific to common Model API error cases.
async fn read_json_actionable<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    label: &str,
    model_id: Option<&str>,
) -> Result<T> {
    let status = resp.status();
    if status.is_success() {
        return resp
            .json::<T>()
            .await
            .with_context(|| format!("parse {} response", label));
    }
    let code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    if code == 401 || code == 403 {
        return Err(anyhow!(CliError::Api {
            status: code,
            message: format!(
                "auth failed ({}) — run `runcomfy login` to refresh, or set RUNCOMFY_TOKEN",
                label
            ),
        }));
    }
    if code == 404 {
        let hint = match model_id {
            Some(m) => format!(
                "model `{}` not found. Verify the model_id with `runcomfy models list` \
                 or at https://www.runcomfy.com/models",
                m
            ),
            None => format!("{} not found", label),
        };
        return Err(anyhow!(CliError::Api {
            status: code,
            message: hint
        }));
    }
    if code == 422 || code == 400 {
        let hint = match model_id {
            Some(m) => format!(
                "input did not match the model's schema. Check it with \
                 `runcomfy models get {}` — server said: {}",
                m,
                truncate_error_body(&body)
            ),
            None => format!("{} HTTP 4xx: {}", label, truncate_error_body(&body)),
        };
        return Err(anyhow!(CliError::Api {
            status: code,
            message: hint
        }));
    }
    if code == 429 {
        return Err(anyhow!(CliError::Api {
            status: 429,
            message: format!("rate limited; retry in a moment ({})", label),
        }));
    }
    Err(anyhow!(CliError::Api {
        status: code,
        message: if body.is_empty() {
            status.canonical_reason().unwrap_or("unknown").to_string()
        } else {
            truncate_error_body(&body)
        },
    }))
}
