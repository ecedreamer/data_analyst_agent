use chrono::Local;

use crate::{
    db::Database,
    llm::{LlmConfig, LlmProvider, Provider},
};

const MAX_STEPS: usize = 5;
const STOP_SEQUENCE: &str = "Observation:";

// ── Public surface ─────────────────────────────────────────────────────────────

pub struct SQLiteAgent {
    db: Database,
    llm: Provider,
    system_prompt: String,
}

impl SQLiteAgent {
    pub fn new(db: Database, llm_config: LlmConfig, system_prompt: String) -> Self {
        Self {
            db,
            llm: llm_config.into_provider(),
            system_prompt,
        }
    }

    /// Run the ReAct (Reasoning → Action → Observation) loop until the agent
    /// emits a Final Answer or exhausts `MAX_STEPS`.
    pub async fn run(&self, question: &str) {
        let mut history = self.build_initial_history(question);

        for step in 1..=MAX_STEPS {
            println!("\n--- Step {step} ---");

            let response = match self.llm.complete(&history, &[STOP_SEQUENCE]).await {
                Ok(r) if r.is_empty() => {
                    eprintln!("[Agent]: Empty response – stopping.");
                    break;
                }
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[Error]: {e}");
                    break;
                }
            };

            println!("[Agent]: {response}");
            history.push_str(&response);

            if response.contains("Final Answer:") {
                break;
            }

            if let Some(sql) = extract_sql_block(&response) {
                println!("[System]: Querying database…");
                let observation = match self.db.query(sql) {
                    Ok(rows) => rows,
                    Err(e) => format!("Query failed: {e}"),
                };
                println!("[Database]: {observation}");
                history.push_str(&format!("\n{STOP_SEQUENCE} {observation}\n"));
            }
        }
    }

    fn build_initial_history(&self, question: &str) -> String {
        let date = Local::now().format("%Y-%m-%d");
        format!(
            "{}\nCurrent Date: {date}.\nQuestion: {question}\n",
            self.system_prompt
        )
    }
}

// ── Standalone helpers ─────────────────────────────────────────────────────────

/// Extract the first fenced SQL block (` ```sql … ``` `) from a response string.
fn extract_sql_block(text: &str) -> Option<&str> {
    const OPEN: &str = "```sql";
    const CLOSE: &str = "```";

    let start = text.find(OPEN)? + OPEN.len();
    let end = text[start..].find(CLOSE)?;
    Some(text[start..start + end].trim())
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sql_block_happy_path() {
        let input = "Thought: ...\n```sql\nSELECT 1;\n```\n";
        assert_eq!(extract_sql_block(input), Some("SELECT 1;"));
    }

    #[test]
    fn extract_sql_block_missing_returns_none() {
        assert_eq!(extract_sql_block("no sql here"), None);
    }

    #[test]
    fn extract_sql_block_unterminated_returns_none() {
        assert_eq!(extract_sql_block("```sql\nSELECT 1;"), None);
    }
}
