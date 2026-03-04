use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, FromRow)]
pub struct ContactRequest {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub company: Option<String>,
    pub service: Option<String>,
    pub budget: Option<String>,
    pub description: String,
    pub timeline: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateContactRequest {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub budget: String,
    pub description: String,
    #[serde(default)]
    pub timeline: String,
}
