use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("LLM provider error: {0}")]
    Llm(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

impl From<std::env::VarError> for AppError {
    fn from(e: std::env::VarError) -> Self {
        AppError::Config(e.to_string())
    }
}
