use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use crate::api::chat::ChatMessage;
use super::traits::{LlmError, LlmProvider};

pub struct GeminiProvider {
    client: Client,
    api_key: String,
}

impl GeminiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn generate_response(
        &self,
        system_instruction: &str,
        messages: &[ChatMessage],
        model: &str,
    ) -> Result<String, LlmError> {
        let mut contents = Vec::new();
        
        for msg in messages {
            let role = if msg.role == "ai" || msg.role == "assistant" { "model" } else { "user" };
            
            let mut parts = Vec::new();
            for part in &msg.parts {
                if let Some(t) = &part.text {
                    parts.push(json!({ "text": t }));
                }
            }
            contents.push(json!({
                "role": role,
                "parts": parts,
            }));
        }

        let payload = json!({
            "systemInstruction": {
                "parts": [{ "text": system_instruction }]
            },
            "contents": contents,
            "generationConfig": {
                "temperature": 0.3
            }
        });

        // OpenRouter model string might be "google/gemini-2.0-flash-001". 
        // Direct API expects just "gemini-..."
        let clean_model = model.strip_prefix("google/").unwrap_or(model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            clean_model, self.api_key
        );

        let res = self.client.post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            tracing::error!("Gemini API error (Status: {}): {}", status, err_body);
            
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(LlmError::RateLimit);
            }
            if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                return Err(LlmError::PaymentRequired);
            }
            return Err(LlmError::ApiError(format!("Gemini returned {}: {}", status, err_body)));
        }

        let gemini_data: serde_json::Value = res.json().await.map_err(|e| {
            tracing::error!("Failed to parse Gemini JSON: {:?}", e);
            LlmError::Internal(format!("Failed to parse Gemini response: {}", e))
        })?;

        // Handle possible safety blocks or empty candidates
        let output_text = gemini_data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                if let Some(reason) = gemini_data["promptFeedback"]["blockReason"].as_str() {
                    tracing::warn!("Gemini blocked prompt: {}", reason);
                    Some(format!("REVERION_SECURE_BLOCK: {}", reason))
                } else if let Some(finish_reason) = gemini_data["candidates"][0]["finishReason"].as_str() {
                    tracing::warn!("Gemini finished early: {}", finish_reason);
                    Some(format!("REVERION_FINISH_REASON: {}", finish_reason))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                tracing::error!("Gemini response missing text. Data: {:?}", gemini_data);
                "NO RESPONDING DATALINK.".to_string()
            });

        Ok(output_text)
    }
}
