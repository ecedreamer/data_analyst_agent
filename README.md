# Rust SQLite Data Analyst Agent 

A **ReAct (Reasoning and Acting) Agent** built in Rust. This project enables Large Language Models (LLMs) to interact securely with a local SQLite database to answer data-driven questions using natural language.

**Note:** This project is for learning/educational purpose only. 

## The ReAct Architecture

The agent doesn't just "guess" an answer. It follows a structured loop that allows it to think, act, and observe results before providing a final response.

1.  **Thought**: The LLM analyzes the request and determines the necessary SQL logic.
2.  **Action**: The model generates a specific SQL query inside a markdown block.
3.  **Observation**: The Rust backend executes the query against your 1,000,000+ row database.
4.  **Final Answer**: The model synthesizes the raw data into a human-readable summary.



## Features

* **Multi-Model Support**: Native integration for **Gemini 2.0/2.5 Flash** and **OpenAI GPT-4o**.
* **Robust JSON Parsing**: Specialized logic to handle Gemini's multi-part responses and safety metadata.
* **Safe SQL Execution**: Enforcement of read-only `SELECT` queries to prevent accidental data modification.
* **Dynamic Context**: Automatically injects the current system date using `chrono` so the LLM can handle relative time queries (e.g., "last 30 days").
* **Zero-Panic Error Handling**: Defensive coding style that catches API errors and malformed SQL without crashing the Rust runtime.

## Prerequisites

- **Rust**: Latest stable version.
- **SQLite**: A database file at `./databases/database1.sqlite3`.
- **API Keys**: Valid keys for Google AI Studio or OpenAI.

## Configuration

Create a `.env` file in your project root:

```env
MODEL_PROVIDER=gemini
GEMINI_API_KEY=your_key_here
GEMINI_MODEL=gemini-1.5-flash

# For OpenAI usage:
# MODEL_PROVIDER=openai
# OPENAI_API_KEY=your_key_here
# OPENAI_MODEL=gpt-4o
