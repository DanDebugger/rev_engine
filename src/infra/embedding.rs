use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbeddingResponse {
    pub embedding: EmbeddingData,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbeddingData {
    pub values: Vec<f32>,
}

pub async fn generate_embedding(text: &str) -> Result<Vec<f32>, String> {
    let api_key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY not set")?;
    
    let client = Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:embedContent?key={}",
        api_key
    );

    let payload = json!({
        "model": "models/text-embedding-004",
        "content": {
            "parts": [{ "text": text }]
        }
    });

    let res = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to call Gemini: {}", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Gemini Embedding API Error: {}", err_body));
    }

    let data: EmbeddingResponse = res.json().await.map_err(|e| format!("Failed to parse response: {}", e))?;
    Ok(data.embedding.values)
}
