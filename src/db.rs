use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, types::ValueRef};

use crate::error::AppError;

const MAX_ROWS: usize = 15;

/// Thread-safe wrapper around a single SQLite connection.
///
/// Only `SELECT` statements are permitted; all other DML/DDL is rejected
/// before reaching the database.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Execute a read-only SQL query and return results formatted as a
    /// pipe-delimited table string. Returns an `Err` on policy or SQL errors.
    pub fn query(&self, sql: &str) -> Result<String, QueryError> {
        let sql = sanitize(sql)?;
        let conn = self.conn.lock().expect("DB mutex poisoned");

        let mut stmt = conn.prepare(sql).map_err(QueryError::Syntax)?;
        let col_count = stmt.column_count();

        let mut rows = stmt.query([]).map_err(QueryError::Execution)?;
        let mut output = String::new();
        let mut count = 0usize;

        while let Some(row) = rows.next().map_err(QueryError::Execution)? {
            if count >= MAX_ROWS {
                output.push_str(&format!("… (output truncated at {MAX_ROWS} rows)\n"));
                break;
            }
            count += 1;

            let mut row_parts = String::new();
            for i in 0..col_count {
                let cell = format_cell(row.get_ref_unwrap(i));
                row_parts.push_str(&cell);
                row_parts.push_str(" | ");
            }
            output.push_str(&format!("| {row_parts}\n"));
        }

        if output.is_empty() {
            Ok("0 rows returned.".into())
        } else {
            Ok(output)
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Strip common decorations from an LLM-generated SQL block and enforce
/// the SELECT-only policy.
fn sanitize(raw: &str) -> Result<&str, QueryError> {
    let sql = raw.trim().trim_matches('`').trim_matches(';').trim();

    if !sql.to_uppercase().contains("SELECT") {
        return Err(QueryError::Forbidden);
    }

    Ok(sql)
}

fn format_cell(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => "NULL".into(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(_) => "<blob>".into(),
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum QueryError {
    /// Non-SELECT statement attempted.
    Forbidden,
    /// SQL failed to prepare (syntax error, etc.).
    Syntax(rusqlite::Error),
    /// SQL prepared but failed at execution time.
    Execution(rusqlite::Error),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryError::Forbidden => write!(f, "Only SELECT queries are permitted."),
            QueryError::Syntax(e) => write!(f, "SQL syntax error: {e}"),
            QueryError::Execution(e) => write!(f, "SQL execution error: {e}"),
        }
    }
}
