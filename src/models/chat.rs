use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Conversation {
    pub id: Uuid,
    pub title: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ChatHistoryItem {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: String, // 'user' or 'assistant'
    pub content: String,
    pub created_at: DateTime<Utc>,
}
