use std::env;

use reqwest::Client;
use serde_json::{Value, json};

use crate::error::AppError;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Uniform interface every LLM backend must satisfy.
///
/// Native async fn in traits is stable since Rust 1.75 (RPITIT). Each impl
/// gets its own concrete future type — no `Box::pin` overhead, no
/// `async_trait` macro needed.
///
/// The trade-off: the trait is **not** object-safe, so we cannot use
/// `Box<dyn LlmProvider>`. Instead, `Provider` (below) is a hand-rolled
/// closed-set enum that dispatches to the concrete types. Adding a new
/// provider = one new variant + one new impl block; the agent and config
/// layers are untouched.
pub trait LlmProvider {
    async fn complete(&self, prompt: &str, stop_sequences: &[&str]) -> Result<String, AppError>;
}

// ── Enum dispatch ─────────────────────────────────────────────────────────────

/// Closed-set of supported providers used as the concrete type throughout
/// the crate. Delegates to the appropriate inner impl.
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

// ── Configuration ─────────────────────────────────────────────────────────────

/// Provider-agnostic settings resolved from environment variables at startup.
pub struct LlmConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
}

pub enum ProviderKind {
    OpenAI,
    Gemini,
}

impl LlmConfig {
    pub fn from_env() -> Result<Self, AppError> {
        match env::var("MODEL_PROVIDER")
            .unwrap_or_else(|_| "openai".into())
            .to_lowercase()
            .as_str()
        {
            "gemini" => Ok(Self {
                provider: ProviderKind::Gemini,
                model: env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-1.5-flash".into()),
                api_key: env::var("GEMINI_API_KEY")
                    .map_err(|_| AppError::Config("GEMINI_API_KEY not set".into()))?,
            }),
            _ => Ok(Self {
                provider: ProviderKind::OpenAI,
                model: env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into()),
                api_key: env::var("OPENAI_API_KEY")
                    .map_err(|_| AppError::Config("OPENAI_API_KEY not set".into()))?,
            }),
        }
    }

    /// Consume config and construct the concrete `Provider`.
    pub fn into_provider(self) -> Provider {
        let client = Client::new();
        match self.provider {
            ProviderKind::OpenAI => Provider::OpenAI(OpenAiProvider {
                model: self.model,
                api_key: self.api_key,
                client,
            }),
            ProviderKind::Gemini => Provider::Gemini(GeminiProvider {
                model: self.model,
                api_key: self.api_key,
                client,
            }),
        }
    }
}

// ── OpenAI ────────────────────────────────────────────────────────────────────

pub struct OpenAiProvider {
    model: String,
    api_key: String,
    client: Client,
}

impl LlmProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str, stop_sequences: &[&str]) -> Result<String, AppError> {
        let body = json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "stop": stop_sequences,
            "temperature": 0.0
        });

        let json: Value = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        Ok(json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned())
    }
}

// ── Gemini ────────────────────────────────────────────────────────────────────

pub struct GeminiProvider {
    model: String,
    api_key: String,
    client: Client,
}

impl LlmProvider for GeminiProvider {
    async fn complete(&self, prompt: &str, stop_sequences: &[&str]) -> Result<String, AppError> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model.trim(),
            self.api_key,
        );

        let body = json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "safetySettings": safety_settings(),
            "generationConfig": {
                "stopSequences": stop_sequences,
                "temperature": 0.0,
                "maxOutputTokens": 2048
            }
        });

        let json: Value = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if let Some(msg) = json["error"]["message"].as_str() {
            return Err(AppError::Llm(format!("API error: {msg}")));
        }

        let candidate = &json["candidates"][0];

        if candidate["finishReason"].as_str() == Some("SAFETY") {
            return Err(AppError::Llm(
                "Gemini blocked the response due to safety filters.".into(),
            ));
        }

        Ok(candidate["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_owned())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn safety_settings() -> Value {
    json!([
        {"category": "HARM_CATEGORY_HARASSMENT",        "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_HATE_SPEECH",       "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE"},
    ])
}
