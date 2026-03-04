use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::env;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
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

#[derive(Serialize, Deserialize, Debug)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiContent {
    parts: Option<Vec<ChatPart>>,
    role: Option<String>,
}

pub async fn handle_chat(
    Extension(pool): Extension<PgPool>,
    Json(payload): Json<ChatRequest>,
) -> Result<Response, Response> {
    let api_key = match env::var("GEMINI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            tracing::error!("GEMINI_API_KEY not set in backend environment");
            return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    };

    // 1. RAG Context Gathering
    // Fetch abbreviated context from the database to augment the AI's knowledge base.
    
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

    // Fetch Active Projects
    let projects = sqlx::query_as::<_, (String, String, i32)>(
        "SELECT name, status, progress FROM projects"
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let mut frontend_team_context = String::from("\nADDITIONAL TEAM MEMBERS (From Frontend Data - Name and Role only, emails are confidential):\n");
    frontend_team_context.push_str("- Name: Rod Albores | Role: CEO & Founder\n");
    frontend_team_context.push_str("- Name: Kent Ryan Entice | Role: Back-end Developer\n");
    frontend_team_context.push_str("- Name: John Rexey Cabrera | Role: Front-end Developer\n");
    frontend_team_context.push_str("- Name: Gigi Valdez | Role: Front-end Developer\n");
    frontend_team_context.push_str("- Name: Dangrey Concepcion | Role: Back-end Developer\n");
    frontend_team_context.push_str("- Name: June Bation | Role: Front-end Developer\n");
    frontend_team_context.push_str("- Name: Urian Buenconsejo | Role: Researcher\n");

    let mut projects_context = String::from("\nPROJECTS:\n");
    for proj in projects {
        projects_context.push_str(&format!("- Project: {} | Status: {} | Progress: {}%\n", proj.0, proj.1, proj.2));
    }

    let mut rag_context = String::new();
    let last_user_msg = payload.messages.iter().rev().find(|m| m.role == "user");
    if let Some(msg) = last_user_msg {
        if let Some(part) = msg.parts.first() {
            if let Some(text) = &part.text {
                if let Ok(emb) = crate::infra::embedding::generate_embedding(text).await {
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

    let system_instruction = format!(
        "You are the SYS Core AI for Reverion Tech. You manage the system and have access to current company database state. Answer concisely in a brutalist, robotic, hyper-efficient tone.\n\nCOMPANY CONTACT INFO:\n- Email: contact@reverion.tech\n- Form: The user can use the 'Contact Us' form available on the main landing page to reach the team directly.\n\nHere is the current live database context:\n\n{}{}{}",
        team_context, projects_context, rag_context
    );

    // 2. Format Request for Gemini REST API
    // We map the incoming messages to the Gemini Content array format.
    let mut contents = Vec::new();
    for msg in &payload.messages {
        let role = if msg.role == "ai" || msg.role == "model" { "model" } else { "user" };
        contents.push(json!({
            "role": role,
            "parts": msg.parts
        }));
    }

    let gemini_payload = json!({
        "system_instruction": {
            "parts": [{ "text": system_instruction }]
        },
        "contents": contents,
        "generationConfig": {
            "temperature": 0.3
        }
    });

    let client = Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );

    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&gemini_payload)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Error calling Gemini: {:?}", e);
            StatusCode::BAD_GATEWAY.into_response()
        })?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        tracing::error!("Gemini API returned an error: {}", err_body);
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "AI processing failed", "details": err_body }))
        ).into_response());
    }

    let gemini_data: GeminiResponse = res.json().await.map_err(|e| {
        tracing::error!("Failed to parse Gemini response: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    let output_text = gemini_data
        .candidates
        .as_ref()
        .and_then(|c| c.get(0))
        .and_then(|c| c.content.as_ref())
        .and_then(|c| c.parts.as_ref())
        .and_then(|p| p.get(0))
        .and_then(|p| p.text.clone())
        .unwrap_or_else(|| "NO RESPONDING DATALINK.".to_string());

    Ok(Json(json!({ "text": output_text })).into_response())
}
