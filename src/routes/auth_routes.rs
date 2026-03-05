use axum::{routing::{get, post, put, patch, delete}, Router};
use crate::api::auth;
use crate::api::auth_contact as contact;
use crate::api::auth_login_attempts as login_attempts;
use crate::api::auth_profile as profile;
use crate::api::team;
use crate::api::project;
use crate::api::chat;
use crate::api::llm as api_llm;
use crate::api::rag;

pub fn routes() -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/contact", post(contact::submit_contact))
        .route("/contacts", get(contact::list_contacts))
        .route("/contacts", delete(contact::clear_contacts))
        .route("/contacts/{id}", patch(contact::update_contact_status))
        .route("/contacts/{id}", delete(contact::delete_contact))
        .route("/login-attempts", post(login_attempts::record_attempt))
        .route("/login-attempts", get(login_attempts::list_attempts))
        .route("/profile", get(profile::get_profile))
        .route("/profile", put(profile::update_profile))
        .route("/team/me", get(team::get_my_team_status))
        .route("/team", get(team::list_team_members))
        .route("/team/{id}", patch(team::update_team_member_status))
        .route("/team/{id}", delete(team::remove_team_member))
        .route("/projects", get(project::get_projects))
        .route("/projects", post(project::create_project))
        .route("/projects/{id}/updates", post(project::post_update))
        .route("/chat", post(chat::handle_chat))
        .route("/llm/models", get(api_llm::list_models))
        .route("/ingest/text", post(rag::ingest_text))
        .route("/ingest/url", post(rag::ingest_url))
        .route("/ingest/pdf", post(rag::ingest_pdf))
}

async fn root_handler() -> &'static str {
    "Server is running."
}