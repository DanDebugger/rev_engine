use axum::{
    body::Body,
    middleware::Next,
    response::{IntoResponse, Response},
    http::{Request, StatusCode, Method},
};
use http_body_util::BodyExt; // For collect()
use crate::utils::crypto;
use serde_json::json;
use std::env;

pub async fn security_layer(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    tracing::info!("Security layer intercepting {} {}", method, uri);
    let response = next.run(req).await;

    // Skip encryption for CORS preflight (OPTIONS)
    if method == Method::OPTIONS {
        tracing::debug!("Skipping security layer for OPTIONS request");
        return response;
    }

    // Only encrypt successful JSON-like responses OR as requested by user ("every response")
    // We'll skip encryption for non-200 if desired, but user said "every response".
    // However, we must be careful with streaming bodies.
    
    let (parts, body) = response.into_parts();
    
    // Extract preview if provided via extensions (optional feature)
    let preview = parts.extensions.get::<String>().cloned().unwrap_or_else(|| "Response Encrypted".to_string());

    // Collect the body bytes
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let plaintext = String::from_utf8_lossy(&bytes);
    
    // Get encryption key from environment
    let key_str = env::var("CRYPTO_KEY").unwrap_or_else(|_| "00000000000000000000000000000000".to_string());
    let mut key = [0u8; 32];
    let key_bytes = key_str.as_bytes();
    let len = key_bytes.len().min(32);
    key[..len].copy_from_slice(&key_bytes[..len]);

    // Encrypt and sign
    match crypto::encrypt_and_sign(&key, &plaintext, &preview) {
        Ok(envelope) => {
            let json_body = json!(envelope).to_string();
            Response::from_parts(parts, Body::from(json_body))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Security Layer Error: {}", e)).into_response()
        }
    }
}
