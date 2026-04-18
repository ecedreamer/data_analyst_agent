mod agent;
mod config;
mod db;
mod error;
mod llm;

use std::io::{self, Write};

use agent::SQLiteAgent;
use config::AppConfig;
use db::Database;
use error::AppError;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env()?;
    let db = Database::open(&config.db_path)?;
    let agent = SQLiteAgent::new(db, config.llm, config.system_prompt);

    let question = prompt_user("Ask your data question: ")?;
    if question.is_empty() {
        return Ok(());
    }

    agent.run(&question).await;

    Ok(())
}

fn prompt_user(prompt: &str) -> Result<String, AppError> {
    print!("\n{prompt}");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_owned())
}
