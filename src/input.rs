//! Helpers for reading JSON / text arguments from the command line, a
//! file, or stdin (`-`).

use std::io::Read;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::error::CliError;

/// Read a text argument from a path, or from stdin when the path is `-`.
pub fn read_text(path: &str, what: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .with_context(|| format!("read {} from stdin", what))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("read {} file {}", what, path))
    }
}

/// Parse a JSON string, mapping syntax errors to `CliError::InvalidInput`
/// (exit code 65).
pub fn parse_json(raw: &str, what: &str) -> Result<Value> {
    serde_json::from_str(raw)
        .map_err(|e| anyhow!(CliError::InvalidInput(format!("{}: {}", what, e))))
}

/// Resolve an inline-JSON flag and its `-file` twin. Returns `None` when
/// neither was passed. `flag` is the bare flag name used in messages,
/// e.g. `overrides` for `--overrides` / `--overrides-file`.
pub fn json_arg(inline: Option<&str>, file: Option<&str>, flag: &str) -> Result<Option<Value>> {
    match (inline, file) {
        (Some(_), Some(_)) => bail!(CliError::InvalidInput(format!(
            "pass either --{0} or --{0}-file, not both",
            flag
        ))),
        (Some(s), None) => Ok(Some(parse_json(s, &format!("--{}", flag))?)),
        (None, Some(p)) => Ok(Some(parse_json(
            &read_text(p, &format!("--{}-file", flag))?,
            &format!("--{}-file", flag),
        )?)),
        (None, None) => Ok(None),
    }
}

/// Reject non-object JSON where the API expects an object.
pub fn require_object(v: &Value, what: &str) -> Result<()> {
    if !v.is_object() {
        bail!(CliError::InvalidInput(format!(
            "{} must be a JSON object, got {}",
            what,
            json_kind(v)
        )));
    }
    Ok(())
}

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}
