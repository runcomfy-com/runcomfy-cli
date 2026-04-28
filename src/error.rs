use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("not authenticated — run `runcomfy login` first")]
    NotAuthenticated,

    #[error("authentication expired — run `runcomfy login` to refresh")]
    AuthExpired,

    #[error("device-code flow timed out after {0}s")]
    AuthTimeout(u64),

    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("unknown skill `{0}` — pass a model_id directly, or run `runcomfy deploy create {0}` first")]
    UnknownSkill(String),

    #[error("invalid input JSON: {0}")]
    InvalidInput(String),
}
