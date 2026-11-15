use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("JSON error: {0}")]
    Json(String),
    #[error("Processing error: {0}")]
    Other(String),
}
