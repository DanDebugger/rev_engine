use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct TeamMember {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub email: String,
    pub name: String,
    pub job_title: Option<String>,
    pub status: String, // 'applicant', 'active', 'inactive'
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusPayload {
    pub status: String,
}
