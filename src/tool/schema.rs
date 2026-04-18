use super::Tool;
use crate::{db::Database, error::AppError};

/// Introspects table columns via `PRAGMA table_info`.
/// Lets the LLM discover schema at runtime instead of relying on a stale prompt.
pub struct DescribeSchemaTool {
    db: Database,
}

impl DescribeSchemaTool {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

impl Tool for DescribeSchemaTool {
    fn name(&self) -> &'static str {
        "describe_schema"
    }

    fn description(&self) -> &'static str {
        "Returns column names and types for a table. Input: table name."
    }

    async fn invoke(&self, input: &str) -> Result<String, AppError> {
        let table = input.trim();
        // PRAGMA is not a SELECT but is always read-only; bypass the SELECT guard.
        let sql = format!("SELECT name, type FROM pragma_table_info('{table}')");
        match self.db.query(&sql) {
            Ok(rows) if rows == "0 rows returned." => {
                Ok(format!("Table '{table}' not found or has no columns."))
            }
            Ok(rows) => Ok(format!("Schema for '{table}':\n{rows}")),
            Err(e) => Ok(format!("Schema lookup failed: {e}")),
        }
    }
}
