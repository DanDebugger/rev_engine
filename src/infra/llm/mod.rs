pub mod traits;
pub mod openrouter;
pub mod gemini;

pub use traits::{LlmError, LlmProvider};

use std::sync::Arc;
use std::env;

/// A unified wrapper ready for dependency injection into state/services.
#[derive(Clone)]
pub struct LlmClient {
    pub provider: Arc<dyn LlmProvider>,
}

impl LlmClient {
    pub fn new(provider: impl LlmProvider + 'static) -> Self {
        Self {
            provider: Arc::new(provider),
        }
    }
}


/// Central configuration helpers for LLM and embedding models.
pub fn chat_model_from_env() -> String {
    env::var("CHAT_MODEL").unwrap_or_else(|_| "openrouter/free".to_string())
}

pub fn embedding_model_from_env() -> String {
    env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "openai/text-embedding-3-small".to_string())
}

/// Model option for frontend dropdowns: (id, display label).
#[derive(serde::Serialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

/// Chat models available for selection (OpenRouter-style ids).
pub fn available_chat_models() -> Vec<ModelOption> {
    vec![
        ModelOption { id: "openrouter/free".to_string(), label: "Free (OpenRouter)".to_string() },
        ModelOption { id: "openai/gpt-4o-mini".to_string(), label: "GPT-4o Mini".to_string() },
        ModelOption { id: "openai/gpt-4o".to_string(), label: "GPT-4o".to_string() },
        ModelOption { id: "google/gemini-2.0-flash-001".to_string(), label: "Gemini 2.0 Flash".to_string() },
        ModelOption { id: "anthropic/claude-3.5-sonnet".to_string(), label: "Claude 3.5 Sonnet".to_string() },
    ]
}

/// Embedding models available for selection.
pub fn available_embedding_models() -> Vec<ModelOption> {
    vec![
        ModelOption { id: "openai/text-embedding-3-small".to_string(), label: "OpenAI 3 Small".to_string() },
        ModelOption { id: "openai/text-embedding-3-large".to_string(), label: "OpenAI 3 Large".to_string() },
    ]
}

