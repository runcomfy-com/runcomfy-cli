//! Generic "wait until the remote job reaches a terminal state" loop,
//! shared by `run`, `deployments run`, `train submit --wait` and
//! `datasets upload --wait`.
//!
//! The loop polls `status_url` every `interval`, prints a line whenever
//! the human description of the status changes, and returns the final
//! status payload. Ctrl-C either cancels the remote job (when the caller
//! supplies a `cancel_url`, as inference commands do — an abandoned GPU
//! request would otherwise keep billing) or merely stops watching (when
//! `cancel_url` is `None`, as for training jobs and dataset validation,
//! where hours of work must not be thrown away by an accidental Ctrl-C).

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::Value;

use crate::api;
use crate::error::CliError;
use crate::output;
use crate::signal::sigint_stream;

pub struct Watch<'a> {
    /// Absolute URL to GET for the status payload.
    pub status_url: &'a str,
    /// When `Some`, Ctrl-C POSTs here to cancel the remote job before
    /// exiting. When `None`, Ctrl-C only stops watching.
    pub cancel_url: Option<&'a str>,
    pub interval: Duration,
    /// Give up waiting (the remote job keeps running) after this long.
    pub timeout: Option<Duration>,
    /// Human noun for messages, e.g. `request req_xxx` / `training job abc`.
    pub what: String,
    /// Hint printed when the CLI stops watching without cancelling, e.g.
    /// "check with `runcomfy train status abc`".
    pub detach_hint: String,
}

/// Poll until `is_terminal(payload)` is true. Returns that payload.
pub async fn wait_terminal<T, D>(
    client: &Client,
    token: &str,
    watch: Watch<'_>,
    is_terminal: T,
    describe: D,
) -> Result<Value>
where
    T: Fn(&Value) -> bool,
    D: Fn(&Value) -> String,
{
    let started = Instant::now();
    let mut sigint = sigint_stream();
    let mut last_desc: Option<String> = None;
    let mut first = true;

    loop {
        if !first {
            tokio::select! {
                _ = tokio::time::sleep(watch.interval) => {}
                Some(_) = sigint.recv() => {
                    return handle_interrupt(client, token, &watch).await;
                }
            }
        }
        first = false;

        let resp = client
            .get(watch.status_url)
            .bearer_auth(token)
            .send()
            .await
            .with_context(|| format!("GET {}", watch.status_url))?;
        let payload = api::read_json_value(resp, "status").await?;

        let desc = describe(&payload);
        if last_desc.as_deref() != Some(desc.as_str()) {
            output::detail(&desc);
            last_desc = Some(desc);
        }

        if is_terminal(&payload) {
            return Ok(payload);
        }

        if let Some(t) = watch.timeout {
            if started.elapsed() >= t {
                return Err(anyhow!(CliError::WaitTimeout {
                    what: watch.what.clone(),
                    secs: t.as_secs(),
                    hint: watch.detach_hint.clone(),
                }));
            }
        }
    }
}

async fn handle_interrupt(client: &Client, token: &str, watch: &Watch<'_>) -> Result<Value> {
    match watch.cancel_url {
        Some(cancel_url) => {
            output::progress(
                "⏹",
                "cancel",
                format!("SIGINT received; cancelling {}", watch.what),
            );
            match client.post(cancel_url).bearer_auth(token).send().await {
                Ok(resp) if resp.status().is_success() => {
                    // The Model API only cancels queued requests and answers
                    // `not_cancellable` once a run is in progress; say so
                    // instead of implying the GPU work stopped.
                    let body: Value = resp.json().await.unwrap_or(Value::Null);
                    let outcome = body.get("outcome").and_then(Value::as_str).unwrap_or("");
                    let status = body.get("status").and_then(Value::as_str).unwrap_or("");
                    if outcome == "not_cancellable" {
                        output::detail(format!(
                            "{} could not be cancelled (status: {}); it will run to completion — {}",
                            watch.what, status, watch.detach_hint
                        ));
                    } else {
                        output::detail(format!(
                            "cancel accepted ({})",
                            if status.is_empty() { outcome } else { status }
                        ));
                    }
                }
                Ok(resp) => {
                    let code = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    eprintln!(
                        "warning: remote cancel returned HTTP {} for {}; {}. body: {}",
                        code,
                        watch.what,
                        watch.detach_hint,
                        api::truncate_error_body(&body)
                    );
                }
                Err(e) => {
                    eprintln!(
                        "warning: failed to send cancel for {}: {}; {}",
                        watch.what, e, watch.detach_hint
                    );
                }
            }
            Err(anyhow!("cancelled by user"))
        }
        None => {
            output::progress(
                "⏹",
                "detach",
                format!(
                    "SIGINT received; {} keeps running remotely — {}",
                    watch.what, watch.detach_hint
                ),
            );
            Err(anyhow!("stopped waiting (interrupted by user)"))
        }
    }
}

/// `status` field of a payload, lower-cased, or `""` when absent.
pub fn status_of(v: &Value) -> String {
    v.get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Terminal check for Model API / Serverless inference requests.
pub fn inference_is_terminal(v: &Value) -> bool {
    matches!(
        status_of(v).as_str(),
        "completed" | "succeeded" | "failed" | "cancelled" | "canceled"
    )
}

/// One-line description of an inference status payload, e.g.
/// `in_queue (position 3)` or `in_progress (instance 1697cb1a-...)`.
pub fn describe_inference(v: &Value) -> String {
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("(no status)")
        .to_string();
    if let Some(pos) = v.get("queue_position").and_then(Value::as_i64) {
        return format!("{} (position {})", status, pos);
    }
    if let Some(inst) = v.get("instance_id").and_then(Value::as_str) {
        return format!("{} (instance {})", status, inst);
    }
    status
}
