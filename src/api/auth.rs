use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sqlx::PgPool;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use crate::models::auth_user::{AuthResponse, CreateUser, LoginUser, User};
use crate::security::jwt;

use serde_json::json;

pub async fn register(
    Extension(pool): Extension<PgPool>,
    Json(payload): Json<CreateUser>,
) -> Result<Response, Response> {
    // Validate password
    if payload.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Password must be at least 8 characters long" })),
        )
            .into_response());
    }
    if !payload
        .password
        .chars()
        .next()
        .map_or(false, |c: char| c.is_uppercase())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Password must start with a capital letter" })),
        )
            .into_response());
    }
    if !payload.password.chars().any(|c: char| !c.is_alphanumeric()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Password must contain at least one special character" })),
        )
            .into_response());
    }

    // Check if user exists
    let user_exists: Option<User> = sqlx::query_as::<sqlx::Postgres, User>("SELECT * FROM users WHERE email = $1")
        .bind(&payload.email)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?;

    if user_exists.is_some() {
        return Err(StatusCode::CONFLICT.into_response());
    }

    // Hash password
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(payload.password.as_bytes(), &salt)
        .map_err(|e| {
            tracing::error!("Hashing error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?
        .to_string();

    // Insert user
    let user: User = sqlx::query_as::<sqlx::Postgres, User>(
        "INSERT INTO users (email, password_hash, name) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(&payload.email)
    .bind(password_hash)
    .bind(&payload.name)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database insert error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    // Generate token
    let token = jwt::sign_access(&user.id, &user.email, "local")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

    let response = AuthResponse { token, user };

    Ok(Json(response).into_response())
}

pub async fn login(
    Extension(pool): Extension<PgPool>,
    Json(payload): Json<LoginUser>,
) -> Result<Response, StatusCode> {
    // Find user
    let user: Option<User> = sqlx::query_as::<sqlx::Postgres, User>(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&payload.email)
    .fetch_optional(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = match user {
        Some(u) => u,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Verify password
    let parsed_hash = PasswordHash::new(&user.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Generate token
    let token = jwt::sign_access(&user.id, &user.email, "local")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let response = AuthResponse { token, user };

    Ok(Json(response).into_response())
}
