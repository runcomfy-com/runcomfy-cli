pub mod balance;
pub mod cancel;
pub mod datasets;
pub mod deployments;
pub mod login;
pub mod models;
pub mod result;
pub mod run;
pub mod status;
pub mod train;
pub mod whoami;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::cli::DownloadOpts;
use crate::download;
use crate::error::CliError;
use crate::output;

/// Render a JSON field for a table cell: strings verbatim, numbers and
/// booleans via Display, arrays joined with `,`, null / missing as `-`.
pub fn field(v: &Value, key: &str) -> String {
    match v.get(key) {
        None | Some(Value::Null) => "-".to_string(),
        Some(Value::String(s)) if s.is_empty() => "-".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) if a.is_empty() => "-".to_string(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(other) => other.to_string(),
    }
}

/// Require a string field in an API response.
pub fn require_str<'a>(v: &'a Value, key: &str, label: &str) -> Result<&'a str> {
    v.get(key).and_then(Value::as_str).ok_or_else(|| {
        anyhow!(
            "{} response is missing `{}`: {}",
            label,
            key,
            serde_json::to_string(v).unwrap_or_default()
        )
    })
}

/// Confirmation gate for destructive commands: `--yes` skips it, a TTY
/// prompt asks, and a non-interactive session without `--yes` is refused
/// (exit 64) rather than silently proceeding.
pub fn confirm_or_bail(yes: bool, action: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    match output::confirm(&format!("{}?", action))? {
        Some(true) => Ok(()),
        Some(false) => bail!(CliError::Aborted),
        None => bail!(CliError::NeedsConfirmation(action.to_string())),
    }
}

/// Print a cancel response (`{request_id, status, outcome}`) the same way
/// for Model API and Serverless requests.
pub fn print_cancel_outcome(v: &Value, id: &str) -> Result<()> {
    if output::is_json() {
        return output::payload(v);
    }
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("(no status)");
    // The Model API answers `{"outcome": "cancelled" | "not_cancellable", ...}`.
    // Serverless answers `{"status": "cancellation_requested"}` with no
    // `outcome`: the cancel has only been accepted and the request may still
    // be running, so say exactly that instead of claiming a final state.
    let outcome = match v.get("outcome").and_then(Value::as_str) {
        Some(o) => o,
        None if status == "cancellation_requested" => "cancellation_requested",
        None if matches!(status, "cancelled" | "canceled") => "cancelled",
        None => "(no outcome)",
    };
    match outcome {
        "cancellation_requested" => output::progress(
            "⏳",
            "requested",
            format!(
                "Cancellation requested for {}; it may still be running — poll its status to confirm it reaches canceled",
                id
            ),
        ),
        "cancelled" | "canceled" => output::progress(
            "✅",
            "ok",
            format!("Cancelled {} (final status: {})", id, status),
        ),
        "not_cancellable" => output::progress(
            "ℹ",
            "info",
            format!("{} is already terminal ({}); cancel is a no-op", id, status),
        ),
        other => {
            output::kv(&[
                ("request_id", id.to_string()),
                ("outcome", other.to_string()),
                ("status", status.to_string()),
            ]);
        }
    }
    Ok(())
}

/// Finish an inference result payload (Model API or Serverless): print
/// it, download generated assets on success, and turn `failed` /
/// `cancelled` into non-zero exits.
///
/// `output_keys` names the sub-object that holds asset URLs (`output`
/// for the Model API, `outputs` for Serverless ComfyUI deployments,
/// `output` for Serverless LoRA deployments); only that sub-object is
/// scanned for downloads. When `output_only` is set, success prints just
/// that sub-object (the `run` contract); otherwise the whole record.
pub async fn finish_inference_result(
    result: &Value,
    output_keys: &[&str],
    download: &DownloadOpts,
    output_only: bool,
) -> Result<()> {
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let out = output_keys.iter().find_map(|k| result.get(*k));

    match status.as_str() {
        "succeeded" | "completed" => {
            output::progress("✅", "ok", &status);
            if output_only {
                let empty = Value::Object(Default::default());
                output::payload(out.unwrap_or(&empty))?;
            } else {
                output::payload(result)?;
            }
            if !download.no_download {
                if let Some(o) = out {
                    download::download_outputs(o, &download.output_dir).await?;
                }
            }
            Ok(())
        }
        "cancelled" | "canceled" => {
            if !output_only {
                output::payload(result)?;
            }
            Err(anyhow!("request cancelled"))
        }
        "failed" => {
            if !output_only {
                output::payload(result)?;
            }
            let detail = result
                .get("error")
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_else(|| "(no detail)".into());
            Err(anyhow!("request failed: {}", detail))
        }
        other => {
            let shown = if other.is_empty() { "(unknown)" } else { other };
            output::progress("ℹ", "info", format!("not finished yet (status: {})", shown));
            output::payload(result)?;
            Ok(())
        }
    }
}
