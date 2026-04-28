use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::api::{http, model_api_base, require_token};
use crate::error::CliError;
use crate::output;

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
    output_dir: String,
    download: bool,
) -> Result<()> {
    if !model_id.contains('/') {
        bail!(CliError::InvalidInput(format!(
            "`{}` is not a valid model_id (expected slash-separated, e.g. \
             `blackforestlabs/flux-1-kontext/pro/edit`). \
             Find model_ids at https://www.runcomfy.com/models",
            model_id
        )));
    }

    let body = parse_input(input.as_deref(), input_file.as_deref())?;

    let token = require_token().map_err(friendlier_auth_error)?;
    let client = http()?;
    let base = model_api_base();

    output::verbose(format!("Model API base: {}", base));

    // 1. Submit
    let submit_url = format!("{}/models/{}", base.trim_end_matches('/'), model_id);
    output::progress("⏳", "submit", format!("Submitting request to {}", model_id));
    let submit_resp = client
        .post(&submit_url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {}", submit_url))?;

    let submit: SubmitResponse = read_json_actionable(submit_resp, "submit", Some(&model_id)).await?;
    output::detail(format!("request_id: {}", submit.request_id));

    if !wait {
        // Single-line stable JSON for non-waiting mode (script-friendly).
        output::payload(&serde_json::json!({
            "request_id": submit.request_id,
            "wait": false
        }))?;
        return Ok(());
    }

    // 2. Poll. Listen for SIGINT in parallel so Ctrl-C cancels the remote.
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
    let cancel_url = format!(
        "{}/requests/{}/cancel",
        base.trim_end_matches('/'),
        submit.request_id
    );

    output::progress("⏳", "poll", format!("Polling status (every {}s)...", poll_secs));
    let interval = Duration::from_secs(poll_secs.max(1));

    let mut sigint = sigint_stream();
    let mut last_state: Option<String> = None;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = sigint.recv() => {
                output::progress("⏹", "cancel", "SIGINT received; cancelling remote request");
                let _ = client.post(&cancel_url).bearer_auth(&token).send().await;
                return Err(anyhow!("cancelled by user"));
            }
        }

        let resp = client
            .get(&status_url)
            .bearer_auth(&token)
            .send()
            .await
            .with_context(|| format!("GET {}", status_url))?;
        let st: StatusResponse = read_json_actionable(resp, "status", None).await?;

        if Some(&st.status) != last_state.as_ref() {
            match st.status.as_str() {
                "in_queue" => match st.queue_position {
                    Some(pos) => output::detail(format!("in_queue (position {})", pos)),
                    None => output::detail("in_queue"),
                },
                other => output::detail(other),
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
    let result: ResultResponse = read_json_actionable(result_resp, "result", None).await?;

    match result.status.as_str() {
        "succeeded" | "completed" => {
            output::progress("✅", "ok", &result.status);
            let out = result.output.unwrap_or_else(|| Value::Object(Default::default()));
            output::payload(&out)?;

            if download {
                let mut urls = Vec::new();
                collect_urls(&out, &mut urls);
                if !urls.is_empty() {
                    let dir = PathBuf::from(&output_dir);
                    if !dir.exists() {
                        std::fs::create_dir_all(&dir)
                            .with_context(|| format!("create output dir {}", dir.display()))?;
                    }
                    output::progress(
                        "📥",
                        "download",
                        format!("Downloading {} file(s) to {}", urls.len(), dir.display()),
                    );
                    for url in urls {
                        match download_file(&client, &url, &dir).await {
                            Ok(path) => output::detail(path.display().to_string()),
                            Err(e) => output::detail(format!("⚠ {}: {}", url, e)),
                        }
                    }
                }
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
        (None, Some("-")) => {
            // Read JSON from stdin so callers can pipe it in.
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            buf
        }
        (None, Some(p)) => fs::read_to_string(p)
            .with_context(|| format!("read input file {}", p))?,
        (None, None) => "{}".to_string(),
    };
    serde_json::from_str(&raw)
        .map_err(|e| anyhow!(CliError::InvalidInput(format!("{}", e))))
}

/// Single-shot SIGINT receiver (uses tokio's signal crate). Returns a
/// stream that fires once per Ctrl-C.
fn sigint_stream() -> tokio::sync::mpsc::UnboundedReceiver<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut sig = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        {
            Ok(s) => s,
            Err(_) => return,
        };
        while sig.recv().await.is_some() {
            if tx.send(()).is_err() {
                break;
            }
        }
    });
    rx
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
                "model `{}` not found. Verify the model_id at \
                 https://www.runcomfy.com/models",
                m
            ),
            None => format!("{} not found", label),
        };
        return Err(anyhow!(CliError::Api { status: code, message: hint }));
    }
    if code == 422 || code == 400 {
        let hint = match model_id {
            Some(m) => format!(
                "input did not match the model's schema. Check the Input \
                 schema at https://www.runcomfy.com/models/{}/api — server said: {}",
                m, body
            ),
            None => format!("{} HTTP 4xx: {}", label, body),
        };
        return Err(anyhow!(CliError::Api { status: code, message: hint }));
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
            body
        },
    }))
}

fn friendlier_auth_error(e: anyhow::Error) -> anyhow::Error {
    if let Some(CliError::NotAuthenticated) = e.downcast_ref::<CliError>() {
        anyhow!(CliError::NotAuthenticated)
    } else {
        e
    }
}

/// Recursively walk the model output JSON and collect every http(s) URL.
fn collect_urls(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            if s.starts_with("http://") || s.starts_with("https://") {
                out.push(s.clone());
            }
        }
        Value::Array(a) => {
            for x in a {
                collect_urls(x, out);
            }
        }
        Value::Object(o) => {
            for (_, x) in o {
                collect_urls(x, out);
            }
        }
        _ => {}
    }
}

fn filename_from_url(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or(url);
    let last = no_query.rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        "output".to_string()
    } else {
        last.to_string()
    }
}

fn dedup_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let ext = p.extension().and_then(|e| e.to_str()).map(|s| s.to_string());
    for i in 1..1000 {
        let new_name = match &ext {
            Some(e) => format!("{}-{}.{}", stem, i, e),
            None => format!("{}-{}", stem, i),
        };
        let p = dir.join(&new_name);
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{}-many", stem))
}

async fn download_file(client: &Client, url: &str, dir: &Path) -> Result<PathBuf> {
    let name = filename_from_url(url);
    let path = dedup_path(dir, &name);
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?
        .error_for_status()
        .with_context(|| format!("download {}", url))?;
    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("read body {}", url))?;
    tokio::fs::write(&path, bytes)
        .await
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}
