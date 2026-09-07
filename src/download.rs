//! Downloading generated assets referenced by API result payloads.
//!
//! Shared by `run`, `result`, `deployments run` / `deployments result`,
//! and `train result`. Every caller passes only the *output* portion of a
//! response (Model API `output`, Serverless `outputs`, Trainer
//! `artifacts`) — never the whole payload, which also carries
//! `status_url` / `cancel_url` API links that must not be fetched as files.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Url};
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::api;
use crate::output;

/// Hosts the CLI is willing to download generated assets from.
///
/// RunComfy result URLs live on RunComfy-controlled CDN buckets
/// (`playgrounds-storage-public.runcomfy.net`,
/// `serverless-api-storage.runcomfy.net`, `files.runcomfy.net`, ...).
/// Downloads are restricted to these hosts so a compromised / adversarial
/// upstream model can't trick the CLI into pulling arbitrary internet
/// content (SSRF-style amplification).
///
/// Add new hosts here as RunComfy starts returning new CDN domains.
pub const TRUSTED_DOWNLOAD_HOST_SUFFIXES: &[&str] = &[".runcomfy.net", ".runcomfy.com"];

/// Cap a single result file at 2 GiB. Generative video and LoRA
/// checkpoints can legitimately be hundreds of MB, but anything past this
/// is likely runaway / malicious.
pub const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Download every trusted asset URL found in `value` into `output_dir`.
///
/// Logs skipped (untrusted-host) URLs and per-file results through
/// `output`, creates the directory on demand, and returns the paths
/// written. A single failed download is reported and skipped rather than
/// aborting the rest.
pub async fn download_outputs(value: &Value, output_dir: &str) -> Result<Vec<PathBuf>> {
    let mut all_urls = Vec::new();
    collect_urls(value, &mut all_urls);

    let (trusted, untrusted): (Vec<_>, Vec<_>) = all_urls
        .into_iter()
        .partition(|u| is_trusted_download_url(u));

    if !untrusted.is_empty() {
        output::progress(
            "⚠",
            "skip",
            format!(
                "Skipped {} URL(s) outside trusted hosts (downloads disabled for non-RunComfy CDNs)",
                untrusted.len()
            ),
        );
        for u in &untrusted {
            output::detail(format!("(not downloaded) {}", u));
        }
    }

    if trusted.is_empty() {
        return Ok(Vec::new());
    }

    let dir = PathBuf::from(output_dir);
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create output dir {}", dir.display()))?;
    }
    output::progress(
        "📥",
        "download",
        format!("Downloading {} file(s) to {}", trusted.len(), dir.display()),
    );

    // Long-lived client: no overall request timeout, only connect / read
    // inactivity timeouts, so a multi-hundred-MB video isn't cut off.
    let client = api::http_long()?;
    let mut written = Vec::new();
    for url in trusted {
        match download_file(&client, &url, &dir).await {
            Ok(path) => {
                output::detail(path.display().to_string());
                written.push(path);
            }
            Err(e) => output::detail(format!("⚠ {}: {}", url, e)),
        }
    }
    Ok(written)
}

/// Recursively walk a JSON value and collect every http(s) URL, in
/// document order, without duplicates.
pub fn collect_urls(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            if (s.starts_with("http://") || s.starts_with("https://")) && !out.contains(s) {
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

/// File name for a downloaded asset: the URL's last path segment, reduced
/// to a single safe filesystem component. Path separators, control
/// characters and characters that are illegal on Windows are replaced, and
/// a name that would be empty, `.` or `..` becomes `output` — so a crafted
/// result URL can never make `Path::join` write outside `--output-dir`.
pub fn filename_from_url(url: &str) -> String {
    let last_segment = Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut segs| segs.rfind(|s| !s.is_empty()).map(str::to_string))
        })
        .unwrap_or_default();
    sanitize_filename(&last_segment)
}

fn sanitize_filename(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "output".to_string()
    } else {
        trimmed.to_string()
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
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_string());
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

/// Stream-download a URL into `dir`. Refuses to write more than
/// `MAX_DOWNLOAD_BYTES` to disk. Aborts and removes the partial file on
/// any error so the caller never sees a half-written output.
///
/// The request is deliberately sent *without* the bearer token: result
/// URLs are pre-signed / public CDN links, and the token must never
/// leave the RunComfy API hosts.
pub async fn download_file(client: &Client, url: &str, dir: &Path) -> Result<PathBuf> {
    // Parse once and hand the parsed URL to reqwest, so the host that was
    // checked is exactly the host that gets contacted. Re-checked here
    // because this function is also reachable directly.
    let parsed = Url::parse(url).with_context(|| format!("parse download URL {}", url))?;
    if !is_trusted_download_url(url) {
        return Err(anyhow!("refusing to download from untrusted URL {}", url));
    }
    let name = filename_from_url(url);
    let path = dedup_path(dir, &name);

    let resp = client
        .get(parsed)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?
        .error_for_status()
        .with_context(|| format!("download {}", url))?;

    if let Some(len) = resp.content_length() {
        if len > MAX_DOWNLOAD_BYTES {
            return Err(anyhow!(
                "refusing to download {} bytes (limit {}); url={}",
                len,
                MAX_DOWNLOAD_BYTES,
                url
            ));
        }
    }

    let mut file = tokio::fs::File::create(&path)
        .await
        .with_context(|| format!("create {}", path.display()))?;

    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk.with_context(|| format!("read body {}", url)) {
            Ok(c) => c,
            Err(e) => {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(e);
            }
        };
        total = total.saturating_add(chunk.len() as u64);
        if total > MAX_DOWNLOAD_BYTES {
            // Drop the partial file so users don't see truncated output.
            let _ = tokio::fs::remove_file(&path).await;
            return Err(anyhow!(
                "stream exceeded {} bytes; aborted download of {}",
                MAX_DOWNLOAD_BYTES,
                url
            ));
        }
        if let Err(e) = file
            .write_all(&chunk)
            .await
            .with_context(|| format!("write {}", path.display()))
        {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(e);
        }
    }
    file.flush()
        .await
        .with_context(|| format!("flush {}", path.display()))?;

    Ok(path)
}

/// Trusted-host check for download URLs.
///
/// Uses the same WHATWG parser reqwest uses, so the host checked here is
/// the host that would actually be contacted. A hand-rolled authority
/// split can be fooled by inputs such as
/// `https://127.0.0.1\@cdn.runcomfy.net/x`, where the parser treats the
/// backslash as a path separator and the real host is `127.0.0.1`.
pub fn is_trusted_download_url(url: &str) -> bool {
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return false,
    };
    if !matches!(parsed.scheme(), "https" | "http") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    TRUSTED_DOWNLOAD_HOST_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_hosts() {
        assert!(is_trusted_download_url(
            "https://serverless-api-storage.runcomfy.net/a/b.png"
        ));
        assert!(is_trusted_download_url(
            "https://files.runcomfy.net/train/x.safetensors?sig=1"
        ));
        assert!(is_trusted_download_url(
            "https://user:pw@cdn.runcomfy.com:443/x"
        ));
        assert!(!is_trusted_download_url(
            "https://evil.example.com/runcomfy.net/x"
        ));
        assert!(!is_trusted_download_url("https://runcomfy.net.evil.com/x"));
        assert!(!is_trusted_download_url("ftp://files.runcomfy.net/x"));
        assert!(!is_trusted_download_url("not a url"));
    }

    #[test]
    fn parser_resolves_backslash_authority_to_the_real_host() {
        // Why a hand-rolled authority split is wrong here: the URL parser
        // reqwest uses reads this host as 127.0.0.1 (the backslash begins
        // the path), while a naive rsplit('@') reads "cdn.runcomfy.net".
        let u = Url::parse("https://127.0.0.1\\@cdn.runcomfy.net/file").unwrap();
        assert_eq!(u.host_str(), Some("127.0.0.1"));
        assert!(!is_trusted_download_url(u.as_str()));
    }

    #[test]
    fn host_confusion_attempts_are_untrusted() {
        // Backslash is a path separator to the URL parser: the real host is
        // 127.0.0.1, even though a naive authority split reads
        // "cdn.runcomfy.net" after the '@'.
        assert!(!is_trusted_download_url(
            "https://127.0.0.1\\@cdn.runcomfy.net/file"
        ));
        // Trusted-looking userinfo in front of an untrusted host.
        assert!(!is_trusted_download_url(
            "https://cdn.runcomfy.net@evil.example.com/file"
        ));
        // Trusted name only in the path, query or fragment.
        assert!(!is_trusted_download_url(
            "https://evil.example.com/#@cdn.runcomfy.net"
        ));
        assert!(!is_trusted_download_url(
            "https://evil.example.com/?u=https://cdn.runcomfy.net"
        ));
        // IP literals never match a suffix.
        assert!(!is_trusted_download_url("https://[::1]/x"));
        assert!(!is_trusted_download_url("http://10.0.0.1/x.runcomfy.net"));
    }

    #[test]
    fn urls_are_collected_once_in_order() {
        let v = serde_json::json!({
            "images": ["https://a.runcomfy.net/1.png", "https://a.runcomfy.net/1.png"],
            "nested": {"video": "https://a.runcomfy.net/2.mp4", "n": 1}
        });
        let mut out = Vec::new();
        collect_urls(&v, &mut out);
        assert_eq!(
            out,
            vec![
                "https://a.runcomfy.net/1.png",
                "https://a.runcomfy.net/2.mp4"
            ]
        );
    }

    #[test]
    fn filename_strips_query_and_fragment() {
        assert_eq!(
            filename_from_url("https://x.runcomfy.net/a/b.png?x=1#f"),
            "b.png"
        );
        assert_eq!(filename_from_url("https://x.runcomfy.net/"), "output");
    }

    #[test]
    fn filenames_stay_inside_the_output_dir() {
        // Windows path separator inside a segment must not escape.
        assert_eq!(
            filename_from_url("https://x.runcomfy.net/a/..\\victim"),
            "victim"
        );
        // Traversal segments resolve away or degrade to a safe default.
        assert_eq!(filename_from_url("https://x.runcomfy.net/a/.."), "output");
        assert_eq!(
            filename_from_url("https://x.runcomfy.net/a/%2e%2e/"),
            "output"
        );
        assert_eq!(sanitize_filename("con:fig?.txt"), "con_fig_.txt");
        assert_eq!(sanitize_filename("a/b"), "a_b");
        assert_eq!(sanitize_filename("..."), "output");
        assert_eq!(sanitize_filename(""), "output");
        assert!(!filename_from_url("https://x.runcomfy.net/a/..\\victim").contains('\\'));
    }
}
