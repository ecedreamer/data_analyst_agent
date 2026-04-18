mod agent;
mod config;
mod db;
mod error;
mod llm;
mod prompt;
mod tool;

use std::io::{self, Write};

use agent::ReActRunner;
use config::AppConfig;
use db::Database;
use error::AppError;
use tool::{ToolEnum, ToolRegistry, query::QueryDatabaseTool, schema::DescribeSchemaTool};

#[tokio::main]
async fn main() -> Result<(), AppError> {
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env()?;
    let db = Database::open(&config.db_path)?;

    // Register all tools. Each tool gets its own clone of the db handle
    // (cheap — it's an Arc<Mutex<Connection>> internally).
    let mut registry = ToolRegistry::new();
    registry.register(ToolEnum::DescribeSchema(DescribeSchemaTool::new(
        db.clone(),
    )));
    registry.register(ToolEnum::QueryDatabase(QueryDatabaseTool::new(db)));

    // Build the system prompt from the live registry — no static file needed.
    let system_prompt = prompt::build_system_prompt(&registry);

    let runner = ReActRunner::new(config.llm.into_provider(), registry, system_prompt);

    let question = prompt_user("Ask your data question: ")?;
    if question.is_empty() {
        return Ok(());
    }

    let answer = runner.run(&question).await;
    println!("\n══ Final Answer ══════════════════════════════════\n{answer}\n");

    Ok(())
}

fn prompt_user(prompt: &str) -> Result<String, AppError> {
    print!("\n{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_owned())
}
