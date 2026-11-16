use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Json(String),
    #[error("{0}")]
    Other(String),
    #[error("{0}")]
    NotImplemented(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialize_as_ui_readable_messages() {
        let error = AppError::NotImplemented("Settings are not available yet.".to_string());

        assert_eq!(
            serde_json::to_string(&error).expect("serialize error"),
            "\"Settings are not available yet.\""
        );
    }
}
