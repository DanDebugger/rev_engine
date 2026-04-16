use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::env;
use uuid::Uuid;

use crate::infra::ingestion::{ingest_document, DocumentSource};
use crate::infra::llm;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    /// Optional conversation id for persistence.
    pub conversation_id: Option<Uuid>,
    /// Optional chat model id (e.g. openrouter/free). Overrides env CHAT_MODEL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_model: Option<String>,
    /// Optional embedding model id for RAG and uploads. Overrides env EMBEDDING_MODEL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub parts: Vec<ChatPart>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatPart {
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "inlineData")]
    pub inline_data: Option<InlineData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

pub async fn handle_chat(
    Extension(pool): Extension<PgPool>,
    Json(payload): Json<ChatRequest>,
) -> Result<Response, Response> {
    let openrouter_key = env::var("OPENROUTER_API_KEY").ok();
    let gemini_key = env::var("GEMINI_API_KEY").ok();
    let openai_key = env::var("OPENAI_API_KEY").ok();

    if openrouter_key.is_none() && gemini_key.is_none() && openai_key.is_none() {
        tracing::error!("No LLM API keys set in backend environment");
        return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    // 0. Persistence Setup: Get or Create Conversation
    let conversation_id = if let Some(id) = payload.conversation_id {
        id
    } else {
        // Create a new conversation record
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO conversations (id, created_at, updated_at) VALUES ($1, NOW(), NOW()) RETURNING id"
        )
        .bind(Uuid::new_v4())
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create conversation: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?
    };

    // 1. RAG Context Gathering & Upload Persistence
    let mut uploaded_doc_ids: Vec<Uuid> = Vec::new();
    let mut last_user_text = String::new();

    for msg in &payload.messages {
        if msg.role == "user" {
            for part in &msg.parts {
                if let Some(text) = &part.text {
                    last_user_text = text.clone();
                }
                if let Some(inline) = &part.inline_data {
                    let bytes = match base64::engine::general_purpose::STANDARD.decode(&inline.data) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("Failed to decode inlineData base64: {:?}", e);
                            continue;
                        }
                    };

                    let source = if inline.mime_type == "application/pdf" {
                        Some(DocumentSource::Pdf(bytes))
                    } else {
                        tracing::warn!("Skipping unsupported inlineData mime type: {}", inline.mime_type);
                        None
                    };

                    if let Some(source) = source {
                        let title = format!("Chat upload {}", chrono::Utc::now());
                        let emb_override = payload.embedding_model.as_deref();
                        match ingest_document(&pool, &title, source, None, emb_override).await {
                            Ok(doc_id) => {
                                tracing::info!("Ingested inline document with id {}", doc_id);
                                uploaded_doc_ids.push(doc_id);
                            }
                            Err(e) => {
                                tracing::error!("Failed to ingest inline document: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // Save the user's message to history if it's not empty
    if !last_user_text.is_empty() {
        let _ = sqlx::query(
            "INSERT INTO chat_history (id, conversation_id, role, content, created_at) VALUES ($1, $2, $3, $4, NOW())"
        )
        .bind(Uuid::new_v4())
        .bind(conversation_id)
        .bind("user")
        .bind(&last_user_text)
        .execute(&pool)
        .await;
    }

    // Fetch Team Members
    let team = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT name, job_title FROM team_members WHERE status = 'active'"
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let mut team_context = String::from("ACTIVE TEAM MEMBERS:\n");
    for (name, job_title) in team {
        team_context.push_str(&format!("- Name: {} | Role: {}\n", name.unwrap_or_default(), job_title.unwrap_or_default()));
    }

    let mut rag_context = String::new();
    let embedding_model = payload.embedding_model.as_deref();
    if !last_user_text.is_empty() {
        if let Ok(emb) = crate::infra::embedding::generate_embedding(&last_user_text, embedding_model).await {
            if let Ok(chunks) = crate::infra::retrieval::search_similar_chunks(&pool, emb, 5).await {
                if !chunks.is_empty() {
                    rag_context.push_str("\nRELEVANT DOCUMENT KNOWLEDGE BASE:\n");
                    for chunk in chunks {
                        rag_context.push_str(&format!("- {}\n", chunk));
                    }
                }
            }
        }
    }

    let system_instruction = format!(
        "You are Reverion Tech's Lead AI Business Consultant. Respond professionally. Use the provided context strictly.\n\nTEAM CONTEXT:\n{}\n\n{}",
        team_context,
        if rag_context.is_empty() { "".to_string() } else { format!("DOCUMENT CONTEXT:\n{}", rag_context) }
    );

    // 2. Generate Response with Automatic Fallback
    let chat_model = payload.chat_model.clone().unwrap_or_else(llm::chat_model_from_env);
    
    // Determine the sequence of providers to try to maximize availability
    let mut attempts: Vec<(std::sync::Arc<dyn llm::LlmProvider>, String, &'static str)> = Vec::new();
    
    if chat_model.starts_with("openai/") || chat_model.starts_with("gpt-") {
        // OpenAI models: OpenAI Direct -> OpenRouter -> Gemini fallback
        if let Some(key) = openai_key.clone() {
            attempts.push((std::sync::Arc::new(llm::openai::OpenAiProvider::new(key)), chat_model.clone(), "OpenAI Direct"));
        }
        if let Some(key) = openrouter_key.clone() {
            attempts.push((std::sync::Arc::new(llm::openrouter::OpenRouterProvider::new(key)), chat_model.clone(), "OpenRouter (OpenAI)"));
        }
        if let Some(key) = gemini_key.clone() {
            attempts.push((std::sync::Arc::new(llm::gemini::GeminiProvider::new(key)), "gemini-2.5-flash".to_string(), "Gemini Fallback"));
        }
    } else if chat_model.starts_with("gemini-") || chat_model.starts_with("google/") {
        // Google models: Gemini Direct -> OpenRouter -> OpenAI fallback
        if let Some(key) = gemini_key.clone() {
            attempts.push((std::sync::Arc::new(llm::gemini::GeminiProvider::new(key)), chat_model.clone(), "Gemini Direct"));
        }
        if let Some(key) = openrouter_key.clone() {
            attempts.push((std::sync::Arc::new(llm::openrouter::OpenRouterProvider::new(key)), chat_model.clone(), "OpenRouter (Gemini)"));
        }
        if let Some(key) = openai_key.clone() {
            attempts.push((std::sync::Arc::new(llm::openai::OpenAiProvider::new(key)), "gpt-4o-mini".to_string(), "OpenAI Fallback"));
        }
    } else {
        // Other models (Anthropic, etc.): OpenRouter -> OpenAI -> Gemini fallback
        if let Some(key) = openrouter_key.clone() {
            attempts.push((std::sync::Arc::new(llm::openrouter::OpenRouterProvider::new(key)), chat_model.clone(), "OpenRouter"));
        }
        if let Some(key) = openai_key.clone() {
            attempts.push((std::sync::Arc::new(llm::openai::OpenAiProvider::new(key)), "gpt-4o-mini".to_string(), "OpenAI Fallback"));
        }
        if let Some(key) = gemini_key.clone() {
            attempts.push((std::sync::Arc::new(llm::gemini::GeminiProvider::new(key)), "gemini-2.5-flash".to_string(), "Gemini Fallback"));
        }
    }

    if attempts.is_empty() {
        tracing::error!("No LLM providers or keys available for model: {}", chat_model);
        return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let mut output_text = None;
    let mut last_llm_error = None;

    for (provider, model_id, label) in attempts {
        tracing::info!("Attempting to generate response with {} using model {}", label, model_id);
        match provider.generate_response(&system_instruction, &payload.messages, &model_id).await {
            Ok(text) => {
                output_text = Some(text);
                break;
            }
            Err(e) => {
                tracing::warn!("{} failed: {:?}. Checking if fallback is possible...", label, e);
                last_llm_error = Some(e);
                
                // Only fallback on availability errors (Rate Limits or Billing)
                match last_llm_error.as_ref().unwrap() {
                    llm::LlmError::RateLimit | llm::LlmError::PaymentRequired => {
                        continue; 
                    }
                    _ => break, // Fatal error (e.g. malformed JSON, internal error), stop trying
                }
            }
        }
    }

    let output_text = match output_text {
        Some(t) => t,
        None => {
            let err = last_llm_error.unwrap_or(llm::LlmError::Internal("Unknown LLM failure".to_string()));
            tracing::error!("All LLM providers failed. Final error: {:?}", err);
            return Err((StatusCode::BAD_GATEWAY, Json(json!({ 
                "error": "The AI consultant is currently experiencing high demand. Please try again in 30 seconds.",
                "details": format!("{:?}", err)
            }))).into_response());
        }
    };

    // 3. Persist AI Response
    let _ = sqlx::query(
        "INSERT INTO chat_history (id, conversation_id, role, content, created_at) VALUES ($1, $2, $3, $4, NOW())"
    )
    .bind(Uuid::new_v4())
    .bind(conversation_id)
    .bind("assistant")
    .bind(&output_text)
    .execute(&pool)
    .await;

    // Update conversation timestamp
    let _ = sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
        .bind(conversation_id)
        .execute(&pool)
        .await;

    Ok(Json(json!({ "text": output_text, "conversation_id": conversation_id })).into_response())
}

pub async fn list_conversations(
    Extension(pool): Extension<PgPool>,
) -> Result<Response, StatusCode> {
    let rows = sqlx::query_as::<_, crate::models::chat::Conversation>(
        "SELECT * FROM conversations ORDER BY updated_at DESC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows).into_response())
}

pub async fn get_conversation_history(
    Extension(pool): Extension<PgPool>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Response, StatusCode> {
    let rows = sqlx::query_as::<_, crate::models::chat::ChatHistoryItem>(
        "SELECT * FROM chat_history WHERE conversation_id = $1 ORDER BY created_at ASC"
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows).into_response())
}
