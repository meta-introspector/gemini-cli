use thiserror::Error;

/// Gemini API errors
#[derive(Error, Debug)]
pub enum GeminiError {
    #[error("API Error: {0}")]
    ApiError(String),

    #[error("Configuration Error: {0}")]
    ConfigError(String),

    #[error("Request Error: {0}")]
    RequestError(String),

    #[error("Response Error: {0}")]
    ResponseError(String),

    #[error("Parsing Error: {0}")]
    ParsingError(String),

    #[error("HTTP Error: {status_code} - {message}")]
    HttpError { status_code: u16, message: String },

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("Other Error: {0}")]
    OtherError(String),
}

impl GeminiError {
    // Factory methods were removed as they are unused internally
    // pub(crate) fn api_error(...) { ... }
    // pub(crate) fn config_error(...) { ... }
    // pub(crate) fn request_error(...) { ... }
    // pub(crate) fn response_error(...) { ... }
    // pub(crate) fn parsing_error(...) { ... }
    // pub(crate) fn http_error(...) { ... }
    // pub(crate) fn other_error(...) { ... }
}

/// Result type for Gemini operations
pub type GeminiResult<T> = Result<T, GeminiError>;
