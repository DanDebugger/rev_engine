
pub mod utils;
pub mod api;
mod routes;
pub mod db;
pub mod limiter;
pub mod models;
pub mod security;
pub mod infra;

// use axum::{body::Body, extract::State, http::{Method, Request, StatusCode}, middleware::{self, Next}, response::Response, Extension};
use axum::{http::{Method}, middleware::{self}, Extension};
use dotenvy::dotenv;
use std::{env, net::SocketAddr};
// use std::env;
use tower_http::{cors::{CorsLayer}, set_header::SetResponseHeaderLayer, trace::TraceLayer,};
use db::init_db_pool;
use routes::auth_routes;
use limiter::{ConcurrencyLimiter, enforce_concurrency};
// async fn enforce_origin(
//     State(allowed_origin): State<Arc<String>>,
//     req: Request<Body>,
//     next: Next,
// ) -> Result<Response, StatusCode> {
//     let origin = req.headers().get("origin").and_then(|v| v.to_str().ok());

//     if let Some(origin) = origin {
//         if origin == allowed_origin.as_str() {
//             return Ok(next.run(req).await);
//         }
//     }

//     Err(StatusCode::FORBIDDEN)
// }

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let client_origin = env::var("CLIENT_URL").unwrap_or_else(|_| "http://localhost:5173".to_string());
    // let allowed_origin = Arc::new(client_origin.clone());
    let limiter = ConcurrencyLimiter::new(5);

    let port = env::var("PORT").unwrap_or_else(|_| "5000".to_string());

    let cors = CorsLayer::new()
        .allow_origin(
            client_origin.parse::<axum::http::HeaderValue>().unwrap()
        )
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
            axum::http::header::ACCEPT_LANGUAGE,
            axum::http::header::ACCEPT_ENCODING,
            axum::http::header::CACHE_CONTROL,
            axum::http::header::PRAGMA,
            axum::http::header::EXPIRES,
        ])
        .allow_credentials(true);

    let db_pool = init_db_pool().await;

    // Warm up Supabase JWKS in the background so the first
    // authenticated request doesn't incur the network latency.
    tokio::spawn(async {
        security::jwt::warm_up_supabase_jwks().await;
    });

    let app = auth_routes::routes()
        .layer(Extension(db_pool))
        // .route_layer(middleware::from_fn_with_state(
        //     allowed_origin.clone()
        // ))
        .layer(middleware::from_fn(move |req, next| {
            enforce_concurrency(limiter.clone(), req, next)
        }))
        .layer(cors)
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            axum::http::HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(TraceLayer::new_for_http());

    let addr = format!("0.0.0.0:{}", port);
    println!("Server running on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
