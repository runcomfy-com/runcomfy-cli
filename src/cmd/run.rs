use std::fs;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::api::{http, model_api_base, require_token};
use crate::error::CliError;

#[derive(Debug, Deserialize)]
struct SubmitResponse {
    request_id: String,
    #[allow(dead_code)]
    status_url: Option<String>,
    result_url: Option<String>,
    #[allow(dead_code)]
    cancel_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    #[allow(dead_code)]
    request_id: String,
    status: String,
    queue_position: Option<i64>,
    #[allow(dead_code)]
    status_url: Option<String>,
    result_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResultResponse {
    #[allow(dead_code)]
    request_id: String,
    status: String,
    output: Option<Value>,
    #[allow(dead_code)]
    created_at: Option<String>,
    #[allow(dead_code)]
    finished_at: Option<String>,
    error: Option<Value>,
}

/// `runcomfy run <model_id>` — submit a Model API request, optionally poll
/// until terminal, and print the result.
///
/// Endpoints:
///   POST {model_api_base}/models/{model_id}     → request_id
///   GET  {model_api_base}/requests/{id}/status  → in_queue / in_progress / completed / cancelled
///   GET  {model_api_base}/requests/{id}/result  → status + output (URLs)
pub async fn run(
    model_id: String,
    input: Option<String>,
    input_file: Option<String>,
    wait: bool,
    poll_secs: u64,
) -> Result<()> {
    if !model_id.contains('/') {
        bail!(
            "`{}` doesn't look like a model_id (expected slash-separated, e.g. \
             `blackforestlabs/flux-1-kontext/pro/edit`). \
             Find the model_id on its page at runcomfy.com/models.",
            model_id
        );
    }

    let body = parse_input(input.as_deref(), input_file.as_deref())?;

    let token = require_token().map_err(friendlier_auth_error)?;
    let client = http()?;
    let base = model_api_base();

    // 1. Submit
    let submit_url = format!("{}/models/{}", base.trim_end_matches('/'), model_id);
    eprintln!("⏳ Submitting request to {}", model_id);
    let submit_resp = client
        .post(&submit_url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {}", submit_url))?;

    let submit: SubmitResponse = read_json(submit_resp, "submit").await?;
    eprintln!("   request_id: {}", submit.request_id);

    if !wait {
        // Non-waiting mode: emit a stable JSON line so downstream tools can parse.
        println!(
            "{}",
            serde_json::json!({
                "request_id": submit.request_id,
                "wait": false
            })
        );
        return Ok(());
    }

    // 2. Poll
    let status_url = format!(
        "{}/requests/{}/status",
        base.trim_end_matches('/'),
        submit.request_id
    );
    let result_url = submit.result_url.unwrap_or_else(|| {
        format!(
            "{}/requests/{}/result",
            base.trim_end_matches('/'),
            submit.request_id
        )
    });

    eprintln!("⏳ Polling status (every {}s)...", poll_secs);
    let interval = Duration::from_secs(poll_secs.max(1));
    let mut last_state: Option<String> = None;

    loop {
        tokio::time::sleep(interval).await;
        let resp = client
            .get(&status_url)
            .bearer_auth(&token)
            .send()
            .await
            .with_context(|| format!("GET {}", status_url))?;
        let st: StatusResponse = read_json(resp, "status").await?;

        if Some(&st.status) != last_state.as_ref() {
            match st.status.as_str() {
                "in_queue" => {
                    if let Some(pos) = st.queue_position {
                        eprintln!("   in_queue  (position {})", pos);
                    } else {
                        eprintln!("   in_queue");
                    }
                }
                other => eprintln!("   {}", other),
            }
            last_state = Some(st.status.clone());
        }

        match st.status.as_str() {
            "in_queue" | "in_progress" => continue,
            "completed" | "succeeded" | "failed" | "cancelled" => break,
            other => bail!("unexpected status `{}`", other),
        }
    }

    // 3. Fetch result
    let result_resp = client
        .get(&result_url)
        .bearer_auth(&token)
        .send()
        .await
        .with_context(|| format!("GET {}", result_url))?;
    let result: ResultResponse = read_json(result_resp, "result").await?;

    match result.status.as_str() {
        "succeeded" | "completed" => {
            eprintln!("✅ {}", result.status);
            // Pretty-print output. Most models return a flat object of
            // URL strings or arrays of URL strings; show the full JSON
            // so callers (humans + agents) get exactly what the API gave.
            match result.output {
                Some(out) => println!("{}", serde_json::to_string_pretty(&out)?),
                None => println!("{{}}"),
            }
            Ok(())
        }
        "cancelled" => Err(anyhow!("request cancelled")),
        "failed" => {
            let detail = result
                .error
                .map(|e| serde_json::to_string(&e).unwrap_or_default())
                .unwrap_or_else(|| "(no detail)".into());
            Err(anyhow!("request failed: {}", detail))
        }
        other => Err(anyhow!("unexpected terminal status `{}`", other)),
    }
}

fn parse_input(inline: Option<&str>, file: Option<&str>) -> Result<Value> {
    let raw = match (inline, file) {
        (Some(_), Some(_)) => bail!("pass either --input or --input-file, not both"),
        (Some(s), None) => s.to_string(),
        (None, Some(p)) => fs::read_to_string(p)
            .with_context(|| format!("read input file {}", p))?,
        (None, None) => "{}".to_string(),
    };
    serde_json::from_str(&raw)
        .map_err(|e| anyhow!(CliError::InvalidInput(format!("{}", e))))
}

async fn read_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    label: &str,
) -> Result<T> {
    let status = resp.status();
    if status.is_success() {
        return resp
            .json::<T>()
            .await
            .with_context(|| format!("parse {} response", label));
    }
    if status.as_u16() == 401 {
        return Err(anyhow!(
            "token rejected by Model API ({}) — run `runcomfy login` to refresh",
            label
        ));
    }
    let body = resp.text().await.unwrap_or_default();
    Err(anyhow!(
        "{} returned HTTP {}: {}",
        label,
        status.as_u16(),
        if body.is_empty() {
            status.canonical_reason().unwrap_or("unknown").to_string()
        } else {
            body
        }
    ))
}

fn friendlier_auth_error(e: anyhow::Error) -> anyhow::Error {
    if let Some(CliError::NotAuthenticated) = e.downcast_ref::<CliError>() {
        anyhow!("not signed in. Run `runcomfy login` first.")
    } else {
        e
    }
}
