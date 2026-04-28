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
    #[error("unknown skill `{0}` — pass a model_id directly, or run `runcomfy deploy create {0}` first")]
    #[allow(dead_code)]
    UnknownSkill(String),

    /// User-supplied input JSON didn't parse / validate.
    #[error("invalid input JSON: {0}")]
    InvalidInput(String),
}
