use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use crate::api::chat::ChatMessage;
use super::traits::{LlmError, LlmProvider};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
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

        // Strip "openai/" prefix if present (e.g. "openai/gpt-4o-mini" -> "gpt-4o-mini")
        let clean_model = model.strip_prefix("openai/").unwrap_or(model);

        let payload = json!({
            "model": clean_model,
            "messages": api_messages,
            "temperature": 0.3
        });

        let res = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            tracing::error!("OpenAI API error (Status: {}): {}", status, err_body);

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(LlmError::RateLimit);
            }
            if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                return Err(LlmError::PaymentRequired);
            }
            return Err(LlmError::ApiError(format!("OpenAI returned {}: {}", status, err_body)));
        }

        let openai_data: serde_json::Value = res.json().await.map_err(|e| {
            tracing::error!("Failed to parse OpenAI JSON: {:?}", e);
            LlmError::Internal(format!("Failed to parse OpenAI response: {}", e))
        })?;

        let output_text = openai_data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("NO RESPONDING DATALINK.")
            .to_string();

        Ok(output_text)
    }
}
