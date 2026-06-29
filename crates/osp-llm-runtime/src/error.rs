//! Runtime errors.

use std::fmt;

/// All failures the runtime can produce.
#[derive(Debug)]
pub enum LlmError {
    /// HTTP transport / network failure (DNS, connection, timeout).
    Http(reqwest::Error),
    /// Non-2xx response from the API. Body included for diagnostics.
    Status { code: u16, body: String },
    /// API returned a shape we did not expect (missing `choices`, etc.).
    BadResponse(String),
    /// Assistant message parsed as JSON but failed `DeltaProposal` schema.
    ProposalParse {
        raw: String,
        source: serde_json::Error,
    },
    /// API key was missing or empty.
    MissingApiKey,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "http error: {e}"),
            Self::Status { code, body } => {
                let snippet = if body.len() > 300 {
                    format!("{}…", &body[..300])
                } else {
                    body.clone()
                };
                write!(f, "api status {code}: {snippet}")
            }
            Self::BadResponse(msg) => write!(f, "bad response shape: {msg}"),
            Self::ProposalParse { source, .. } => {
                write!(f, "delta proposal parse failed: {source}")
            }
            Self::MissingApiKey => write!(f, "api key missing or empty"),
        }
    }
}

impl std::error::Error for LlmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::ProposalParse { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}
