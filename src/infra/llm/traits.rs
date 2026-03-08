use async_trait::async_trait;
use thiserror::Error;
use crate::api::chat::ChatMessage;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("API request failed: {0}")]
    ApiError(String),
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Internal service error: {0}")]
    Internal(String),
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate_response(
        &self,
        system_instruction: &str,
        messages: &[ChatMessage],
        model: &str,
    ) -> Result<String, LlmError>;
}
