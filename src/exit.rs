//! sysexits-style exit codes.
//!
//! Reference: BSD `sysexits(3)` — https://man.openbsd.org/sysexits.3
//!
//! Scripts can branch on these:
//!   - `64` (EX_USAGE)       — bad CLI args / unknown subcommand (clap also returns 2)
//!   - `65` (EX_DATAERR)     — input data wrong (bad JSON, schema mismatch)
//!   - `69` (EX_UNAVAILABLE) — server side 5xx
//!   - `75` (EX_TEMPFAIL)    — retryable: timeout, 408, 429
//!   - `77` (EX_NOPERM)      — auth: 401 / 403, missing or expired token
//!   - `1`                    — generic / unclassified

use crate::error::CliError;

pub const EX_USAGE: i32 = 64;
pub const EX_DATAERR: i32 = 65;
#[allow(dead_code)]
pub const EX_NOINPUT: i32 = 66;
pub const EX_UNAVAILABLE: i32 = 69;
#[allow(dead_code)]
pub const EX_SOFTWARE: i32 = 70;
pub const EX_TEMPFAIL: i32 = 75;
pub const EX_NOPERM: i32 = 77;

/// Map an `anyhow::Error` (typically wrapping `CliError`) to a sysexits-style
/// exit code. Falls back to `1` for unclassified errors.
pub fn classify(e: &anyhow::Error) -> i32 {
    if let Some(c) = e.downcast_ref::<CliError>() {
        return match c {
            CliError::NotAuthenticated | CliError::AuthExpired => EX_NOPERM,
            CliError::AuthTimeout(_) => EX_TEMPFAIL,
            CliError::Api { status, .. } => {
                let s = *status;
                if s == 401 || s == 403 {
                    EX_NOPERM
                } else if s == 408 || s == 429 {
                    EX_TEMPFAIL
                } else if (500..600).contains(&s) {
                    EX_UNAVAILABLE
                } else if (400..500).contains(&s) {
                    EX_DATAERR
                } else {
                    1
                }
            }
            CliError::UnknownSkill(_) => EX_USAGE,
            CliError::InvalidInput(_) => EX_DATAERR,
        };
    }

    // Walk the error chain looking for a string that hints at category.
    // This is a fallback for callers that didn't wrap their error in CliError.
    let chain = format!("{:#}", e).to_lowercase();
    if chain.contains("401") || chain.contains("unauthorized") || chain.contains("not signed in") {
        return EX_NOPERM;
    }
    if chain.contains("timed out") || chain.contains("timeout") || chain.contains("429") {
        return EX_TEMPFAIL;
    }
    if chain.contains("invalid input json") || chain.contains("invalid json") {
        return EX_DATAERR;
    }
    1
}
