use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::env;

use crate::api::chat::ChatMessage;
use super::traits::{LlmError, LlmProvider};

pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn generate_response(
        &self,
        system_instruction: &str,
        messages: &[ChatMessage],
        model: &str,
    ) -> Result<String, LlmError> {
        let mut api_messages = Vec::new();
        
        // Add system instruction first
        api_messages.push(json!({
            "role": "system",
            "content": system_instruction
        }));

        for msg in messages {
            let role = if msg.role == "ai" || msg.role == "model" { "assistant" } else { "user" };
            
            let mut text_content = String::new();
            for part in &msg.parts {
                if let Some(t) = &part.text {
                    text_content.push_str(t);
                    text_content.push('\n');
                }
            }

            api_messages.push(json!({
                "role": role,
                "content": text_content.trim()
            }));
        }

        let payload = json!({
            "model": model,
            "messages": api_messages,
            "temperature": 0.3
        });

        let url = "https://openrouter.ai/api/v1/chat/completions";
        let referer = env::var("CLIENT_URL").unwrap_or_else(|_| "http://localhost:5173".to_string());

        let res = self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", referer)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            tracing::error!("OpenRouter API error (Status: {}): {}", status, err_body);
            
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(LlmError::RateLimit);
            }
            if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                return Err(LlmError::PaymentRequired);
            }
            return Err(LlmError::ApiError(format!("OpenRouter returned {}: {}", status, err_body)));
        }

        let openrouter_data: serde_json::Value = res.json().await.map_err(|e| {
            LlmError::Internal(format!("Failed to parse OpenRouter response: {}", e))
        })?;

        let output_text = openrouter_data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("NO RESPONDING DATALINK.")
            .to_string();

        Ok(output_text)
    }
}
