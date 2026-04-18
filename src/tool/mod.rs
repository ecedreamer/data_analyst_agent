pub mod query;
pub mod schema;

use crate::error::AppError;
use std::collections::HashMap;

// ── Tool trait ────────────────────────────────────────────────────────────────

/// Every tool the agent can invoke must implement this.
///
/// Native async fn in trait (stable 1.75) — no Box::pin, no async_trait.
/// Not object-safe by design; the closed `ToolEnum` below handles dispatch.
pub trait Tool {
    /// Unique name the LLM uses in `Action:` lines.
    fn name(&self) -> &'static str;

    /// One-line description injected into the system prompt.
    fn description(&self) -> &'static str;

    /// Execute with the raw `Action Input:` string and return an observation.
    async fn invoke(&self, input: &str) -> Result<String, AppError>;
}

// ── Closed-set enum dispatch (avoids Box<dyn Tool>) ──────────────────────────

/// Every registered tool variant. Add a new variant here to register a tool.
pub enum ToolEnum {
    DescribeSchema(schema::DescribeSchemaTool),
    QueryDatabase(query::QueryDatabaseTool),
}

impl Tool for ToolEnum {
    fn name(&self) -> &'static str {
        match self {
            ToolEnum::DescribeSchema(t) => t.name(),
            ToolEnum::QueryDatabase(t) => t.name(),
        }
    }

    fn description(&self) -> &'static str {
        match self {
            ToolEnum::DescribeSchema(t) => t.description(),
            ToolEnum::QueryDatabase(t) => t.description(),
        }
    }

    async fn invoke(&self, input: &str) -> Result<String, AppError> {
        match self {
            ToolEnum::DescribeSchema(t) => t.invoke(input).await,
            ToolEnum::QueryDatabase(t) => t.invoke(input).await,
        }
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Holds all available tools indexed by name.
/// The agent uses this to dispatch `Action:` lines and build the system prompt.
pub struct ToolRegistry {
    tools: HashMap<&'static str, ToolEnum>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: ToolEnum) {
        self.tools.insert(tool.name(), tool);
    }

    /// Invoke a tool by name. Returns an error observation if name is unknown.
    pub async fn invoke(&self, name: &str, input: &str) -> String {
        match self.tools.get(name) {
            Some(tool) => match tool.invoke(input).await {
                Ok(obs) => obs,
                Err(e) => format!("Tool error: {e}"),
            },
            None => format!(
                "Unknown tool '{name}'. Available: {}",
                self.available_names().join(", ")
            ),
        }
    }

    /// Descriptions formatted for injection into the system prompt.
    pub fn prompt_block(&self) -> String {
        let mut lines: Vec<String> = self
            .tools
            .values()
            .map(|t| format!("  - {}: {}", t.name(), t.description()))
            .collect();
        lines.sort(); // deterministic ordering
        lines.join("\n")
    }

    fn available_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.tools.keys().copied().collect();
        names.sort();
        names
    }
}
