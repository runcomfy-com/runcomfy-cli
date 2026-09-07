//! `runcomfy deployments ...` — Serverless API (ComfyUI) deployments and
//! the async inference queue on top of them.
//!
//! Endpoints (Serverless API, base `https://api.runcomfy.net/prod/v2`):
//!   GET    /deployments[?ids=..&includes=payload&includes=readme]
//!   POST   /deployments
//!   GET    /deployments/{id}[?includes=..]
//!   PATCH  /deployments/{id}
//!   DELETE /deployments/{id}
//!   POST   /deployments/{id}/inference[?webhook=..&webhook_intermediate_status=..]
//!   GET    /deployments/{id}/requests/{rid}/status
//!   GET    /deployments/{id}/requests/{rid}/result
//!   POST   /deployments/{id}/requests/{rid}/cancel
//!   POST   /deployments/{id}/instances/{iid}/proxy/{path}

use std::time::Duration;

use anyhow::{bail, Result};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::api;
use crate::cli::DownloadOpts;
use crate::cmd::{self, confirm_or_bail, field, require_str};
use crate::error::CliError;
use crate::input;
use crate::output;
use crate::poll::{self, Watch};

pub struct CreateArgs {
    pub name: String,
    pub workflow_id: String,
    pub workflow_version: String,
    pub hardware: String,
    pub min_instances: u32,
    pub max_instances: u32,
    pub queue_size: u32,
    pub keep_warm_secs: u64,
}

pub struct UpdateArgs {
    pub name: Option<String>,
    pub workflow_version: Option<String>,
    pub hardware: Option<String>,
    pub min_instances: Option<u32>,
    pub max_instances: Option<u32>,
    pub queue_size: Option<u32>,
    pub keep_warm_secs: Option<u64>,
    pub is_enabled: Option<bool>,
}

pub struct RunArgs {
    pub overrides: Option<String>,
    pub overrides_file: Option<String>,
    pub workflow_file: Option<String>,
    pub extra_data: Option<String>,
    pub webhook_url: Option<String>,
    pub webhook_intermediate: bool,
    pub wait: bool,
    pub poll_secs: u64,
    pub timeout: Option<u64>,
    pub download: DownloadOpts,
}

fn includes(include_payload: bool, include_readme: bool) -> Vec<(&'static str, String)> {
    let mut q = Vec::new();
    if include_payload {
        q.push(("includes", "payload".to_string()));
    }
    if include_readme {
        q.push(("includes", "readme".to_string()));
    }
    q
}

pub async fn list(ids: Vec<String>, include_payload: bool, include_readme: bool) -> Result<()> {
    let mut query: Vec<(&str, String)> = ids
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| ("ids", s.trim().to_string()))
        .collect();
    query.extend(includes(include_payload, include_readme));

    let v = api::request(
        Method::GET,
        &api::serverless_api_base(),
        "/deployments",
        &query,
        None,
    )
    .await?;
    if output::is_json() || include_payload || include_readme {
        return output::payload(&v);
    }

    // The API returns a bare array; tolerate a wrapped form too.
    let items: Vec<Value> = match &v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .get("deployments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if items.is_empty() {
        output::progress("ℹ", "info", "No deployments found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|d| {
            vec![
                field(d, "id"),
                field(d, "name"),
                field(d, "status"),
                field(d, "is_enabled"),
                field(d, "hardware"),
                format!(
                    "{}-{}",
                    field(d, "min_instances"),
                    field(d, "max_instances")
                ),
                field(d, "workflow_version"),
            ]
        })
        .collect();
    output::table(
        &[
            "ID",
            "NAME",
            "STATUS",
            "ENABLED",
            "HARDWARE",
            "INSTANCES",
            "VERSION",
        ],
        &rows,
    );
    Ok(())
}

pub async fn get(deployment_id: String, include_payload: bool, include_readme: bool) -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::serverless_api_base(),
        &format!("/deployments/{}", deployment_id),
        &includes(include_payload, include_readme),
        None,
    )
    .await?;
    output::payload(&v)
}

pub async fn create(args: CreateArgs) -> Result<()> {
    let body = json!({
        "name": args.name,
        "workflow_id": args.workflow_id,
        "workflow_version": args.workflow_version,
        "hardware": [args.hardware],
        "min_instances": args.min_instances,
        "max_instances": args.max_instances,
        "queue_size": args.queue_size,
        "keep_warm_duration_in_seconds": args.keep_warm_secs,
    });
    output::progress(
        "⏳",
        "create",
        format!("Creating deployment `{}`", args.name),
    );
    let v = api::request(
        Method::POST,
        &api::serverless_api_base(),
        "/deployments",
        &[],
        Some(&body),
    )
    .await?;
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        output::progress("✅", "ok", format!("Created deployment {}", id));
    }
    print_deployment(&v)
}

/// Pretty mode: a compact key/value summary (the raw record also carries
/// the whole workflow graph, which is noise here). JSON mode: the raw
/// record, untouched.
fn print_deployment(v: &Value) -> Result<()> {
    if output::is_json() {
        return output::payload(v);
    }
    let mut pairs = Vec::new();
    for key in [
        "id",
        "name",
        "status",
        "is_enabled",
        "workflow_id",
        "workflow_version",
        "hardware",
        "min_instances",
        "max_instances",
        "queue_size",
        "keep_warm_duration_in_seconds",
        "updated_at",
    ] {
        if v.get(key).is_some() {
            pairs.push((key, field(v, key)));
        }
    }
    if pairs.is_empty() {
        return output::payload(v);
    }
    output::kv(&pairs);
    Ok(())
}

pub async fn update(deployment_id: String, args: UpdateArgs) -> Result<()> {
    let mut body = Map::new();
    if let Some(n) = args.name {
        body.insert("name".into(), json!(n));
    }
    if let Some(wv) = args.workflow_version {
        body.insert("workflow_version".into(), json!(wv));
    }
    if let Some(h) = args.hardware {
        body.insert("hardware".into(), json!([h]));
    }
    if let Some(n) = args.min_instances {
        body.insert("min_instances".into(), json!(n));
    }
    if let Some(n) = args.max_instances {
        body.insert("max_instances".into(), json!(n));
    }
    if let Some(n) = args.queue_size {
        body.insert("queue_size".into(), json!(n));
    }
    if let Some(n) = args.keep_warm_secs {
        body.insert("keep_warm_duration_in_seconds".into(), json!(n));
    }
    if let Some(e) = args.is_enabled {
        body.insert("is_enabled".into(), json!(e));
    }
    if body.is_empty() {
        bail!(CliError::InvalidInput(
            "nothing to update — pass at least one of --name, --workflow-version, \
             --hardware, --min-instances, --max-instances, --queue-size, \
             --keep-warm-secs, --enable, --disable"
                .into()
        ));
    }
    let base = api::serverless_api_base();
    // The API rejects a PATCH without `name` ("Deployment name is
    // required"), so carry the current name over when it isn't changing.
    if !body.contains_key("name") {
        let current = api::request(
            Method::GET,
            &base,
            &format!("/deployments/{}", deployment_id),
            &[],
            None,
        )
        .await?;
        let name = require_str(&current, "name", "get deployment")?;
        body.insert("name".into(), json!(name));
    }
    let v = api::request(
        Method::PATCH,
        &base,
        &format!("/deployments/{}", deployment_id),
        &[],
        Some(&Value::Object(body)),
    )
    .await?;
    output::progress("✅", "ok", format!("Updated deployment {}", deployment_id));
    print_deployment(&v)
}

pub async fn delete(deployment_id: String, yes: bool) -> Result<()> {
    confirm_or_bail(
        yes,
        &format!("permanently delete deployment {}", deployment_id),
    )?;
    let v = api::request(
        Method::DELETE,
        &api::serverless_api_base(),
        &format!("/deployments/{}", deployment_id),
        &[],
        None,
    )
    .await?;
    output::progress("✅", "ok", format!("Deleted deployment {}", deployment_id));
    if output::is_json() {
        output::payload(&json!({
            "deployment_id": deployment_id,
            "deleted": true,
            "response": v,
        }))?;
    }
    Ok(())
}

pub async fn run(deployment_id: String, args: RunArgs) -> Result<()> {
    let mut body = Map::new();
    if let Some(o) = input::json_arg(
        args.overrides.as_deref(),
        args.overrides_file.as_deref(),
        "overrides",
    )? {
        input::require_object(&o, "--overrides")?;
        body.insert("overrides".into(), o);
    }
    if let Some(p) = args.workflow_file.as_deref() {
        let w = input::parse_json(&input::read_text(p, "--workflow-file")?, "--workflow-file")?;
        input::require_object(&w, "--workflow-file")?;
        body.insert("workflow_api_json".into(), w);
    }
    if let Some(s) = args.extra_data.as_deref() {
        let e = input::parse_json(s, "--extra-data")?;
        input::require_object(&e, "--extra-data")?;
        body.insert("extra_data".into(), e);
    }

    if !body.contains_key("overrides") && !body.contains_key("workflow_api_json") {
        bail!(CliError::InvalidInput(format!(
            "pass --overrides / --overrides-file (inputs keyed by node ID) or \
             --workflow-file; run `runcomfy deployments get {} --include-payload` \
             to see the node IDs and default overrides",
            deployment_id
        )));
    }

    let mut query: Vec<(&str, String)> = Vec::new();
    if let Some(w) = args.webhook_url.as_deref() {
        query.push(("webhook", w.to_string()));
        if args.webhook_intermediate {
            query.push(("webhook_intermediate_status", "true".to_string()));
        }
    }

    let base = api::serverless_api_base();
    output::progress(
        "⏳",
        "submit",
        format!("Submitting request to deployment {}", deployment_id),
    );
    let submit = api::request(
        Method::POST,
        &base,
        &format!("/deployments/{}/inference", deployment_id),
        &query,
        Some(&Value::Object(body)),
    )
    .await?;
    let request_id = require_str(&submit, "request_id", "submit")?.to_string();
    output::detail(format!("request_id: {}", request_id));

    if !args.wait {
        return output::payload(&json!({
            "request_id": request_id,
            "deployment_id": deployment_id,
            "wait": false,
        }));
    }

    let req_path = |suffix: &str| {
        api::url(
            &base,
            &format!(
                "deployments/{}/requests/{}/{}",
                deployment_id, request_id, suffix
            ),
        )
    };
    let status_url = req_path("status");
    let cancel_url = req_path("cancel");
    let client = api::http()?;
    let token = api::require_token()?;

    output::progress(
        "⏳",
        "poll",
        format!("Polling status (every {}s)...", args.poll_secs.max(1)),
    );
    poll::wait_terminal(
        &client,
        &token,
        Watch {
            status_url: &status_url,
            cancel_url: Some(&cancel_url),
            interval: Duration::from_secs(args.poll_secs.max(1)),
            timeout: args.timeout.map(Duration::from_secs),
            what: format!("request {}", request_id),
            detach_hint: format!(
                "run `runcomfy deployments cancel {} {}` to stop it, or \
                 `runcomfy deployments result {} {}` to fetch it later",
                deployment_id, request_id, deployment_id, request_id
            ),
        },
        poll::inference_is_terminal,
        poll::describe_inference,
    )
    .await?;

    let result = api::request(
        Method::GET,
        &base,
        &format!(
            "/deployments/{}/requests/{}/result",
            deployment_id, request_id
        ),
        &[],
        None,
    )
    .await?;
    cmd::finish_inference_result(&result, &["outputs", "output"], &args.download, true).await
}

pub async fn status(deployment_id: String, request_id: String) -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::serverless_api_base(),
        &format!(
            "/deployments/{}/requests/{}/status",
            deployment_id, request_id
        ),
        &[],
        None,
    )
    .await?;
    if output::is_json() {
        return output::payload(&v);
    }
    let mut pairs = vec![
        ("request_id", field(&v, "request_id")),
        ("status", field(&v, "status")),
    ];
    if v.get("queue_position")
        .map(|p| !p.is_null())
        .unwrap_or(false)
    {
        pairs.push(("queue", format!("position {}", field(&v, "queue_position"))));
    }
    if v.get("instance_id").map(|p| !p.is_null()).unwrap_or(false) {
        pairs.push(("instance_id", field(&v, "instance_id")));
    }
    for key in ["status_url", "result_url"] {
        if v.get(key).and_then(Value::as_str).is_some() {
            pairs.push((key, field(&v, key)));
        }
    }
    output::kv(&pairs);
    Ok(())
}

pub async fn result(
    deployment_id: String,
    request_id: String,
    download: DownloadOpts,
) -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::serverless_api_base(),
        &format!(
            "/deployments/{}/requests/{}/result",
            deployment_id, request_id
        ),
        &[],
        None,
    )
    .await?;
    cmd::finish_inference_result(&v, &["outputs", "output"], &download, false).await
}

pub async fn cancel(deployment_id: String, request_id: String) -> Result<()> {
    let v = api::request(
        Method::POST,
        &api::serverless_api_base(),
        &format!(
            "/deployments/{}/requests/{}/cancel",
            deployment_id, request_id
        ),
        &[],
        None,
    )
    .await?;
    cmd::print_cancel_outcome(&v, &request_id)
}

pub async fn proxy(
    deployment_id: String,
    instance_id: String,
    path: String,
    body: Option<String>,
    body_file: Option<String>,
) -> Result<()> {
    let body = input::json_arg(body.as_deref(), body_file.as_deref(), "body")?
        .unwrap_or_else(|| Value::Object(Default::default()));
    let route = path.trim_start_matches('/');
    // The ComfyUI backend can answer with plain text, so this is the one
    // call site that tolerates a non-JSON body.
    let v = api::request_allow_text(
        Method::POST,
        &api::serverless_api_base(),
        &format!(
            "/deployments/{}/instances/{}/proxy/{}",
            deployment_id, instance_id, route
        ),
        Some(&body),
    )
    .await?;
    match &v {
        // Non-JSON backend replies come back as a plain string; print
        // them verbatim in pretty mode instead of as a quoted JSON string.
        Value::String(s) if !output::is_json() => {
            println!("{}", s);
            Ok(())
        }
        Value::Null if !output::is_json() => {
            output::progress("✅", "ok", format!("{} returned no body", route));
            Ok(())
        }
        _ => output::payload(&v),
    }
}
