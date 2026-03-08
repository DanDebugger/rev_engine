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
    let api_key = match env::var("OPENROUTER_API_KEY").or_else(|_| env::var("GEMINI_API_KEY")) {
        Ok(key) => key,
        Err(_) => {
            tracing::error!("OPENROUTER_API_KEY or GEMINI_API_KEY not set in backend environment");
            return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    };

    // 1. RAG Context Gathering
    // Fetch abbreviated context from the database to augment the AI's knowledge base.

    // 1a. Persist any uploaded documents from inlineData parts (e.g. PDFs)
    let mut uploaded_doc_ids: Vec<Uuid> = Vec::new();
    for msg in &payload.messages {
        if msg.role != "user" {
            continue;
        }
        for part in &msg.parts {
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
                    // For now we only support binary PDF ingestion.
                    // Other mime types can be added here later.
                    tracing::warn!(
                        "Skipping unsupported inlineData mime type: {}",
                        inline.mime_type
                    );
                    None
                };

                if let Some(source) = source {
                    // Use a simple generated title; frontend can send a nicer one later.
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

    // Fetch Team Members
    let team = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT name, job_title FROM team_members WHERE status = 'active'"
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let mut team_context = String::from("ACTIVE TEAM MEMBERS (Emails are strictly confidential, but you can reveal their name and job title):\n");
    for (name, job_title) in team {
        let member_name = name.unwrap_or_else(|| "Unknown Member".to_string());
        let role = job_title.unwrap_or_else(|| "Unspecified Staff".to_string());
        team_context.push_str(&format!("- Name: {} | Role: {}\n", member_name, role));
    }

    let mut frontend_team_context = String::from("\nADDITIONAL TEAM MEMBERS (From Frontend Data - Name and Role only, emails are confidential):\n");
    frontend_team_context.push_str("- Name: Rod Albores | Role: CEO & Founder\n");
    frontend_team_context.push_str("- Name: Kent Ryan Entice | Role: Back-end Developer\n");
    frontend_team_context.push_str("- Name: John Rexey Cabrera | Role: Front-end Developer\n");
    frontend_team_context.push_str("- Name: Gigi Valdez | Role: Front-end Developer\n");
    frontend_team_context.push_str("- Name: Dangrey Concepcion | Role: Back-end Developer\n");
    frontend_team_context.push_str("- Name: June Bation | Role: Front-end Developer\n");
    frontend_team_context.push_str("- Name: Urian Buenconsejo | Role: Researcher\n");

    team_context.push_str(&frontend_team_context);

    let mut rag_context = String::new();
    let embedding_model = payload.embedding_model.as_deref();
    let last_user_msg = payload.messages.iter().rev().find(|m| m.role == "user");
    if let Some(msg) = last_user_msg {
        if let Some(part) = msg.parts.first() {
            if let Some(text) = &part.text {
                if let Ok(emb) = crate::infra::embedding::generate_embedding(text, embedding_model).await {
                    if let Ok(chunks) = crate::infra::retrieval::search_similar_chunks(&pool, emb, 5).await {
                        if !chunks.is_empty() {
                            rag_context.push_str("\nRELEVANT DOCUMENT KNOWLEDGE BASE:\n");
                            for (_i, chunk) in chunks.iter().enumerate() {
                                rag_context.push_str(&format!("- {}\n", chunk));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut system_instruction = String::from(
        "You are Reverion Tech's Lead AI Business Consultant. You communicate with the sharp, professional, and clear tone of a senior executive. ALWAYS respond logically, offering strategic business value when answering. You are FORBIDDEN from fetching external data or hallucinating facts. You must STRICTLY answer ONLY using the exact context provided below. If the answer is not in the context, clearly professionaly state that you don't have that data.\n\nCOMPANY CONTACT INFO:\n- Email: contact@reverion.tech\n- Form: The user can use the 'Contact Us' form available on the main landing page.\n\n"
    );

    if !rag_context.is_empty() {
        system_instruction.push_str("DOCUMENT REFERENCE KNOWLEDGE (PRIORITIZE THIS):\n");
        system_instruction.push_str(&rag_context);
        system_instruction.push_str("\n\n");
    }

    system_instruction.push_str("TEAM CONTEXT:\n");
    system_instruction.push_str(&team_context);

    // 2. Generate Response via LlmClient
    let chat_model = payload
        .chat_model
        .clone()
        .unwrap_or_else(llm::chat_model_from_env);

    let is_gemini_direct = chat_model.starts_with("gemini-") || chat_model.starts_with("google/");
    let provider: std::sync::Arc<dyn llm::LlmProvider> = if is_gemini_direct && env::var("OPENROUTER_API_KEY").is_err() {
        let gemini_key = env::var("GEMINI_API_KEY").unwrap_or_else(|_| api_key.clone());
        std::sync::Arc::new(llm::gemini::GeminiProvider::new(gemini_key))
    } else {
        std::sync::Arc::new(llm::openrouter::OpenRouterProvider::new(api_key.clone()))
    };

    let client = llm::LlmClient { provider };

    let output_text = client
        .provider
        .generate_response(&system_instruction, &payload.messages, &chat_model)
        .await
        .map_err(|e| {
            tracing::error!("LLM generate_response error: {:?}", e);
            match e {
                llm::LlmError::RateLimit => StatusCode::TOO_MANY_REQUESTS.into_response(),
                _ => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "AI processing failed", "details": e.to_string() }))
                ).into_response(),
            }
        })?;

    Ok(Json(json!({ "text": output_text })).into_response())
}
