//! `runcomfy balance` — the account's remaining balance.
//!
//! Endpoint: GET {serverless_base}/balance
//! One wallet funds Model API requests, Serverless deployments, and
//! training jobs alike, so this lives on the core API rather than being
//! mirrored per product.

use anyhow::Result;
use reqwest::Method;
use serde_json::Value;

use crate::api;
use crate::output;

pub async fn run() -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::serverless_api_base(),
        "/balance",
        &[],
        None,
    )
    .await?;
    if output::is_json() {
        return output::payload(&v);
    }
    match v.get("balance_usd").and_then(Value::as_f64) {
        Some(usd) => {
            let currency = v.get("currency").and_then(Value::as_str).unwrap_or("USD");
            println!("balance: ${:.2} {}", usd, currency);
            Ok(())
        }
        None => output::payload(&v),
    }
}
