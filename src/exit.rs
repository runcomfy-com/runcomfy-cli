//! sysexits-style exit codes.
//!
//! Reference: BSD `sysexits(3)` — https://man.openbsd.org/sysexits.3
//!
//! Scripts can branch on these:
//!   - `64` (EX_USAGE)       — bad CLI args / unknown subcommand (clap also returns 2)
//!   - `65` (EX_DATAERR)     — input data wrong (bad JSON, schema mismatch)
//!   - `66` (EX_NOINPUT)     — a local input file / directory does not exist
//!   - `69` (EX_UNAVAILABLE) — server side 5xx
//!   - `75` (EX_TEMPFAIL)    — retryable: timeout, 408, 429
//!   - `77` (EX_NOPERM)      — auth: 401 / 403, missing or expired token
//!   - `1`                    — generic / unclassified

use crate::error::CliError;

pub const EX_USAGE: i32 = 64;
pub const EX_DATAERR: i32 = 65;
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
            CliError::NotAuthenticated | CliError::TokenRejected | CliError::AuthDenied => {
                EX_NOPERM
            }
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
            CliError::WaitTimeout { .. } => EX_TEMPFAIL,
            CliError::NeedsConfirmation(_) => EX_USAGE,
            CliError::Aborted => 1,
            CliError::TrainingIncomplete { .. } => EX_TEMPFAIL,
        };
    }

    // A missing local input file anywhere in the cause chain (`--input-file`,
    // `train submit --config`, `datasets upload` paths). Matched on the
    // typed `io::ErrorKind` rather than the OS's message text, which differs
    // between platforms ("No such file or directory" vs "The system cannot
    // find the file specified").
    if e.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|io| io.kind() == std::io::ErrorKind::NotFound)
            .unwrap_or(false)
    }) {
        return EX_NOINPUT;
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
    if chain.contains("invalid input") || chain.contains("invalid json") {
        return EX_DATAERR;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_maps_to_noinput_by_error_kind() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = anyhow::Error::from(io).context("read config file ./nope.yaml");
        assert_eq!(classify(&err), EX_NOINPUT);
    }

    #[test]
    fn other_io_errors_stay_generic() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let err = anyhow::Error::from(io).context("read config file");
        assert_eq!(classify(&err), 1);
    }

    #[test]
    fn incomplete_training_is_retryable() {
        let err = anyhow::Error::from(CliError::TrainingIncomplete {
            job_id: "j".into(),
            detail: "stopped at step 1/2".into(),
        });
        assert_eq!(classify(&err), EX_TEMPFAIL);
    }
}
