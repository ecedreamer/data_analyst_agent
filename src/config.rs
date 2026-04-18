use std::{env, fs, path::PathBuf};

use crate::{error::AppError, llm::LlmConfig};

pub struct AppConfig {
    pub db_path: PathBuf,
    pub system_prompt: String,
    pub llm: LlmConfig,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let db_path = PathBuf::from(
            env::var("DB_PATH").unwrap_or_else(|_| "./databases/database1.sqlite3".into()),
        );

        let system_prompt_path =
            env::var("SYSTEM_PROMPT_PATH").unwrap_or_else(|_| "system_prompt.txt".into());

        let system_prompt = fs::read_to_string(&system_prompt_path).map_err(|_| {
            AppError::Config(format!(
                "Failed to read system prompt: {system_prompt_path}"
            ))
        })?;

        let llm = LlmConfig::from_env()?;

        Ok(Self {
            db_path,
            system_prompt,
            llm,
        })
    }
}
