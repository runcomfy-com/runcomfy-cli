//! `runcomfy models {list,categories,get}` — browse the hosted model
//! catalog behind `runcomfy run`.
//!
//! Endpoints (Model API):
//!   GET {base}/models?search=&category=&kind=&include_schema=&limit=&offset=
//!   GET {base}/models/categories
//!   GET {base}/models/{model_id}

use anyhow::Result;
use reqwest::Method;
use serde_json::Value;

use crate::api;
use crate::cmd::field;
use crate::output;

pub async fn list(
    search: Option<String>,
    category: Option<String>,
    kind: Option<String>,
    include_schema: bool,
    limit: u32,
    offset: u32,
) -> Result<()> {
    let mut query: Vec<(&str, String)> =
        vec![("limit", limit.to_string()), ("offset", offset.to_string())];
    if let Some(s) = search.filter(|s| !s.trim().is_empty()) {
        query.push(("search", s));
    }
    if let Some(c) = category.filter(|c| !c.trim().is_empty()) {
        query.push(("category", c));
    }
    if let Some(k) = kind.filter(|k| !k.trim().is_empty()) {
        query.push(("kind", k));
    }
    if include_schema {
        query.push(("include_schema", "true".to_string()));
    }

    let v = api::request(Method::GET, &api::model_api_base(), "/models", &query, None).await?;
    if output::is_json() || include_schema {
        return output::payload(&v);
    }

    let models = v
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = v
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(models.len() as u64);

    if models.is_empty() {
        output::progress("ℹ", "info", "No models matched.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            vec![
                field(m, "model_id"),
                field(m, "display_name"),
                field(m, "categories"),
                price(m),
            ]
        })
        .collect();
    output::table(&["MODEL_ID", "NAME", "CATEGORY", "PRICE"], &rows);
    output::detail(format!(
        "Showing {}-{} of {} (page with --offset/--limit; filter with --search/--category)",
        offset + 1,
        offset as usize + models.len(),
        total
    ));
    Ok(())
}

pub async fn categories() -> Result<()> {
    let v = api::request(
        Method::GET,
        &api::model_api_base(),
        "/models/categories",
        &[],
        None,
    )
    .await?;
    if output::is_json() {
        return output::payload(&v);
    }
    match v.get("categories").and_then(Value::as_array) {
        Some(cats) => {
            for c in cats {
                println!(
                    "{}",
                    c.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| c.to_string())
                );
            }
            Ok(())
        }
        None => output::payload(&v),
    }
}

pub async fn get(model_id: String) -> Result<()> {
    let id = model_id.trim().trim_matches('/').to_string();
    let v = api::request(
        Method::GET,
        &api::model_api_base(),
        &format!("/models/{}", id),
        &[],
        None,
    )
    .await?;
    // The schema is the useful part and it is JSON either way, so both
    // modes print the payload; pretty mode just adds a one-line header.
    if !output::is_json() {
        let name = field(&v, "display_name");
        let cats = field(&v, "categories");
        output::progress(
            "📦",
            "model",
            format!("{} — {} [{}] {}", id, name, cats, price(&v)),
        );
    }
    output::payload(&v)
}

/// `$0.044/output` style price cell, or `-` when the catalog entry has
/// no pricing.
fn price(m: &Value) -> String {
    match (
        m.get("base_price_usd").and_then(Value::as_f64),
        m.get("price_unit").and_then(Value::as_str),
    ) {
        (Some(p), Some(unit)) => format!("${}/{}", p, unit),
        (Some(p), None) => format!("${}", p),
        _ => "-".to_string(),
    }
}
