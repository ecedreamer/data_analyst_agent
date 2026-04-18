use crate::{error::AppError, llm::LlmConfig};
use std::{env, path::PathBuf};

pub struct AppConfig {
    pub db_path: PathBuf,
    pub llm: LlmConfig,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let db_path = PathBuf::from(
            env::var("DB_PATH").unwrap_or_else(|_| "./databases/database1.sqlite3".into()),
        );
        let llm = LlmConfig::from_env()?;
        Ok(Self { db_path, llm })
    }
}
