use axum::{
    extract::{ConnectInfo, Extension, Json},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;
use sqlx::PgPool;
use crate::models::login_attempt::{CreateLoginAttempt, LoginAttempt};

/// Extract real client IP: X-Forwarded-For > X-Real-IP > socket address.
fn extract_ip(headers: &HeaderMap, connect_info: &ConnectInfo<SocketAddr>) -> String {
    // X-Forwarded-For: client, proxy1, proxy2
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first_ip) = forwarded.split(',').next() {
            let ip = first_ip.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }
    // X-Real-IP (single IP)
    if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let ip = real_ip.trim();
        if !ip.is_empty() {
            return ip.to_string();
        }
    }
    // Fallback: actual TCP socket address
    connect_info.0.ip().to_string()
}

fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub async fn record_attempt(
    connect_info: ConnectInfo<SocketAddr>,
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
    Json(payload): Json<CreateLoginAttempt>,
) -> Result<Response, Response> {
    let ip = extract_ip(&headers, &connect_info);
    let user_agent = extract_user_agent(&headers);

    let attempt: LoginAttempt = sqlx::query_as::<sqlx::Postgres, LoginAttempt>(
        "INSERT INTO login_attempts (email, ip_address, method, success, user_agent, error_message) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *"
    )
    .bind(&payload.email)
    .bind(&ip)
    .bind(&payload.method)
    .bind(payload.success)
    .bind(&user_agent)
    .bind(&payload.error_message)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to record login attempt: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Json(attempt).into_response())
}

pub async fn list_attempts(
    Extension(pool): Extension<PgPool>,
) -> Result<Response, Response> {
    let rows: Vec<LoginAttempt> = sqlx::query_as::<sqlx::Postgres, LoginAttempt>(
        "SELECT * FROM login_attempts ORDER BY created_at DESC LIMIT 100"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to list login attempts: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Json(rows).into_response())
}
