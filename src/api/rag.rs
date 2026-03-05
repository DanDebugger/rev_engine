use axum::{
    extract::{Extension, Json, Multipart},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use crate::infra::ingestion::{ingest_document, DocumentSource};

#[derive(Deserialize)]
pub struct TextIngestRequest {
    pub title: String,
    pub text: String,
}

#[derive(Deserialize)]
pub struct UrlIngestRequest {
    pub title: String,
    pub url: String,
}

pub async fn ingest_text(
    Extension(pool): Extension<PgPool>,
    Json(payload): Json<TextIngestRequest>,
) -> Result<Response, Response> {
    let source = DocumentSource::Text(payload.text);
    match ingest_document(&pool, &payload.title, source, None, None).await {
        Ok(doc_id) => Ok(Json(json!({ "status": "success", "document_id": doc_id })).into_response()),
        Err(e) => {
            tracing::error!("Text ingestion failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response())
        }
    }
}

pub async fn ingest_url(
    Extension(pool): Extension<PgPool>,
    Json(payload): Json<UrlIngestRequest>,
) -> Result<Response, Response> {
    let source = DocumentSource::Url(payload.url.clone());
    match ingest_document(&pool, &payload.title, source, Some(payload.url), None).await {
        Ok(doc_id) => Ok(Json(json!({ "status": "success", "document_id": doc_id })).into_response()),
        Err(e) => {
            tracing::error!("URL ingestion failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response())
        }
    }
}

pub async fn ingest_pdf(
    Extension(pool): Extension<PgPool>,
    mut multipart: Multipart,
) -> Result<Response, Response> {
    let mut title = String::new();
    let mut pdf_bytes = Vec::new();

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();
        if name == "title" {
            title = field.text().await.unwrap_or_default();
        } else if name == "file" {
            pdf_bytes = field.bytes().await.unwrap_or_default().to_vec();
        }
    }

    if title.is_empty() || pdf_bytes.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "Missing title or file" }))).into_response());
    }

    let source = DocumentSource::Pdf(pdf_bytes);
    match ingest_document(&pool, &title, source, None, None).await {
        Ok(doc_id) => Ok(Json(json!({ "status": "success", "document_id": doc_id })).into_response()),
        Err(e) => {
            tracing::error!("PDF ingestion failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response())
        }
    }
}
