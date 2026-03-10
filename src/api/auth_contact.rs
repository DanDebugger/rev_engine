use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sqlx::PgPool;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use crate::models::contact_request::{ContactRequest, CreateContactRequest};
use crate::utils::crypto::decrypt_payload;
use crate::models::contact_request::EncryptedPayload;

const SECRET_KEY: [u8; 32] = *b"super_secure_key_32_bytes_length";

pub async fn submit_contact(
    Extension(pool): Extension<PgPool>,
    Json(body): Json<EncryptedPayload>,
) -> Result<Response, Response> {

    let decrypted = decrypt_payload(&SECRET_KEY, &body.payload)
        .map_err(|_| StatusCode::BAD_REQUEST.into_response())?;

    let payload: CreateContactRequest = serde_json::from_str(&decrypted)
        .map_err(|_| StatusCode::BAD_REQUEST.into_response())?;

    if payload.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Name is required" })),
        ).into_response());
    }

    let contact: ContactRequest = sqlx::query_as::<sqlx::Postgres, ContactRequest>(
        "INSERT INTO contact_requests (name, email, company, service, budget, description, timeline)
         VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING *"
    )
    .bind(payload.name.trim())
    .bind(payload.email.trim())
    .bind(payload.company.trim())
    .bind(payload.service.trim())
    .bind(payload.budget.trim())
    .bind(payload.description.trim())
    .bind(payload.timeline.trim())
    .fetch_one(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

    Ok(Json(contact).into_response())
}

pub async fn list_contacts(
    Extension(pool): Extension<PgPool>,
) -> Result<Response, Response> {
    let rows: Vec<ContactRequest> = sqlx::query_as::<sqlx::Postgres, ContactRequest>(
        "SELECT * FROM contact_requests ORDER BY created_at DESC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to list contact requests: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Json(rows).into_response())
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusPayload {
    pub status: String,
}

pub async fn update_contact_status(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateStatusPayload>,
) -> Result<Response, Response> {
    let valid = ["new", "read", "contacted"];
    if !valid.contains(&payload.status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid status. Must be new, read, or contacted." })),
        ).into_response());
    }

    sqlx::query("UPDATE contact_requests SET status = $1 WHERE id = $2")
        .bind(&payload.status)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update contact status: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?;

    Ok(Json(json!({ "ok": true })).into_response())
}

pub async fn delete_contact(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Response, Response> {
    sqlx::query("DELETE FROM contact_requests WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete contact request: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?;

    Ok(Json(json!({ "ok": true })).into_response())
}

pub async fn clear_contacts(
    Extension(pool): Extension<PgPool>,
) -> Result<Response, Response> {
    sqlx::query("DELETE FROM contact_requests")
        .execute(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to clear contact requests: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?;

    Ok(Json(json!({ "ok": true })).into_response())
}

