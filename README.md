# SQLite ReAct Agent

A command-line AI agent that answers natural language questions about a SQLite database. Built in Rust using the [ReAct](https://arxiv.org/abs/2210.03629) (Reasoning + Acting) pattern — the agent thinks, picks a tool, observes the result, and repeats until it can give a grounded final answer.

---

## How it works

The agent runs a structured loop. Each step produces exactly one of:

```
Thought:      <reasoning about what to do next>
Action:       <tool name>
Action Input: <input for that tool>
Observation:  <result injected by the runner — never written by the LLM>
Final Answer: <complete answer to the user's question>
```

A full trace looks like this:

```
Question: What was the top-selling product category last month?

Thought: I should check the schema before writing any query.
Action: describe_schema
Action Input: sales
Observation: Schema for 'sales':
| id | INTEGER |
| transaction_date | TEXT |
| product_category | TEXT |
| units_sold | INTEGER |
| total_revenue | REAL |
| region | TEXT |

Thought: I have the columns. I'll group by category and sum units_sold.
Action: query_database
Action Input: SELECT product_category, SUM(units_sold) AS total
              FROM sales
              WHERE strftime('%Y-%m', transaction_date) = '2026-03'
              GROUP BY product_category
              ORDER BY total DESC
              LIMIT 1
Observation: | Electronics | 4821 |

Final Answer: The top-selling category in March 2026 was Electronics with 4,821 units sold.
```

Every step is structurally parsed — not pattern-matched on raw strings. If the LLM produces a malformed response, the runner injects a corrective observation and lets the model recover.

---

## Architecture

```
src/
├── main.rs               # Wiring: registry → prompt → runner
├── config.rs             # Env var resolution into AppConfig
├── error.rs              # Unified AppError (thiserror)
├── prompt.rs             # Dynamic system prompt built from the live registry
├── llm.rs                # LlmProvider trait + OpenAI/Gemini impls (enum dispatch)
├── db.rs                 # Thread-safe SQLite wrapper, SELECT-only enforcement
├── agent/
│   ├── mod.rs
│   ├── runner.rs         # ReActRunner — the Thought/Action/Observation loop
│   └── trace.rs          # AgentStep enum + strict parser
└── tool/
    ├── mod.rs            # Tool trait + ToolEnum dispatch + ToolRegistry
    ├── schema.rs         # describe_schema — live PRAGMA introspection
    └── query.rs          # query_database — SELECT execution
```

### Key design decisions

**Enum dispatch instead of `Box<dyn Trait>`**
Native async fn in traits (stable since Rust 1.75) are not object-safe, so `Box<dyn LlmProvider>` and `Box<dyn Tool>` don't compile without `async_trait`. Both layers use a hand-rolled closed-set enum (`Provider`, `ToolEnum`) for zero-cost static dispatch — no heap allocation per call, no macro dependency.

**Structured trace parsing (`trace.rs`)**
`AgentStep::parse()` classifies every LLM response into a typed variant before the runner acts on it. Priority order: `FinalAnswer` → `Action` → `Thought` → `Malformed`. This prevents silent failures when the model skips a tag or puts `Final Answer` after an `Action` line.

**Live schema discovery (`schema.rs`)**
The `describe_schema` tool runs `PRAGMA table_info` against the live database. The system prompt contains no hardcoded column names — the model must always discover them at runtime. This means the agent stays correct after schema migrations without any code or prompt changes.

**Dynamic system prompt (`prompt.rs`)**
`build_system_prompt(&registry)` generates the prompt from the registered tools at startup. Adding a new tool automatically updates the prompt the model sees — no manual editing required.

**SELECT-only database layer (`db.rs`)**
`Database::query()` rejects any SQL that doesn't contain `SELECT` before it reaches the database engine. Errors are typed (`QueryError::Forbidden`, `::Syntax`, `::Execution`) so callers can distinguish policy violations from SQL mistakes.

---

## Requirements

- Rust 1.75 or later (required for native async fn in traits)
- An OpenAI or Google Gemini API key
- A SQLite database file

---

## Setup

### 1. Clone and build

```bash
git clone <repo-url>
cd sqlite-agent
cargo build --release
```

### 2. Configure environment

Copy the example env file and fill in your values:

```bash
cp .env.example .env
```

`.env.example`:

```env
# LLM provider: "openai" (default) or "gemini"
MODEL_PROVIDER=openai

# OpenAI
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-4o

# Gemini (used when MODEL_PROVIDER=gemini)
GEMINI_API_KEY=...
GEMINI_MODEL=gemini-1.5-flash

# Path to your SQLite database
DB_PATH=./databases/database1.sqlite3
```

### 3. Run

```bash
cargo run --release
```

```
Ask your data question: Which region had the highest revenue in Q1 2026?
```

---

## Adding a new tool

1. Create `src/tool/mytool.rs` and implement the `Tool` trait:

```rust
use crate::{db::Database, error::AppError};
use super::Tool;

pub struct MyTool { /* ... */ }

impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }
    fn description(&self) -> &'static str { "Does X. Input: Y." }

    async fn invoke(&self, input: &str) -> Result<String, AppError> {
        // ...
        Ok(result)
    }
}
```

2. Add a variant to `ToolEnum` in `src/tool/mod.rs`:

```rust
pub enum ToolEnum {
    DescribeSchema(schema::DescribeSchemaTool),
    QueryDatabase(query::QueryDatabaseTool),
    MyTool(mytool::MyTool),           // ← add this
}

impl Tool for ToolEnum {
    // add the matching arm to name(), description(), invoke()
}
```

3. Register it in `main.rs`:

```rust
registry.register(ToolEnum::MyTool(MyTool::new()));
```

The tool name and description are automatically injected into the system prompt. No other changes needed.

---

## Adding a new LLM provider

1. Add a struct implementing `LlmProvider` in `src/llm.rs`.
2. Add a variant to `Provider` and `ProviderKind`.
3. Add the match arm in `LlmConfig::from_env()` and `LlmConfig::into_provider()`.

---

## Dependencies

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime |
| `reqwest` | HTTP client for LLM APIs |
| `rusqlite` | SQLite driver (bundled, no system lib needed) |
| `serde_json` | JSON serialisation for API payloads |
| `thiserror` | Ergonomic error type derivation |
| `dotenvy` | `.env` file loading |
| `chrono` | Current date injection into prompts |

---

## Limitations

- **Single database connection** — the `Arc<Mutex<Connection>>` serialises all queries. Sufficient for a CLI tool; replace with a connection pool (`r2d2`, `deadpool`) for concurrent workloads.
- **Closed tool set** — adding a tool requires a new `ToolEnum` variant and recompilation. This is intentional (zero-cost dispatch) but means no runtime plugin loading.
- **No conversation memory** — each `run()` call starts with a fresh history. Multi-turn sessions are not supported.
- **Row cap** — results are truncated at 15 rows to avoid overflowing the context window.
