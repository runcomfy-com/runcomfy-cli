use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    /// No local token at all (user never logged in).
    #[error("not signed in — run `runcomfy login` first, or set RUNCOMFY_TOKEN")]
    NotAuthenticated,

    /// Server rejected the token we have. Could be expired, revoked, or
    /// just plain wrong. We don't always know which.
    #[error("authentication failed — token rejected by server. Run `runcomfy login` to refresh, or check RUNCOMFY_TOKEN if you set it manually")]
    TokenRejected,

    /// Device-code flow itself timed out (we never got an authorize click).
    #[error("device-code flow timed out after {0}s — run `runcomfy login` again")]
    AuthTimeout(u64),

    /// User explicitly clicked Deny on the authorize page.
    #[error("authorization was denied")]
    AuthDenied,

    /// Generic upstream HTTP error with a passthrough message.
    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },

    /// Reserved for v0.2 (auto-deploy of named workflow skills).
    #[error(
        "unknown skill `{0}` — pass a model_id directly, or run `runcomfy deploy create {0}` first"
    )]
    #[allow(dead_code)]
    UnknownSkill(String),

    /// User-supplied input (JSON payload, config file, upload paths, flag
    /// combination) didn't parse / validate.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// `--wait` / `--timeout` elapsed while the remote job kept running.
    #[error("timed out after {secs}s waiting for {what}; it is still running remotely — {hint}")]
    WaitTimeout {
        what: String,
        secs: u64,
        hint: String,
    },

    /// A destructive command was run non-interactively without `--yes`.
    #[error("refusing to {0} without confirmation — re-run with --yes, or answer the prompt in a terminal")]
    NeedsConfirmation(String),

    /// The user answered "no" to a confirmation prompt.
    #[error("aborted")]
    Aborted,

    /// A training job reported `STOPPED` without reaching its final step
    /// (preempted / reclaimed by the platform). It can be resumed from its
    /// latest checkpoint, so this maps to the retryable exit code.
    #[error("training job {job_id} did not finish: {detail}")]
    TrainingIncomplete { job_id: String, detail: String },
}
