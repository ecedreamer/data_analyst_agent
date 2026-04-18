# SQLite ReAct Agent

A command-line AI agent that answers natural language questions about a SQLite database. Built in Rust using the [ReAct](https://arxiv.org/abs/2210.03629) (Reasoning + Acting) pattern — the agent reasons, selects a tool, observes the result, and repeats until it produces a grounded final answer.

---

## How it works

The agent runs a structured loop. Each LLM response is parsed into exactly one typed step:

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

Every step is structurally parsed into a typed Rust enum — not pattern-matched on raw strings. Malformed responses get a corrective observation so the model can recover without crashing the loop.

---

## Project structure

```
src/
├── main.rs               # Startup: wires registry → prompt → runner
├── config.rs             # Env var resolution into AppConfig
├── error.rs              # Unified AppError via thiserror
├── prompt.rs             # Dynamic system prompt built from the live registry
├── llm.rs                # LlmProvider trait + OpenAI/Gemini enum dispatch
├── db.rs                 # Thread-safe SQLite wrapper, SELECT-only policy
├── agent/
│   ├── mod.rs            # Public re-export of ReActRunner
│   ├── runner.rs         # The Thought → Action → Observation loop
│   └── trace.rs          # AgentStep enum + strict parser
└── tool/
    ├── mod.rs            # Tool trait + ToolEnum dispatch + ToolRegistry
    ├── schema.rs         # describe_schema — live PRAGMA introspection
    └── query.rs          # query_database — SELECT execution
```

---

## Architecture

### Error handling — `error.rs`

All fallible paths in the crate converge on a single `AppError` enum derived with [`thiserror`](https://docs.rs/thiserror):

```rust
#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("LLM provider error: {0}")]
    Llm(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}
```

`#[from]` derives `From<T>` implementations automatically, so `?` propagation works across module boundaries without manual conversion boilerplate. `Config` and `Llm` use owned `String` because those errors originate as formatted messages, not from external types.

`db.rs` defines its own `QueryError` enum separately because database errors have three meaningfully distinct causes (`Forbidden`, `Syntax`, `Execution`) that callers may want to match on individually — collapsing them into `AppError::Database` would lose that distinction.

---

### LLM abstraction — `llm.rs`

The `LlmProvider` trait uses native async fn in traits, stable since Rust 1.75 (RPITIT — Return Position Impl Trait in Traits):

```rust
pub trait LlmProvider {
    async fn complete(&self, prompt: &str, stop_sequences: &[&str]) -> Result<String, AppError>;
}
```

Each impl gets its own concrete future type, monomorphised at compile time — no heap allocation per call. The trade-off is that the trait is **not object-safe**, so `Box<dyn LlmProvider>` does not compile. The solution is a closed-set `Provider` enum that delegates via `match`:

```rust
pub enum Provider {
    OpenAI(OpenAiProvider),
    Gemini(GeminiProvider),
}

impl LlmProvider for Provider {
    async fn complete(&self, prompt: &str, stop_sequences: &[&str]) -> Result<String, AppError> {
        match self {
            Provider::OpenAI(p) => p.complete(prompt, stop_sequences).await,
            Provider::Gemini(p) => p.complete(prompt, stop_sequences).await,
        }
    }
}
```

The `match` is resolved at compile time. Call sites hold a `Provider` value — a concrete type with a known size — so no vtable, no indirection, and no `async_trait` macro dependency.

`stop_sequences` is `&[&str]` rather than `Vec<String>` — a slice reference is zero-copy and the idiomatic Rust way to express "a sequence I don't own and won't outlive this call."

`LlmConfig::into_provider()` acts as a factory, consuming the config and constructing the correct `Provider` variant. The single `reqwest::Client` is constructed once here and shared across requests via internal `Arc`.

---

### Database layer — `db.rs`

`Database` wraps a single SQLite connection behind `Arc<Mutex<Connection>>`:

```rust
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}
```

`Clone` is cheap — it increments the `Arc` reference count. This lets each tool hold its own `Database` handle without duplicating the connection or requiring lifetime annotations at call sites. All clones share the same underlying `Mutex<Connection>`, so queries are serialised correctly.

The SELECT-only policy is enforced in `sanitize()` before any SQL reaches the engine:

```rust
fn sanitize(raw: &str) -> Result<&str, QueryError> {
    let sql = raw.trim().trim_matches('`').trim_matches(';').trim();
    if !sql.to_uppercase().contains("SELECT") {
        return Err(QueryError::Forbidden);
    }
    Ok(sql)
}
```

The trimming strips backtick fences and trailing semicolons that the LLM commonly emits. Returning `&str` (a slice of the input) avoids an allocation — the caller gets a borrow into the original string.

`format_cell` matches exhaustively on `rusqlite::ValueRef`, which borrows directly from the row buffer without copying. Each cell is converted to `String` only at the point of formatting, not during the query loop:

```rust
fn format_cell(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null        => "NULL".into(),
        ValueRef::Integer(i)  => i.to_string(),
        ValueRef::Real(f)     => f.to_string(),
        ValueRef::Text(t)     => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(_)     => "<blob>".into(),
    }
}
```

Results are capped at 15 rows via a `const MAX_ROWS: usize = 15` guard to prevent context window overflow. A truncation notice is appended so the model knows the result was cut.

---

### Tool system — `tool/`

Three pieces compose the tool layer.

**`Tool` trait** defines the contract every tool must satisfy:

```rust
pub trait Tool {
    fn name(&self) -> &'static str;         // used as the registry key
    fn description(&self) -> &'static str;  // injected into the system prompt
    async fn invoke(&self, input: &str) -> Result<String, AppError>;
}
```

`name()` and `description()` return `&'static str` — these are compile-time string literals, so no allocation and no lifetime juggling at the call site.

**`ToolEnum`** mirrors the same pattern as `Provider` — a closed-set enum that implements `Tool` by delegating to its inner value:

```rust
pub enum ToolEnum {
    DescribeSchema(schema::DescribeSchemaTool),
    QueryDatabase(query::QueryDatabaseTool),
}
```

This is the same zero-cost enum dispatch pattern as `LlmProvider`/`Provider`. Adding a new tool means adding one variant here and the corresponding `match` arms — nothing else changes.

**`ToolRegistry`** is a `HashMap<&'static str, ToolEnum>` indexed by tool name:

```rust
pub struct ToolRegistry {
    tools: HashMap<&'static str, ToolEnum>,
}
```

Using `&'static str` as the key (rather than `String`) avoids allocating keys at insert time and makes lookups a direct pointer comparison. `invoke()` on the registry returns `String` (not `Result`) because tool errors are themselves valid observations — the agent should see "Query error: ..." and reason about it, not crash:

```rust
pub async fn invoke(&self, name: &str, input: &str) -> String {
    match self.tools.get(name) {
        Some(tool) => match tool.invoke(input).await {
            Ok(obs) => obs,
            Err(e)  => format!("Tool error: {e}"),
        },
        None => format!("Unknown tool '{name}'. Available: {}", ...),
    }
}
```

`prompt_block()` collects tool descriptions, sorts them for deterministic output, and joins them into the string that gets embedded in the system prompt. Sorting matters because `HashMap` iteration order is non-deterministic — without it, the prompt would change between runs.

**`describe_schema`** (`schema.rs`) uses `pragma_table_info` via a SELECT wrapper so it passes the `sanitize()` guard:

```rust
let sql = format!("SELECT name, type FROM pragma_table_info('{table}')");
```

This means the model always queries the live schema rather than relying on a hardcoded description in the prompt, which would drift after any schema migration.

---

### Trace parsing — `agent/trace.rs`

Every LLM response is classified into one of four variants before the runner acts on it:

```rust
#[derive(Debug, PartialEq)]
pub enum AgentStep {
    Thought(String),
    Action { tool: String, input: String },
    FinalAnswer(String),
    Malformed(String),
}
```

`AgentStep::parse()` applies tags in priority order. `FinalAnswer` is checked first because a single response can legally contain both `Action:` text and `Final Answer:` text — without priority, the parser would misclassify it:

```rust
pub fn parse(response: &str) -> Self {
    if let Some(rest) = find_tag(response, "Final Answer:") { ... }
    if let Some(line) = find_tag(response, "Action:")       { ... }
    if let Some(text) = find_tag(response, "Thought:")      { ... }
    AgentStep::Malformed(response.trim().to_owned())
}
```

`find_tag` returns `Option<&str>` — a slice into the original response string — so no intermediate allocations occur during parsing. Allocations happen only when constructing the final `AgentStep` variant.

`Malformed` is a first-class variant rather than an `Err` or a panic. This lets the runner inject a corrective observation and continue the loop, giving the model a chance to recover:

```rust
AgentStep::Malformed(_) => CORRECTIVE_OBSERVATION.to_owned(),
```

Five unit tests cover the parser's contract directly, including the priority edge case.

---

### ReAct runner — `agent/runner.rs`

`ReActRunner` owns a `Provider`, a `ToolRegistry`, and the system prompt string. The loop runs up to `MAX_STEPS = 10` iterations:

```rust
pub async fn run(&self, question: &str) -> String {
    let mut history = self.initial_history(question);

    for step in 1..=MAX_STEPS {
        let response = self.llm.complete(&history, STOP_SEQUENCES).await?;
        history.push_str(&response);   // always append before parsing

        match AgentStep::parse(&response) {
            AgentStep::FinalAnswer(answer) => break,
            AgentStep::Action { tool, input } => {
                let obs = self.tools.invoke(&tool, &input).await;
                history.push_str(&format!("\nObservation: {obs}\n"));
            }
            AgentStep::Thought(_) => continue,  // no observation needed
            AgentStep::Malformed(_) => {
                history.push_str(&format!("\nObservation: {CORRECTIVE_OBSERVATION}\n"));
            }
        }
    }
}
```

The stop sequence `"Observation:"` is passed to the LLM on every call. This causes the model to halt generation the moment it would write the observation itself — ensuring the runner always supplies it, preventing the model from fabricating results.

History is a single `String` that grows by appending each response and observation. This is intentional: the full conversation context is passed to the LLM on every call because language models are stateless — they have no memory between requests.

`Thought` steps call `continue` with no observation appended. A pure reasoning step doesn't need a tool result fed back — the model's own thought is already in the history, so it can build on it in the next step.

`run()` returns `String` (the final answer) rather than writing to stdout directly. This keeps the runner testable and decoupled from I/O — `main.rs` is responsible for printing.

---

### System prompt — `prompt.rs`

The system prompt is built at startup from the live tool registry:

```rust
pub fn build_system_prompt(registry: &ToolRegistry) -> String {
    format!(r#"You are an expert SQLite data analyst.

## Tools
{tools}
..."#,
        tools = registry.prompt_block(),
    )
}
```

No `system_prompt.txt` file. No hardcoded schema. The model is told the tool names and descriptions, and it is required to call `describe_schema` before any query. This means the prompt is always consistent with the registered tools, and the schema information the model sees is always consistent with the live database.

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

**1.** Create `src/tool/mytool.rs`:

```rust
use crate::{error::AppError};
use super::Tool;

pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }
    fn description(&self) -> &'static str { "Does X. Input: Y." }

    async fn invoke(&self, input: &str) -> Result<String, AppError> {
        Ok(format!("result for: {input}"))
    }
}
```

**2.** Add a variant to `ToolEnum` in `src/tool/mod.rs` and add the three match arms (`name`, `description`, `invoke`).

**3.** Register it in `main.rs`:

```rust
registry.register(ToolEnum::MyTool(MyTool));
```

The tool name and description are automatically picked up by `prompt_block()` and injected into the system prompt. No other changes needed.

---

## Adding a new LLM provider

**1.** Add a struct implementing `LlmProvider` in `src/llm.rs`.

**2.** Add a variant to both `Provider` and `ProviderKind`.

**3.** Add the match arm in `LlmConfig::from_env()` to read its env vars, and in `LlmConfig::into_provider()` to construct it.

---

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `tokio` | 1 | Async runtime (`full` feature for timer, I/O, sync) |
| `reqwest` | 0.12 | HTTP client for LLM API calls (`json` feature for serde integration) |
| `rusqlite` | 0.33 | SQLite driver (`bundled` feature — no system lib required) |
| `serde_json` | 1 | JSON serialisation for API request/response bodies |
| `thiserror` | 2 | Derive macros for ergonomic error type definitions |
| `dotenvy` | 0.15 | `.env` file loading at startup |
| `chrono` | 0.4 | Current date injection into the prompt (`local-offset` feature) |

---

## Limitations

- **Serialised queries** — `Arc<Mutex<Connection>>` means all tool invocations queue behind the same lock. Sufficient for a single-user CLI; replace with a connection pool (`r2d2` or `deadpool-sqlite`) for concurrent use.
- **Closed tool set** — adding a tool requires a new `ToolEnum` variant and recompilation. This is a deliberate trade-off for zero-cost enum dispatch over runtime plugin flexibility.
- **Stateless sessions** — each `run()` call starts with a fresh history string. Multi-turn conversation is not supported.
- **Row cap** — results are truncated at 15 rows to stay within LLM context limits. Adjust `MAX_ROWS` in `db.rs` to taste.
- **SELECT guard is textual** — the `sanitize()` check looks for the string `"SELECT"` in the SQL. A sufficiently adversarial prompt could construct a non-SELECT statement containing that word. For higher-security deployments, open the connection in read-only WAL mode via `rusqlite::OpenFlags`.
