use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: Uuid,
    pub client_id: Uuid,
    pub name: String,
    pub status: String,
    pub progress: i32,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ProjectMilestone {
    pub id: Uuid,
    pub project_id: Uuid,
    pub label: String,
    pub date: String,
    pub done: bool,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ProjectDeliverable {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ProjectUpdate {
    pub id: Uuid,
    pub project_id: Uuid,
    pub date: String,
    pub phase_id: i32,
    pub content: String,
    pub author: String,
    pub author_name: Option<String>,
}

// Full hydrated project exactly matching the DashboardProject type in TS.
#[derive(Debug, Serialize, Deserialize)]
pub struct HydratedProject {
    pub id: String, // Stringified Uuid
    pub name: String,
    pub status: String,
    pub progress: i32,
    pub milestones: Vec<DashboardMilestone>,
    pub deliverables: Vec<DashboardDeliverable>,
    #[serde(rename = "dailyUpdates")]
    pub daily_updates: Vec<DashboardUpdate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardMilestone {
    pub label: String,
    pub date: String,
    pub done: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardDeliverable {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardUpdate {
    pub id: String,
    pub date: String,
    #[serde(rename = "phaseId")]
    pub phase_id: i32,
    pub content: String,
    pub author: String,
    #[serde(rename = "authorName")]
    pub author_name: Option<String>,
}

// Request Payload DTOs

#[derive(Debug, Deserialize)]
pub struct CreateProjectPayload {
    pub name: String,
    pub client_email: String,
}

#[derive(Debug, Deserialize)]
pub struct PostUpdatePayload {
    pub date: String,
    pub phase_id: i32,
    pub content: String,
    pub author: String,
    pub author_name: Option<String>,
    pub milestones: Option<Vec<DashboardMilestone>>,
    pub deliverables: Option<Vec<DashboardDeliverable>>,
    pub progress: Option<i32>,
}
