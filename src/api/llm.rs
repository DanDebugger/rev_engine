use axum::Json;
use serde::Serialize;

use crate::infra::llm;

#[derive(Serialize)]
pub struct LlmModelsResponse {
    pub chat_models: Vec<llm::ModelOption>,
    pub embedding_models: Vec<llm::ModelOption>,
}

pub async fn list_models() -> Json<LlmModelsResponse> {
    Json(LlmModelsResponse {
        chat_models: llm::available_chat_models(),
        embedding_models: llm::available_embedding_models(),
    })
}
