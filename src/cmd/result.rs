//! `runcomfy result <request_id>` (alias `requests result`) — fetch the
//! result record of a Model API request submitted earlier, typically
//! with `runcomfy run --no-wait`.
//!
//! Endpoint: GET {model_api_base}/requests/{request_id}/result

use anyhow::Result;
use reqwest::Method;

use crate::api;
use crate::cli::DownloadOpts;
use crate::cmd;

pub async fn run(request_id: String, download: DownloadOpts) -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::model_api_base(),
        &format!("/requests/{}/result", request_id),
        &[],
        None,
    )
    .await?;
    cmd::finish_inference_result(&v, &["output"], &download, false).await
}
