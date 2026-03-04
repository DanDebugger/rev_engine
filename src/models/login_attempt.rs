use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, FromRow)]
pub struct LoginAttempt {
    pub id: i32,
    pub email: String,
    pub ip_address: String,
    pub method: String,
    pub success: bool,
    pub user_agent: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateLoginAttempt {
    pub email: String,
    pub method: String,
    pub success: bool,
    #[serde(default)]
    pub error_message: Option<String>,
}
