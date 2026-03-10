use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::infra::llm;

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbeddingData {
    pub embedding: Vec<f32>,
}

#[derive(Deserialize, Debug)]
struct GeminiEmbedResponse {
    embedding: Option<GeminiEmbedding>,
}

#[derive(Deserialize, Debug)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

// Once OpenRouter returns 402, we set this flag and stop calling it for this process.
static OPENROUTER_DISABLED: AtomicBool = AtomicBool::new(false);

async fn generate_embedding_gemini(text: &str) -> Result<Vec<f32>, String> {
    let api_key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY not set")?;
    let client = Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent?key={}",
        api_key
    );
    let payload = json!({
        "content": { "parts": [{ "text": text }] },
        "outputDimensionality": 768
    });
    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to call Gemini: {}", e))?;
    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Gemini Embedding API Error: {}", err_body));
    }
    let data: GeminiEmbedResponse =
        res.json().await.map_err(|e| format!("Failed to parse Gemini response: {}", e))?;
    data.embedding
        .map(|e| e.values)
        .ok_or_else(|| "No embedding in Gemini response".to_string())
}

pub async fn generate_embedding(text: &str, model_override: Option<&str>) -> Result<Vec<f32>, String> {
    let openrouter_key = env::var("OPENROUTER_API_KEY").ok();
    let gemini_key = env::var("GEMINI_API_KEY").ok();

    let embedding_provider_env = env::var("EMBEDDING_PROVIDER")
        .ok()
        .map(|v| v.to_lowercase());

    let prefer_gemini = matches!(embedding_provider_env.as_deref(), Some("gemini"))
        || OPENROUTER_DISABLED.load(Ordering::Relaxed)
        || (gemini_key.is_some() && openrouter_key.is_none());

    if prefer_gemini {
        if gemini_key.is_some() {
            return generate_embedding_gemini(text).await;
        }
    }

    if let Some(api_key) = openrouter_key.clone().filter(|_| !OPENROUTER_DISABLED.load(Ordering::Relaxed)) {
        let model = model_override
            .map(String::from)
            .unwrap_or_else(llm::embedding_model_from_env);

        let client = Client::new();
        let res = client
            .post("https://openrouter.ai/api/v1/embeddings")
            .header("Authorization", format!("Bearer {}", api_key))
            .header(
                "HTTP-Referer",
                env::var("CLIENT_URL").unwrap_or_else(|_| "http://localhost:5173".to_string()),
            )
            .header("Content-Type", "application/json")
            .json(&json!({ "model": model, "input": text }))
            .send()
            .await
            .map_err(|e| format!("Failed to call OpenRouter: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();

            if status.as_u16() == 402 {
                tracing::warn!(
                    "OpenRouter returned 402 (insufficient credits), permanently falling back to Gemini embeddings"
                );
                OPENROUTER_DISABLED.store(true, Ordering::Relaxed);

                if gemini_key.is_some() {
                    return generate_embedding_gemini(text).await;
                }
            }

            return Err(format!("OpenRouter Embedding API Error: {}", err_body));
        }

        let data: EmbeddingResponse =
            res.json().await.map_err(|e| format!("Failed to parse OpenRouter response: {}", e))?;

        return data
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| "No embedding returned from OpenRouter".to_string());
    }

    if gemini_key.is_some() {
        return generate_embedding_gemini(text).await;
    }

    Err("No embedding provider available: set GEMINI_API_KEY or OPENROUTER_API_KEY".to_string())
}
