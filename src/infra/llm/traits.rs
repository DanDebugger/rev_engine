use async_trait::async_trait;
use thiserror::Error;
use crate::api::chat::ChatMessage;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("API request failed: {0}")]
    ApiError(String),
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Billing or credit limit reached (402 Payment Required)")]
    PaymentRequired,
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
