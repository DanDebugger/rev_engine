use axum::{
    extract::{Extension, Json},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use sqlx::PgPool;
use uuid::Uuid;
use serde_json::json;

use crate::models::profile::{ProfileResponse, UpdateProfile};
use crate::security::jwt;

async fn extract_user_id(headers: &HeaderMap) -> Result<Uuid, Response> {
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

    // The verification process checks the signature of the JWT.
    let claims = jwt::verify_access(token).await.map_err(|e| {
        tracing::error!("JWT validation error: {:?}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid or expired token",
                "details": e.to_string()
            })),
        )
            .into_response()
    })?;

    claims.sub.parse::<Uuid>().map_err(|_| {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Invalid token subject UUID" }))).into_response()
    })
}

pub async fn get_profile(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let user_id = extract_user_id(&headers).await?;

    let row = sqlx::query_as::<_, ProfileResponse>(
        "SELECT id, name, email, role, company, phone FROM profiles WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error fetching profile: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    match row {
        Some(profile) => Ok(Json(profile).into_response()),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Profile not found. Please update it first." })),
        )
            .into_response()),
    }
}

pub async fn update_profile(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
    Json(payload): Json<UpdateProfile>,
) -> Result<Response, Response> {
    let user_id = extract_user_id(&headers).await?;

    // Use an UPSERT (ON CONFLICT DO UPDATE).
    // This handles the case where a profile doesn't exist yet but the user
    // authenticated via Supabase. Defaults to empty strings for required fields
    // on initial insert if name/email are absent from the payload.
    let email_insert = payload.email.clone().unwrap_or_default();
    let name_insert = payload.name.clone().unwrap_or_default();

    let row = sqlx::query_as::<_, ProfileResponse>(
        r#"
        INSERT INTO profiles (id, name, email, company, phone, role)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (id) DO UPDATE
        SET
            name    = COALESCE($7,  profiles.name),
            email   = COALESCE($8,  profiles.email),
            company = COALESCE($9,  profiles.company),
            phone   = COALESCE($10, profiles.phone),
            role    = COALESCE($11, profiles.role),
            updated_at = NOW()
        RETURNING id, name, email, role, company, phone
        "#,
    )
    .bind(user_id)           // $1  — id
    .bind(&name_insert)      // $2  — INSERT name  (default "")
    .bind(&email_insert)     // $3  — INSERT email (default "")
    .bind(&payload.company)  // $4  — INSERT company
    .bind(&payload.phone)    // $5  — INSERT phone
    .bind(&payload.role)     // $6  — INSERT role
    .bind(&payload.name)     // $7  — UPDATE name
    .bind(&payload.email)    // $8  — UPDATE email
    .bind(&payload.company)  // $9  — UPDATE company
    .bind(&payload.phone)    // $10 — UPDATE phone
    .bind(&payload.role)     // $11 — UPDATE role
    .fetch_one(&pool)        // UPSERT always returns a row — use fetch_one, not fetch_optional
    .await
    .map_err(|e| {
        tracing::error!("DB update error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    // Sync info to team_members if they are in it, or insert them if they match "reverion"
    if let Some(company) = &payload.company {
        if company.to_lowercase().contains("reverion") {
            let role_val = row.role.clone().unwrap_or_else(|| "Employee".to_string());
            let _ = sqlx::query(
                r#"
                INSERT INTO team_members (profile_id, email, name, job_title, status)
                VALUES ($1, $2, $3, $4, 'applicant')
                ON CONFLICT (profile_id) DO UPDATE SET
                    name = EXCLUDED.name,
                    job_title = EXCLUDED.job_title,
                    updated_at = NOW()
                "#,
            )
            .bind(user_id)
            .bind(&row.email)
            .bind(&row.name)
            .bind(&role_val)
            .execute(&pool)
            .await
            .map_err(|e| tracing::error!("Error automatically adding/updating team: {:?}", e));
        } else {
            // Also sync details if they are already in the team, even if company doesn't contain reverion anymore
            let _ = sqlx::query(
                "UPDATE team_members SET name = $1, job_title = $2, updated_at = NOW() WHERE profile_id = $3"
            )
            .bind(&row.name)
            .bind(&row.role)
            .bind(user_id)
            .execute(&pool)
            .await;
        }
    }

    Ok(Json(row).into_response())
}