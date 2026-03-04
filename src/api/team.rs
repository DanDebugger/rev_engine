use axum::{
    extract::{Extension, Json, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use sqlx::PgPool;
use uuid::Uuid;
use serde_json::json;

use crate::models::team_member::{TeamMember, UpdateStatusPayload};
use crate::security::jwt;

async fn verify_admin_email(headers: &HeaderMap) -> Result<(), Response> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Missing authorization header" }))).into_response()
        })?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Invalid token format" }))).into_response()
        })?;

    let claims = jwt::verify_access(token).await.map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token", "details": e.to_string() })),
        )
            .into_response()
    })?;

    if claims.email != "dangreyconcepcion312@gmail.com" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Admin access required" })),
        )
            .into_response());
    }

    Ok(())
}

async fn verify_team_read_access(headers: &HeaderMap, pool: &PgPool) -> Result<(), Response> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Missing authorization header" }))).into_response()
        })?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Invalid token format" }))).into_response()
        })?;

    let claims = jwt::verify_access(token).await.map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token", "details": e.to_string() })),
        )
            .into_response()
    })?;

    if claims.email == "dangreyconcepcion312@gmail.com" {
        return Ok(());
    }

    let is_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM team_members WHERE email = $1 AND status = 'active')"
    )
    .bind(&claims.email)
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !is_active {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Approval required to view team data." })),
        )
            .into_response());
    }

    Ok(())
}

pub async fn list_team_members(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    verify_team_read_access(&headers, &pool).await?;

    let members = sqlx::query_as::<_, TeamMember>(
        "SELECT id, profile_id, email, name, job_title, status, created_at, updated_at FROM team_members ORDER BY created_at DESC",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error fetching team members: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Json(members).into_response())
}

pub async fn update_team_member_status(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateStatusPayload>,
) -> Result<Response, Response> {
    verify_admin_email(&headers).await?;

    if payload.status != "active" && payload.status != "inactive" && payload.status != "applicant" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid status" })),
        )
            .into_response());
    }

    let result = sqlx::query(
        "UPDATE team_members SET status = $1, updated_at = NOW() WHERE id = $2"
    )
    .bind(&payload.status)
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|e| {
        tracing::error!("DB update error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Team member not found" })),
        )
            .into_response());
    }

    Ok(Json(json!({ "success": true })).into_response())
}

pub async fn remove_team_member(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Response, Response> {
    verify_admin_email(&headers).await?;

    let result = sqlx::query(
        "DELETE FROM team_members WHERE id = $1"
    )
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|e| {
        tracing::error!("DB delete error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Team member not found" })),
        )
            .into_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn get_my_team_status(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Missing authorization header" }))).into_response()
        })?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Invalid token format" }))).into_response()
        })?;

    let claims = jwt::verify_access(token).await.map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token", "details": e.to_string() })),
        )
            .into_response()
    })?;

    if claims.email == "dangreyconcepcion312@gmail.com" {
        return Ok(Json(json!({ "status": "active", "is_admin": true })).into_response());
    }

    let status_result = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, job_title FROM team_members WHERE email = $1"
    )
    .bind(&claims.email)
    .fetch_optional(&pool)
    .await;

    let (status, job_title) = match status_result {
        Ok(Some((s, j))) => (Some(s), j),
        Ok(None) => (None, None),
        Err(e) => {
            tracing::error!("DB error fetching team status: {:?}", e);
            (None, None) // Fallback to safely returning no status on DB error
        }
    };

    Ok(Json(json!({
        "status": status,
        "job_title": job_title,
        "is_admin": false
    })).into_response())
}

