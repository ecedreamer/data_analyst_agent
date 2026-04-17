use rusqlite::{Connection, Result, types::ValueRef};
use serde_json::json;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::{env, fs, io};

// --- Model Configuration ---
pub struct ModelConfig {
    pub model_name: String,
    pub api_key: String,
}

enum LlmProvider {
    OpenAI(ModelConfig),
    Gemini(ModelConfig),
}

impl LlmProvider {
    async fn completion(&self, prompt: &str, stop_sequences: Vec<String>) -> String {
        let client = reqwest::Client::new();

        match self {
            LlmProvider::OpenAI(config) => {
                let body = json!({
                    "model": config.model_name,
                    "messages": [{"role": "user", "content": prompt}],
                    "stop": stop_sequences,
                    "temperature": 0.0
                });

                match client
                    .post("https://api.openai.com/v1/chat/completions")
                    .bearer_auth(&config.api_key)
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(res) => {
                        let json: serde_json::Value = res.json().await.unwrap_or_default();
                        json["choices"][0]["message"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string()
                    }
                    Err(e) => format!("OpenAI Error: {}", e),
                }
            }
            LlmProvider::Gemini(config) => {
                let url = format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                    config.model_name.trim(),
                    config.api_key
                );

                let body = json!({
                    "contents": [{ "parts": [{ "text": prompt }] }],
                    // ADDING SAFETY SETTINGS HERE
                    "safetySettings": [
                        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
                        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
                        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
                        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" }
                    ],
                    "generationConfig": {
                        "stopSequences": stop_sequences,
                        "temperature": 0.0,
                        "maxOutputTokens": 2048
                    }
                });

                match client.post(&url).json(&body).send().await {
                    Ok(res) => {
                        let json: serde_json::Value = res.json().await.unwrap_or_default();

                        // println!("RAW JSON: {}", json);

                        if let Some(candidates) = json["candidates"].as_array() {
                            if let Some(candidate) = candidates.get(0) {
                                if let Some(text) =
                                    candidate["content"]["parts"][0]["text"].as_str()
                                {
                                    return text.to_string();
                                }

                                // Check if it was blocked by safety
                                if let Some(reason) = candidate["finishReason"].as_str() {
                                    if reason == "SAFETY" {
                                        return "Error: Gemini blocked the response due to Safety Filters. Check safetySettings.".to_string();
                                    }
                                    return format!("Gemini stopped. Reason: {}", reason);
                                }
                            }
                        }

                        // Check for top-level API errors (e.g., Invalid API Key)
                        if let Some(error) = json["error"]["message"].as_str() {
                            return format!("API Error: {}", error);
                        }

                        "Error: Empty or malformed response from Gemini.".to_string()
                    }
                    Err(e) => format!("Gemini Network Error: {}", e),
                }
            }
        }
    }
}

struct SQLiteAgent {
    db: Arc<Mutex<Connection>>,
    llm: LlmProvider,
}

impl SQLiteAgent {
    fn execute_query(&self, sql: &str) -> String {
        let conn = self.db.lock().unwrap();
        let sql_trimmed = sql.trim().trim_matches('`').trim_matches(';');

        if !sql_trimmed.to_uppercase().contains("SELECT") {
            return "Error: Restricted to SELECT queries.".to_string();
        }

        match conn.prepare(sql_trimmed) {
            Ok(mut stmt) => {
                let col_count = stmt.column_count();
                let mut results = String::new();
                let mut rows = match stmt.query([]) {
                    Ok(r) => r,
                    Err(e) => return format!("SQL Execution Error: {}", e),
                };

                let mut count = 0;
                while let Some(row) = rows.next().unwrap_or(None) {
                    count += 1;
                    if count > 15 {
                        break;
                    }
                    let mut row_str = String::new();
                    for i in 0..col_count {
                        let val = match row.get_ref_unwrap(i) {
                            ValueRef::Null => "NULL".to_string(),
                            ValueRef::Integer(i) => i.to_string(),
                            ValueRef::Real(f) => f.to_string(),
                            ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                            _ => "data".to_string(),
                        };
                        row_str.push_str(&format!("{} | ", val));
                    }
                    results.push_str(&format!("| {} \n", row_str));
                }
                if results.is_empty() {
                    "0 rows returned.".to_string()
                } else {
                    results
                }
            }
            Err(e) => format!("SQL Syntax Error: {}", e),
        }
    }

    pub async fn run(&self, system_prompt: &str, user_question: &str) {
        let current_date = format!("Current Date: {}.", chrono::Local::now().format("%Y-%m-%d"));

        let mut history = format!(
            "{}\n{}\nQuestion: {}\n",
            system_prompt, current_date, user_question
        );

        for i in 0..5 {
            println!("\n--- Step {} ---", i + 1);
            let response = self
                .llm
                .completion(&history, vec!["Observation:".to_string()])
                .await;

            if response.is_empty() {
                break;
            }
            println!("[Agent]: {}", response);
            history.push_str(&response);

            if response.contains("Final Answer:") {
                break;
            }

            if let Some(start) = response.find("```sql") {
                let after_block = &response[start + 6..];
                if let Some(end) = after_block.find("```") {
                    let query = after_block[..end].trim();
                    println!("[System]: Querying Database...");
                    let observation = self.execute_query(query);
                    let obs_text = format!("\nObservation: {}\n", observation);
                    println!("[Database]: {}", obs_text);
                    history.push_str(&obs_text);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let system_prompt =
        fs::read_to_string("system_prompt.txt").expect("Failed to read system_prompt.txt");

    let provider_name = env::var("MODEL_PROVIDER").unwrap_or_else(|_| "openai".into());
    let llm = match provider_name.as_str() {
        "gemini" => LlmProvider::Gemini(ModelConfig {
            model_name: env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-1.5-flash".into()),
            api_key: env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set"),
        }),
        _ => LlmProvider::OpenAI(ModelConfig {
            model_name: env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into()),
            api_key: env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        }),
    };

    let conn = Connection::open("./databases/database1.sqlite3")?;
    let agent = SQLiteAgent {
        db: Arc::new(Mutex::new(conn)),
        llm,
    };

    print!("\nAsk your data question: ");
    io::stdout().flush()?; // Ensure the prompt prints before input
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;
    let user_input = user_input.trim();

    if !user_input.is_empty() {
        agent.run(&system_prompt, user_input).await;
    }

    Ok(())
}
