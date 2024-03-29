use std::error;
use std::fmt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

// ANCHOR: ErrorResponse
#[skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "Error Response")]
pub struct ErrorResponse {
    /// A human-readable summary of the error
    pub message: String,
    /// Any additional structured information about the error
    pub details: serde_json::Value,
}
// ANCHOR_E

#[derive(Debug, Clone)]
pub struct ConnectorError {
    pub status: reqwest::StatusCode,
    pub error_response: ErrorResponse,
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ConnectorError {{ status: {0}, error_response.message: {1} }}",
            self.status, self.error_response.message
        )
    }
}

#[derive(Debug)]
pub enum Error {
    Reqwest(reqwest::Error),
    Serde(serde_json::Error),
    Io(std::io::Error),
    ConnectorError(ConnectorError),
    InvalidBaseURL,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (module, e) = match self {
            Error::Reqwest(e) => ("reqwest", e.to_string()),
            Error::Serde(e) => ("serde", e.to_string()),
            Error::Io(e) => ("IO", e.to_string()),
            Error::ConnectorError(e) => ("response", format!("status code {}", e.status)),
            Error::InvalidBaseURL => ("url", "invalid base URL".into()),
        };
        write!(f, "error in {}: {}", module, e)
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(match self {
            Error::Reqwest(e) => e,
            Error::Serde(e) => e,
            Error::Io(e) => e,
            Error::ConnectorError(_) => return None,
            Error::InvalidBaseURL => return None,
        })
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Reqwest(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serde(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub fn urlencode<T: AsRef<str>>(s: T) -> String {
    ::url::form_urlencoded::byte_serialize(s.as_ref().as_bytes()).collect()
}

// pub mod default_api;

// pub mod configuration;