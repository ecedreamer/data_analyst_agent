use super::Tool;
use crate::{db::Database, error::AppError};

/// Executes a SELECT query and returns pipe-delimited rows.
pub struct QueryDatabaseTool {
    db: Database,
}

impl QueryDatabaseTool {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

impl Tool for QueryDatabaseTool {
    fn name(&self) -> &'static str {
        "query_database"
    }

    fn description(&self) -> &'static str {
        "Runs a read-only SQL SELECT query. Input: valid SQLite SELECT statement."
    }

    async fn invoke(&self, input: &str) -> Result<String, AppError> {
        match self.db.query(input) {
            Ok(rows) => Ok(rows),
            Err(e) => Ok(format!("Query error: {e}")),
        }
    }
}
