
use anyhow::{anyhow, Result};
use chrono::Utc;
use jsonwebtoken::{
    decode, encode,
    jwk::JwkSet,
    Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessClaims {
    #[serde(default)]
    pub iss: String,
    pub sub: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub provider: String,
    pub exp: i64,
    #[serde(default)]
    pub iat: i64,
}

pub fn sign_access(auth_id: &uuid::Uuid, email: &str, provider: &str) -> Result<String> {
    let now = Utc::now().timestamp();
    let exp_minutes: i64 = std::env::var("ACCESS_TOKEN_TTL_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    let claims = AccessClaims {
        iss: "".into(),
        sub: auth_id.to_string(),
        email: email.into(),
        provider: provider.into(),
        iat: now,
        exp: now + (exp_minutes * 60),
    };

    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET missing");
    Ok(encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

/// Cached Supabase JWKS for ES256 verification.
static SUPABASE_JWKS: OnceCell<JwkSet> = OnceCell::const_new();

fn supabase_jwks_url() -> Result<String> {
    if let Ok(url) = std::env::var("SUPABASE_JWKS_URL") {
        return Ok(url);
    }

    // Derive JWKS URL from SUPABASE_STORAGE_URL if not explicitly set.
    if let Ok(storage_url) = std::env::var("SUPABASE_STORAGE_URL") {
        // Expected format: https://<project-ref>.storage.supabase.co/...
        if let Some(host_start) = storage_url.strip_prefix("https://") {
            if let Some(host) = host_start.split('/').next() {
                if let Some(project_ref) = host.split('.').next() {
                    // Standard JWKS endpoint for Supabase projects.
                    let jwks = format!(
                        "https://{}.supabase.co/auth/v1/.well-known/jwks.json",
                        project_ref
                    );
                    return Ok(jwks);
                }
            }
        }
    }

    Err(anyhow!(
        "SUPABASE_JWKS_URL or SUPABASE_STORAGE_URL must be set for ES256 JWT verification"
    ))
}

async fn load_supabase_jwks() -> Result<&'static JwkSet> {
    SUPABASE_JWKS
        .get_or_try_init(|| async {
            let url = supabase_jwks_url()?;
            tracing::info!("Fetching Supabase JWKS from {}", url);

            let client = Client::new();

            // Some Supabase setups require an apikey / Authorization header even for /auth/v1/keys.
            let mut req = client.get(&url);
            if let Ok(apikey) =
                std::env::var("SUPABASE_ANON_KEY").or_else(|_| std::env::var("SUPABASE_SERVICE_ROLE_KEY"))
            {
                req = req
                    .header("apikey", &apikey)
                    .header("Authorization", format!("Bearer {}", apikey));
            }

            let resp = req.send().await?;
            if !resp.status().is_success() {
                return Err(anyhow!(
                    "Failed to fetch JWKS from {}: HTTP {}",
                    url,
                    resp.status()
                ));
            }

            let jwks: JwkSet = resp.json().await?;
            Ok(jwks)
        })
        .await
}

/// Optionally warm up the Supabase JWKS cache at startup so the first
/// authenticated request doesn't pay the network cost.
pub async fn warm_up_supabase_jwks() {
    if let Err(e) = load_supabase_jwks().await {
        tracing::warn!("Supabase JWKS warm-up failed: {:?}", e);
    }
}

fn verify_hs256(token: &str) -> Result<AccessClaims> {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET missing");
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_aud = false;

    let data = decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(data.claims)
}

async fn verify_es256_with_supabase(token: &str, kid: Option<String>) -> Result<AccessClaims> {
    let kid = kid.ok_or_else(|| anyhow!("Missing 'kid' in JWT header for ES256 token"))?;
    let jwks = load_supabase_jwks().await?;

    let jwk = jwks
        .find(&kid)
        .ok_or_else(|| anyhow!("No JWK found for kid {}", kid))?;

    let decoding_key = DecodingKey::from_jwk(jwk)?;

    let mut validation = Validation::new(Algorithm::ES256);
    validation.validate_aud = false;

    let data = decode::<AccessClaims>(token, &decoding_key, &validation)?;
    Ok(data.claims)
}

pub async fn verify_access(token: &str) -> Result<AccessClaims> {
    let header = jsonwebtoken::decode_header(token)
        .map_err(|e| anyhow!("Failed to decode JWT header: {:?}", e))?;

    // Downgrade header logging to debug to avoid noisy logs in production.
    tracing::debug!("JWT Header: {:?}", header);

    match header.alg {
        Algorithm::HS256 => verify_hs256(token),
        Algorithm::ES256 => verify_es256_with_supabase(token, header.kid).await,
        other => Err(anyhow!("Unsupported JWT algorithm: {:?}", other)),
    }
}
