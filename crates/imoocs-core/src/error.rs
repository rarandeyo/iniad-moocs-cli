//! Error type and structured exit codes.
//!
//! Exit codes (see plan §CLI Design Principles #8):
//!   0 Success, 1 API, 2 Auth, 3 Validation, 4 NotFound, 5 Internal, 6 Network, 7 NetworkRestricted.

use thiserror::Error;

pub type Result<T, E = ImoocsError> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    Api = 1,
    Auth = 2,
    Validation = 3,
    NotFound = 4,
    Internal = 5,
    Network = 6,
    NetworkRestricted = 7,
}

impl ExitCode {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Error)]
pub enum ImoocsError {
    #[error("authentication required or session expired: {reason}")]
    Auth { reason: String, hint: Option<String> },

    #[error("network restricted: the course/problem requires access from INIAD internal network")]
    NetworkRestricted,

    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {what}")]
    NotFound { what: String },

    #[error("API error: {0}")]
    Api(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl ImoocsError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Auth { .. } => ExitCode::Auth,
            Self::NetworkRestricted => ExitCode::NetworkRestricted,
            Self::Validation(_) => ExitCode::Validation,
            Self::NotFound { .. } => ExitCode::NotFound,
            Self::Api(_) => ExitCode::Api,
            Self::Network(_) | Self::Reqwest(_) => ExitCode::Network,
            Self::Parse(_) | Self::Internal(_) | Self::Io(_) | Self::Json(_) | Self::Anyhow(_) => {
                ExitCode::Internal
            }
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Auth { .. } => "AUTH_EXPIRED",
            Self::NetworkRestricted => "NETWORK_RESTRICTED",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::NotFound { .. } => "NOT_FOUND",
            Self::Api(_) => "API_ERROR",
            Self::Network(_) | Self::Reqwest(_) => "NETWORK_ERROR",
            Self::Parse(_) => "PARSE_ERROR",
            Self::Internal(_) | Self::Io(_) | Self::Json(_) | Self::Anyhow(_) => "INTERNAL_ERROR",
        }
    }

    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Auth { hint, .. } => hint.as_deref(),
            Self::NetworkRestricted => Some("Connect to INIAD network (on-campus or VPN) and retry."),
            _ => None,
        }
    }
}
