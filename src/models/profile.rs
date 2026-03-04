use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct UpdateProfile {
    pub name: Option<String>,
    pub email: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub role: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
}
