use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("ZIP/EPUB archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("XML parse error: {0}")]
    Xml(#[from] roxmltree::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Source error ({source_id}): {message}")]
    Source { source_id: String, message: String },

    #[error("Access restricted by robots.txt for URL: {0}")]
    RobotsDisallowed(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Cancelled: {0}")]
    Cancelled(String),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Serialize)]
pub struct SerializedError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Database(_) => "DATABASE_ERROR",
            AppError::Network(_) => "NETWORK_ERROR",
            AppError::Io(_) => "IO_ERROR",
            AppError::Serde(_) => "SERDE_ERROR",
            AppError::Zip(_) => "ARCHIVE_ERROR",
            AppError::Xml(_) => "XML_ERROR",
            AppError::Url(_) => "URL_ERROR",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Source { .. } => "SOURCE_ERROR",
            AppError::RobotsDisallowed(_) => "ROBOTS_DISALLOWED",
            AppError::InvalidInput(_) => "INVALID_INPUT",
            AppError::Cancelled(_) => "CANCELLED",
            AppError::Other(_) => "INTERNAL_ERROR",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SerializedError {
            code: self.code().to_string(),
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
