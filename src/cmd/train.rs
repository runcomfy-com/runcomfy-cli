//! `runcomfy train ...` — AI Toolkit (LoRA) training jobs (Trainer API).
//!
//! Endpoints (base `https://trainer-api.runcomfy.net/prod/v1`):
//!   POST /trainers/ai-toolkit/jobs
//!   GET  /trainers/ai-toolkit/jobs/{id}/status
//!   GET  /trainers/ai-toolkit/jobs/{id}/result
//!   POST /trainers/ai-toolkit/jobs/{id}/cancel
//!   POST /trainers/ai-toolkit/jobs/{id}/resume
//!   POST /trainers/ai-toolkit/jobs/{id}/edit

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::api;
use crate::cmd::field;
use crate::download;
use crate::error::CliError;
use crate::input;
use crate::output;
use crate::poll::{self, Watch};

pub struct SubmitArgs {
    pub config: String,
    pub gpu_type: String,
    pub gpu_count: Option<u32>,
    pub gpu_id: Option<String>,
    pub wait: bool,
    pub poll_secs: u64,
    pub timeout: Option<u64>,
}

fn job_path(job_id: &str, suffix: &str) -> String {
    format!("/trainers/ai-toolkit/jobs/{}{}", job_id, suffix)
}

fn read_config(path: &str) -> Result<String> {
    let yaml = input::read_text(path, "config")?;
    if yaml.trim().is_empty() {
        bail!(CliError::InvalidInput(format!(
            "config file {} is empty",
            path
        )));
    }
    Ok(yaml)
}

pub async fn submit(args: SubmitArgs) -> Result<()> {
    let yaml = read_config(&args.config)?;
    let mut body = Map::new();
    body.insert("config_file_format".into(), json!("yaml"));
    body.insert("config_file".into(), json!(yaml));
    body.insert("gpu_type".into(), json!(args.gpu_type));
    if let Some(n) = args.gpu_count {
        body.insert("gpu_count".into(), json!(n));
    }
    if let Some(id) = args.gpu_id.as_deref() {
        body.insert("gpu_id".into(), json!(id));
    }

    let base = api::trainer_api_base();
    let gpus = match args.gpu_count {
        Some(n) if n > 1 => format!("{} x{}", args.gpu_type, n),
        _ => args.gpu_type.clone(),
    };
    output::progress(
        "⏳",
        "submit",
        format!("Submitting training job ({})", gpus),
    );
    let sub = api::request(
        Method::POST,
        &base,
        "/trainers/ai-toolkit/jobs",
        &[],
        Some(&Value::Object(body)),
    )
    .await?;
    let job_id = sub
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anyhow!(
                "submit response is missing `id`: {}",
                serde_json::to_string(&sub).unwrap_or_default()
            )
        })?
        .to_string();
    output::detail(format!(
        "job_id: {} (name: {})",
        job_id,
        field(&sub, "name")
    ));

    if !args.wait {
        output::detail(format!(
            "Track with `runcomfy train status {}`; artifacts via `runcomfy train result {}`",
            job_id, job_id
        ));
        return output::payload(&sub);
    }

    let status_url = api::url(&base, &job_path(&job_id, "/status"));
    let client = api::http()?;
    let token = api::require_token()?;
    output::progress(
        "⏳",
        "poll",
        format!(
            "Waiting for training to finish (every {}s; Ctrl-C stops watching, not the job)...",
            args.poll_secs.max(1)
        ),
    );
    let final_status = poll::wait_terminal(
        &client,
        &token,
        Watch {
            status_url: &status_url,
            cancel_url: None,
            interval: Duration::from_secs(args.poll_secs.max(1)),
            timeout: args.timeout.map(Duration::from_secs),
            what: format!("training job {}", job_id),
            detach_hint: format!(
                "check with `runcomfy train status {}`; stop with `runcomfy train cancel {}`",
                job_id, job_id
            ),
        },
        training_is_terminal,
        describe_training,
    )
    .await?;

    let result = api::request(Method::GET, &base, &job_path(&job_id, "/result"), &[], None).await?;
    finish_training(&final_status, &result, &job_id)
}

/// What a `STOPPED` job actually is. The public API reports `STOPPED`
/// both for a job that ran to its final step and for one the platform
/// stopped early (spot preemption, server reclaimed), so completion has to
/// be *proven* from the step progress and the result's `error`. Anything
/// that can't be verified is treated as not finished, so automation never
/// consumes a partial checkpoint on the strength of a bare `STOPPED`.
#[derive(Debug, PartialEq, Eq)]
enum StoppedOutcome {
    /// Valid progress with `current_step >= total_steps` and no error.
    Finished,
    /// Progress shows the job stopped before its final step.
    StoppedEarly { current: i64, total: i64 },
    /// The result record carries an `error`.
    ErrorInResult(String),
    /// No usable progress (`progress` missing / not an object /
    /// `total_steps` 0), so completion cannot be verified.
    Unverified,
}

impl StoppedOutcome {
    fn describe(&self) -> String {
        match self {
            StoppedOutcome::Finished => "finished".to_string(),
            StoppedOutcome::StoppedEarly { current, total } => format!(
                "training stopped at step {}/{} before completing (preempted or reclaimed by the platform)",
                current, total
            ),
            StoppedOutcome::ErrorInResult(msg) => format!("the result carries an error: {}", msg),
            StoppedOutcome::Unverified => {
                "the API reported no step progress, so completion cannot be verified".to_string()
            }
        }
    }
}

fn classify_stopped(status: &Value, result: &Value) -> StoppedOutcome {
    if let Some(err) = result.get("error").filter(|e| !e.is_null()) {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| err.to_string());
        return StoppedOutcome::ErrorInResult(message);
    }
    let Some(p) = status.get("progress").filter(|p| p.is_object()) else {
        return StoppedOutcome::Unverified;
    };
    let current = p.get("current_step").and_then(Value::as_i64).unwrap_or(0);
    let total = p.get("total_steps").and_then(Value::as_i64).unwrap_or(0);
    if total <= 0 {
        return StoppedOutcome::Unverified;
    }
    if current >= total {
        StoppedOutcome::Finished
    } else {
        StoppedOutcome::StoppedEarly { current, total }
    }
}

/// Print a finished job's result record. Only a `STOPPED` job that reached
/// its final step with no error exits 0; an early stop exits 75 (resumable),
/// FAILED / CANCELED exit 1.
fn finish_training(status: &Value, result: &Value, job_id: &str) -> Result<()> {
    let st = poll::status_of(status);
    match st.as_str() {
        "stopped" => match classify_stopped(status, result) {
            StoppedOutcome::Finished => {
                output::progress(
                    "✅",
                    "ok",
                    format!(
                        "Training job {} finished (STOPPED at its final step)",
                        job_id
                    ),
                );
                output::payload(result)
            }
            outcome @ StoppedOutcome::StoppedEarly { .. } => {
                output::payload(result)?;
                Err(anyhow!(CliError::TrainingIncomplete {
                    job_id: job_id.to_string(),
                    detail: format!(
                        "{}; resume from the latest checkpoint with `runcomfy train resume {}`",
                        outcome.describe(),
                        job_id
                    ),
                }))
            }
            outcome => {
                output::payload(result)?;
                Err(anyhow!(CliError::TrainingIncomplete {
                    job_id: job_id.to_string(),
                    detail: format!(
                        "{}; inspect `runcomfy train status {}` and `runcomfy train result {}` before using its artifacts",
                        outcome.describe(),
                        job_id,
                        job_id
                    ),
                }))
            }
        },
        "failed" => {
            output::payload(result)?;
            let err = status
                .get("errors")
                .or_else(|| status.get("error"))
                .or_else(|| result.get("error"))
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_else(|| "(no detail)".into());
            Err(anyhow!("training job {} FAILED: {}", job_id, err))
        }
        "canceled" | "cancelled" => {
            output::payload(result)?;
            Err(anyhow!("training job {} was cancelled", job_id))
        }
        other => {
            output::payload(result)?;
            Err(anyhow!("unexpected terminal status `{}`", other))
        }
    }
}

fn training_is_terminal(v: &Value) -> bool {
    matches!(
        poll::status_of(v).as_str(),
        "stopped" | "failed" | "canceled" | "cancelled"
    )
}

/// `RUNNING 16% (320/2000)` style one-liner.
fn describe_training(v: &Value) -> String {
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("(no status)")
        .to_string();
    let Some(p) = v.get("progress").filter(|p| p.is_object()) else {
        return status;
    };
    let mut s = status;
    if let Some(pct) = p.get("percent").and_then(Value::as_f64) {
        s.push_str(&format!(" {}%", pct));
    }
    if let (Some(c), Some(t)) = (
        p.get("current_step").and_then(Value::as_i64),
        p.get("total_steps").and_then(Value::as_i64),
    ) {
        s.push_str(&format!(" ({}/{})", c, t));
    }
    s
}

pub async fn status(job_id: String) -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::trainer_api_base(),
        &job_path(&job_id, "/status"),
        &[],
        None,
    )
    .await?;
    if output::is_json() {
        return output::payload(&v);
    }
    let mut pairs = vec![
        ("id", field(&v, "id")),
        ("name", field(&v, "name")),
        ("status", field(&v, "status")),
    ];
    if let Some(p) = v.get("progress").filter(|p| p.is_object()) {
        pairs.push((
            "progress",
            format!(
                "{}% (step {}/{})",
                field(p, "percent"),
                field(p, "current_step"),
                field(p, "total_steps")
            ),
        ));
    }
    if v.get("error").map(|e| !e.is_null()).unwrap_or(false) {
        pairs.push(("error", v["error"].to_string()));
    }
    for key in ["created_at", "started_at", "finished_at", "result_url"] {
        if v.get(key).and_then(Value::as_str).is_some() {
            pairs.push((key, field(&v, key)));
        }
    }
    if poll::status_of(&v) == "stopped" {
        match classify_stopped(&v, &Value::Null) {
            StoppedOutcome::Finished => {}
            outcome @ StoppedOutcome::StoppedEarly { .. } => pairs.push((
                "note",
                format!(
                    "{}; `runcomfy train resume {}` to continue",
                    outcome.describe(),
                    job_id
                ),
            )),
            outcome => pairs.push(("note", outcome.describe())),
        }
    }
    output::kv(&pairs);
    Ok(())
}

pub async fn result(job_id: String, download_artifacts: bool, output_dir: String) -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::trainer_api_base(),
        &job_path(&job_id, "/result"),
        &[],
        None,
    )
    .await?;
    if !output::is_json() {
        let artifacts = v.get("artifacts");
        let checkpoints = artifacts
            .and_then(|a| a.get("checkpoints"))
            .and_then(Value::as_array)
            .map(|c| c.len())
            .unwrap_or(0);
        let samples = artifacts
            .and_then(|a| a.get("samples"))
            .and_then(Value::as_array)
            .map(|c| c.len())
            .unwrap_or(0);
        output::progress(
            "📦",
            "result",
            format!(
                "job {} — status {}: {} checkpoint(s), {} sample(s)",
                job_id,
                field(&v, "status"),
                checkpoints,
                samples
            ),
        );
    }
    output::payload(&v)?;
    if download_artifacts {
        match v.get("artifacts") {
            Some(a) if !a.is_null() => {
                download::download_outputs(a, &output_dir).await?;
            }
            _ => output::progress("ℹ", "info", "No artifacts to download yet."),
        }
    } else if !output::is_json() {
        output::detail(
            "Pass --download to fetch checkpoints / samples (use --output-dir to choose where).",
        );
    }
    Ok(())
}

pub async fn cancel(job_id: String) -> Result<()> {
    let v = api::request(
        Method::POST,
        &api::trainer_api_base(),
        &job_path(&job_id, "/cancel"),
        &[],
        None,
    )
    .await?;
    output::progress(
        "✅",
        "ok",
        format!(
            "Cancelled training job {} (status: {})",
            job_id,
            field(&v, "status")
        ),
    );
    output::detail("Artifacts produced so far are still available via `runcomfy train result`.");
    output::payload(&v)
}

pub async fn resume(job_id: String) -> Result<()> {
    let v = api::request(
        Method::POST,
        &api::trainer_api_base(),
        &job_path(&job_id, "/resume"),
        &[],
        Some(&json!({})),
    )
    .await?;
    output::progress(
        "✅",
        "ok",
        format!("Resumed training job {} from its latest checkpoint", job_id),
    );
    output::detail(format!("Track with `runcomfy train status {}`", job_id));
    output::payload(&v)
}

pub async fn edit(job_id: String, config: String) -> Result<()> {
    let yaml = read_config(&config)?;
    let body = json!({
        "config_file_format": "yaml",
        "config_file": yaml,
    });
    let v = api::request(
        Method::POST,
        &api::trainer_api_base(),
        &job_path(&job_id, "/edit"),
        &[],
        Some(&body),
    )
    .await?;
    output::progress(
        "✅",
        "ok",
        format!(
            "Updated config of training job {} (status: {})",
            job_id,
            field(&v, "status")
        ),
    );
    output::detail(format!(
        "Re-queue it with `runcomfy train resume {}`",
        job_id
    ));
    output::payload(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(cur: i64, total: i64) -> Value {
        json!({"status": "STOPPED", "progress": {"current_step": cur, "total_steps": total, "percent": 0}})
    }

    #[test]
    fn stopped_at_final_step_is_finished() {
        let r = json!({"status": "STOPPED"});
        assert_eq!(
            classify_stopped(&status(2000, 2000), &r),
            StoppedOutcome::Finished
        );
        assert_eq!(
            classify_stopped(&status(2001, 2000), &r),
            StoppedOutcome::Finished
        );
    }

    #[test]
    fn stopped_early_is_incomplete() {
        assert_eq!(
            classify_stopped(&status(320, 2000), &json!({"status": "STOPPED"})),
            StoppedOutcome::StoppedEarly {
                current: 320,
                total: 2000
            }
        );
    }

    #[test]
    fn result_error_wins_even_at_final_step() {
        let result = json!({"status": "STOPPED", "error": {"message": "Job failed: server was reclaimed.", "code": "TRAINING_ERROR"}});
        assert_eq!(
            classify_stopped(&status(2000, 2000), &result),
            StoppedOutcome::ErrorInResult("Job failed: server was reclaimed.".into())
        );
    }

    #[test]
    fn missing_or_unusable_progress_is_not_finished() {
        assert_eq!(
            classify_stopped(&json!({"status": "STOPPED"}), &Value::Null),
            StoppedOutcome::Unverified
        );
        assert_eq!(
            classify_stopped(
                &json!({"status": "STOPPED", "progress": "n/a"}),
                &Value::Null
            ),
            StoppedOutcome::Unverified
        );
        assert_eq!(
            classify_stopped(&status(5, 0), &Value::Null),
            StoppedOutcome::Unverified
        );
        assert_eq!(
            classify_stopped(&json!({"status": "STOPPED", "progress": {}}), &Value::Null),
            StoppedOutcome::Unverified
        );
    }
}
