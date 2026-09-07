//! `runcomfy datasets ...` — LoRA training datasets (Trainer API).
//!
//! Endpoints (base `https://trainer-api.runcomfy.net/prod/v1`):
//!   POST   /trainers/datasets                              create
//!   GET    /trainers/datasets                              list
//!   GET    /trainers/datasets/{id}/status                  status + files
//!   DELETE /trainers/datasets/{id}
//!   POST   /trainers/datasets/{id}/upload                  multipart, <= 150 MB
//!   POST   /trainers/datasets/{id}/get-upload-endpoint     signed PUT URLs, > 150 MB
//!
//! `upload` picks the transport per file: small files go straight through
//! the multipart endpoint, larger ones fetch a signed URL and stream the
//! bytes there. Either way the token only ever reaches the Trainer API.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::CONTENT_LENGTH;
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Method};
use serde_json::{json, Map, Value};
use tokio_util::io::ReaderStream;

use crate::api;
use crate::cmd::{confirm_or_bail, field};
use crate::error::CliError;
use crate::output;
use crate::poll::{self, Watch};

/// Direct multipart uploads are capped by the API at 150 MB per file.
const SMALL_UPLOAD_LIMIT_BYTES: u64 = 150_000_000;

fn ds_path(dataset_id: &str, suffix: &str) -> String {
    format!("/trainers/datasets/{}{}", dataset_id, suffix)
}

pub async fn create(name: Option<String>) -> Result<()> {
    let mut body = Map::new();
    if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
        body.insert("name".into(), json!(n.trim()));
    }
    let v = api::request(
        Method::POST,
        &api::trainer_api_base(),
        "/trainers/datasets",
        &[],
        Some(&Value::Object(body)),
    )
    .await?;
    output::progress(
        "✅",
        "ok",
        format!(
            "Created dataset {} (name: {}, status: {})",
            field(&v, "id"),
            field(&v, "name"),
            field(&v, "status")
        ),
    );
    output::detail(format!(
        "Next: runcomfy datasets upload {} <files...> --wait",
        field(&v, "id")
    ));
    output::payload(&v)
}

pub async fn list() -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::trainer_api_base(),
        "/trainers/datasets",
        &[],
        None,
    )
    .await?;
    if output::is_json() {
        return output::payload(&v);
    }
    let items: Vec<Value> = match &v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .get("datasets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if items.is_empty() {
        output::progress("ℹ", "info", "No datasets found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|d| {
            vec![
                field(d, "id"),
                field(d, "name"),
                field(d, "status"),
                field(d, "updated_at"),
            ]
        })
        .collect();
    output::table(&["ID", "NAME", "STATUS", "UPDATED"], &rows);
    Ok(())
}

pub async fn status(dataset_id: String) -> Result<()> {
    let v = fetch_status(&dataset_id).await?;
    if output::is_json() {
        return output::payload(&v);
    }
    print_status(&v);
    Ok(())
}

async fn fetch_status(dataset_id: &str) -> Result<Value> {
    api::request(
        Method::GET,
        &api::trainer_api_base(),
        &ds_path(dataset_id, "/status"),
        &[],
        None,
    )
    .await
}

fn print_status(v: &Value) {
    let mut pairs = vec![
        ("id", field(v, "id")),
        ("name", field(v, "name")),
        ("status", field(v, "status")),
        ("created_at", field(v, "created_at")),
        ("updated_at", field(v, "updated_at")),
    ];
    if v.get("error").map(|e| !e.is_null()).unwrap_or(false) {
        pairs.push(("error", v["error"].to_string()));
    }
    output::kv(&pairs);
    if let Some(files) = v.get("files").and_then(Value::as_array) {
        println!();
        if files.is_empty() {
            println!("(no files uploaded yet)");
        } else {
            let rows: Vec<Vec<String>> = files
                .iter()
                .map(|f| vec![field(f, "filename"), field(f, "size_bytes")])
                .collect();
            output::table(&["FILENAME", "SIZE_BYTES"], &rows);
        }
    }
}

pub async fn delete(dataset_id: String, yes: bool) -> Result<()> {
    confirm_or_bail(yes, &format!("permanently delete dataset {}", dataset_id))?;
    let v = api::request(
        Method::DELETE,
        &api::trainer_api_base(),
        &ds_path(&dataset_id, ""),
        &[],
        None,
    )
    .await?;
    output::progress("✅", "ok", format!("Deleted dataset {}", dataset_id));
    if output::is_json() {
        output::payload(&if v.is_null() {
            json!({"id": dataset_id, "deleted": true})
        } else {
            v
        })?;
    }
    Ok(())
}

pub async fn upload(
    dataset_id: String,
    paths: Vec<String>,
    from_url: Option<String>,
    filename: Option<String>,
    wait: bool,
    poll_secs: u64,
) -> Result<()> {
    let files = expand_paths(&paths)?;
    if files.is_empty() && from_url.is_none() {
        bail!(CliError::InvalidInput(
            "nothing to upload — pass one or more files / directories, or --from-url".into()
        ));
    }
    if let Some(u) = from_url.as_deref() {
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            bail!(CliError::InvalidInput(format!(
                "--from-url must be an http(s) URL, got `{}`",
                u
            )));
        }
    }

    let base = api::trainer_api_base();
    let token = api::require_token()?;
    let client = api::http_long()?;

    let mut small: Vec<(PathBuf, String, u64)> = Vec::new();
    let mut large: Vec<(PathBuf, String, u64)> = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("{}: file name is not valid UTF-8", path.display()))?
            .to_string();
        let size = std::fs::metadata(&path)
            .with_context(|| format!("stat {}", path.display()))?
            .len();
        if size <= SMALL_UPLOAD_LIMIT_BYTES {
            small.push((path, name, size));
        } else {
            large.push((path, name, size));
        }
    }

    let total = small.len() + large.len() + usize::from(from_url.is_some());
    output::progress(
        "📤",
        "upload",
        format!("Uploading {} file(s) to dataset {}", total, dataset_id),
    );

    let mut uploaded: Vec<Value> = Vec::new();

    // 1. Small files: one multipart POST each.
    for (path, name, size) in &small {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("read {}", path.display()))?;
        let v = upload_bytes(&client, &token, &base, &dataset_id, name, bytes).await?;
        output::detail(format!("{} ({} bytes)", name, size));
        uploaded.push(upload_record(name, *size, "multipart", v));
    }

    // 2. Large files: one signed URL per file, then a streamed PUT.
    if !large.is_empty() {
        let mut sizes = Map::new();
        for (_, name, size) in &large {
            sizes.insert(name.clone(), json!(size));
        }
        let endpoints = api::request(
            Method::POST,
            &base,
            &ds_path(&dataset_id, "/get-upload-endpoint"),
            &[],
            Some(&json!({ "filenameToByteSize": sizes })),
        )
        .await?;
        let uploads = endpoints
            .get("uploads")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                anyhow!(
                    "get-upload-endpoint response has no `uploads` map: {}",
                    serde_json::to_string(&endpoints).unwrap_or_default()
                )
            })?;
        for (path, name, size) in &large {
            let target = uploads
                .get(name)
                .ok_or_else(|| anyhow!("no signed upload URL returned for {}", name))?;
            put_signed(&client, path, target, *size)
                .await
                .with_context(|| format!("signed upload of {}", name))?;
            output::detail(format!("{} ({} bytes, signed URL)", name, size));
            uploaded.push(upload_record(name, *size, "signed-url", Value::Null));
        }
    }

    // 3. --from-url: fetch into memory (<= 150 MB), then multipart.
    if let Some(url) = from_url.as_deref() {
        output::detail(format!("fetching {}", url));
        let bytes = fetch_public_url(&client, url).await?;
        let name = filename
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                let guess = crate::download::filename_from_url(url);
                if guess == "output" {
                    "upload.bin".to_string()
                } else {
                    guess
                }
            });
        let size = bytes.len() as u64;
        let v = upload_bytes(&client, &token, &base, &dataset_id, &name, bytes).await?;
        output::detail(format!("{} ({} bytes, from URL)", name, size));
        uploaded.push(upload_record(&name, size, "url", v));
    }

    output::progress(
        "✅",
        "ok",
        format!(
            "Uploaded {} file(s) to dataset {}",
            uploaded.len(),
            dataset_id
        ),
    );

    let mut final_status: Option<Value> = None;
    if wait {
        let status_url = api::url(&base, &ds_path(&dataset_id, "/status"));
        output::progress(
            "⏳",
            "poll",
            format!(
                "Waiting for dataset to become READY (every {}s)...",
                poll_secs.max(1)
            ),
        );
        let poll_client = api::http()?;
        let v = poll::wait_terminal(
            &poll_client,
            &token,
            Watch {
                status_url: &status_url,
                cancel_url: None,
                interval: Duration::from_secs(poll_secs.max(1)),
                timeout: None,
                what: format!("dataset {}", dataset_id),
                detach_hint: format!("check with `runcomfy datasets status {}`", dataset_id),
            },
            |v| matches!(poll::status_of(v).as_str(), "ready" | "failed"),
            describe_dataset,
        )
        .await?;
        let st = poll::status_of(&v);
        if st == "failed" {
            let err = v
                .get("error")
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_else(|| "(no detail)".into());
            if output::is_json() {
                output::payload(&json!({
                    "dataset_id": dataset_id,
                    "uploaded": uploaded,
                    "status": v,
                }))?;
            }
            return Err(anyhow!("dataset {} FAILED validation: {}", dataset_id, err));
        }
        output::progress("✅", "ok", format!("Dataset {} is READY", dataset_id));
        final_status = Some(v);
    } else {
        output::detail(format!(
            "Check readiness with `runcomfy datasets status {}` (must be READY before training)",
            dataset_id
        ));
    }

    if output::is_json() {
        output::payload(&json!({
            "dataset_id": dataset_id,
            "uploaded": uploaded,
            "status": final_status,
        }))?;
    } else if let Some(v) = &final_status {
        print_status(v);
    }
    Ok(())
}

fn upload_record(name: &str, size: u64, via: &str, response: Value) -> Value {
    json!({
        "filename": name,
        "bytes": size,
        "via": via,
        "response": response,
    })
}

fn describe_dataset(v: &Value) -> String {
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("(no status)");
    match v.get("files").and_then(Value::as_array) {
        Some(files) => format!("{} ({} file(s) visible)", status, files.len()),
        None => status.to_string(),
    }
}

/// Files to upload: each path is a file, or a directory whose top-level
/// regular files (dotfiles excluded) are taken in sorted order.
fn expand_paths(paths: &[String]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for raw in paths {
        let p = PathBuf::from(raw);
        let meta = std::fs::metadata(&p).with_context(|| format!("{}: not found", p.display()))?;
        if meta.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&p)
                .with_context(|| format!("read directory {}", p.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|e| e.is_file())
                .filter(|e| {
                    !e.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(true)
                })
                .collect();
            entries.sort();
            if entries.is_empty() {
                output::progress(
                    "⚠",
                    "skip",
                    format!("{}: directory contains no files", p.display()),
                );
            }
            out.extend(entries);
        } else if meta.is_file() {
            out.push(p);
        } else {
            bail!(CliError::InvalidInput(format!(
                "{}: not a regular file or directory",
                p.display()
            )));
        }
    }
    Ok(out)
}

async fn upload_bytes(
    client: &Client,
    token: &str,
    base: &str,
    dataset_id: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<Value> {
    if bytes.len() as u64 > SMALL_UPLOAD_LIMIT_BYTES {
        bail!(
            "{} is {} bytes, over the {} byte direct-upload limit",
            name,
            bytes.len(),
            SMALL_UPLOAD_LIMIT_BYTES
        );
    }
    let part = Part::bytes(bytes)
        .file_name(name.to_string())
        .mime_str(guess_mime(name))?;
    let form = Form::new().part("file", part);
    let url = api::url(base, &ds_path(dataset_id, "/upload"));
    let resp = client
        .post(&url)
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("POST {}", url))?;
    api::read_json_value(resp, &format!("upload {}", name)).await
}

/// PUT a local file to a signed URL using the method / headers the API
/// handed back. No bearer token: the signature is the credential.
async fn put_signed(client: &Client, path: &Path, target: &Value, size: u64) -> Result<()> {
    let upload_url = target
        .get("upload_url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("signed upload entry has no upload_url"))?;
    let method = target
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("PUT")
        .to_ascii_uppercase();
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| anyhow!("unsupported upload method `{}`", method))?;

    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;
    let body = reqwest::Body::wrap_stream(ReaderStream::new(file));

    let mut req = client
        .request(method, upload_url)
        .header(CONTENT_LENGTH, size)
        .body(body);
    if let Some(headers) = target.get("headers").and_then(Value::as_object) {
        for (k, v) in headers {
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }
    let resp = req.send().await.context("send signed upload")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!(CliError::Api {
            status: status.as_u16(),
            message: format!(
                "storage rejected the upload: {}",
                api::truncate_error_body(&body)
            ),
        });
    }
    Ok(())
}

/// Download a public URL into memory, refusing anything over the direct
/// upload limit (the server-side fetch has the same cap).
async fn fetch_public_url(client: &Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?
        .error_for_status()
        .with_context(|| format!("fetch {}", url))?;
    if let Some(len) = resp.content_length() {
        if len > SMALL_UPLOAD_LIMIT_BYTES {
            bail!(
                "{} is {} bytes; --from-url only supports files up to {} bytes — download it and upload the local file instead",
                url,
                len,
                SMALL_UPLOAD_LIMIT_BYTES
            );
        }
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("read body {}", url))?;
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > SMALL_UPLOAD_LIMIT_BYTES {
            bail!(
                "{} exceeded {} bytes; --from-url only supports files up to that size",
                url,
                SMALL_UPLOAD_LIMIT_BYTES
            );
        }
    }
    Ok(buf)
}

fn guess_mime(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "txt" => "text/plain",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        _ => "application/octet-stream",
    }
}
