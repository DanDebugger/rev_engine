use axum::{
    extract::{Extension, Json, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::project::{
    CreateProjectPayload, DashboardDeliverable, DashboardMilestone, DashboardUpdate,
    HydratedProject, PostUpdatePayload, Project, ProjectDeliverable, ProjectMilestone, ProjectUpdate,
};
use crate::security::jwt::{self, AccessClaims};

/// Helper: Verify JWT
async fn verify_token(headers: &HeaderMap) -> Result<AccessClaims, Response> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Missing authorization header" })),
            )
                .into_response()
        })?;

    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid token format" })),
        )
            .into_response()
    })?;

    jwt::verify_access(token).await.map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token", "details": e.to_string() })),
        )
            .into_response()
    })
}

/// Helper: Check if user is an admin or active team member
async fn is_team_member_or_admin(email: &str, pool: &PgPool) -> Result<bool, Response> {
    if email == "dangreyconcepcion312@gmail.com" {
        return Ok(true);
    }

    let is_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM team_members WHERE email = $1 AND status = 'active')",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error checking team status: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(is_active)
}

/// Helper: Fetch and hydrate projects
async fn fetch_hydrated_projects(
    pool: &PgPool,
    client_id_filter: Option<Uuid>,
) -> Result<Vec<HydratedProject>, Response> {
    // 1. Fetch Projects
    let projects_query = match client_id_filter {
        Some(cid) => sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE client_id = $1").bind(cid),
        None => sqlx::query_as::<_, Project>("SELECT * FROM projects"),
    };

    let projects = projects_query.fetch_all(pool).await.map_err(|e| {
        tracing::error!("DB error fetching projects: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    let mut hydrated_projects = Vec::new();

    for p in projects {
        let p_id = p.id;

        // Milestones
        let milestones = sqlx::query_as::<_, ProjectMilestone>(
            "SELECT * FROM project_milestones WHERE project_id = $1 ORDER BY date ASC",
        )
        .bind(p_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|m| DashboardMilestone {
            label: m.label,
            date: m.date,
            done: m.done,
        })
        .collect();

        // Deliverables
        let deliverables = sqlx::query_as::<_, ProjectDeliverable>(
            "SELECT * FROM project_deliverables WHERE project_id = $1",
        )
        .bind(p_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| DashboardDeliverable {
            name: d.name,
            status: d.status,
        })
        .collect();

        // Updates
        let updates = sqlx::query_as::<_, ProjectUpdate>(
            "SELECT * FROM project_updates WHERE project_id = $1 ORDER BY date ASC",
        )
        .bind(p_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|u| DashboardUpdate {
            id: u.id.to_string(),
            date: u.date,
            phase_id: u.phase_id,
            content: u.content,
            author: u.author,
            author_name: u.author_name,
        })
        .collect();

        hydrated_projects.push(HydratedProject {
            id: p.id.to_string(),
            name: p.name,
            status: p.status,
            progress: p.progress,
            milestones,
            deliverables,
            daily_updates: updates,
        });
    }

    Ok(hydrated_projects)
}

// ==========================================
// Handlers
// ==========================================

pub async fn get_projects(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let claims = verify_token(&headers).await?;
    let is_team = is_team_member_or_admin(&claims.email, &pool).await?;

    let mut user_id_filter = None;
    if !is_team {
        // If they are strictly a client, filter projects by their auth.users ID.
        if let Ok(id) = Uuid::parse_str(&claims.sub) {
            user_id_filter = Some(id);
        }
    }

    let hydrated = fetch_hydrated_projects(&pool, user_id_filter).await?;
    Ok(Json(hydrated).into_response())
}

pub async fn create_project(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
    Json(payload): Json<CreateProjectPayload>,
) -> Result<Response, Response> {
    let claims = verify_token(&headers).await?;
    let is_team = is_team_member_or_admin(&claims.email, &pool).await?;

    if !is_team {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Only Reverion team members can create projects." })),
        )
            .into_response());
    }

    // Lookup the user ID for the client email
    let client_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM auth.users WHERE email = $1")
        .bind(&payload.client_email)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Error finding client for new project: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?;

    let client_id = match client_id {
        Some(id) => id,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Client email not found in auth.users" })),
            )
                .into_response());
        }
    };

    let p_id = Uuid::new_v4();

    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

    sqlx::query(
        "INSERT INTO projects (id, client_id, name, status, progress) VALUES ($1, $2, $3, 'active', 0)"
    )
    .bind(p_id)
    .bind(client_id)
    .bind(&payload.name)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!("DB error inserting project: {:?}", e);
        if e.to_string().contains("projects_client_id_key") {
            (StatusCode::CONFLICT, Json(json!({"error": "This client already has an active project."}))).into_response()
        } else {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    })?;

    // Default milestones
    let default_mz = vec![
        ("Discovery & Planning", ""),
        ("Design", ""),
        ("Development", ""),
        ("Testing & Launch", ""),
    ];
    for (label, date) in default_mz {
        sqlx::query("INSERT INTO project_milestones (project_id, label, date, done) VALUES ($1, $2, $3, false)")
            .bind(p_id)
            .bind(label)
            .bind(date)
            .execute(&mut *tx)
            .await
            .unwrap();
    }

    // Default deliverables
    let default_dz = vec!["Requirements Doc", "Design Mockups", "Working Build"];
    for name in default_dz {
        sqlx::query("INSERT INTO project_deliverables (project_id, name, status) VALUES ($1, $2, 'Pending')")
            .bind(p_id)
            .bind(name)
            .execute(&mut *tx)
            .await
            .unwrap();
    }

    tx.commit().await.unwrap();

    // Fetch the hydrated project to return
    let created = fetch_hydrated_projects(&pool, Some(client_id)).await?;
    let proj = created.into_iter().next().unwrap();

    Ok(Json(proj).into_response())
}

pub async fn post_update(
    Extension(pool): Extension<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<PostUpdatePayload>,
) -> Result<Response, Response> {
    let claims = verify_token(&headers).await?;
    let is_team = is_team_member_or_admin(&claims.email, &pool).await?;

    let mut is_authorized = is_team;

    // If client, verify they actually own this project
    if !is_team {
        let owner_id = sqlx::query_scalar::<_, Uuid>("SELECT client_id FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;
        
        if let Some(owner) = owner_id {
            if let Ok(user_id) = Uuid::parse_str(&claims.sub) {
                if owner == user_id {
                    is_authorized = true;
                }
            }
        }
    }

    if !is_authorized {
        return Err(StatusCode::FORBIDDEN.into_response());
    }

    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

    // If team member and milestones/deliverables/progress are provided, update them.
    if is_team {
        if let Some(prog) = payload.progress {
            sqlx::query("UPDATE projects SET progress = $1, updated_at = NOW() WHERE id = $2")
                .bind(prog)
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }

        if let Some(milestones) = payload.milestones {
            // Delete old milestones
            sqlx::query("DELETE FROM project_milestones WHERE project_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();

            // Insert new ones
            for m in milestones {
                sqlx::query("INSERT INTO project_milestones (project_id, label, date, done) VALUES ($1, $2, $3, $4)")
                    .bind(id)
                    .bind(m.label)
                    .bind(m.date)
                    .bind(m.done)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }

        if let Some(deliverables) = payload.deliverables {
            sqlx::query("DELETE FROM project_deliverables WHERE project_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();

            for d in deliverables {
                sqlx::query("INSERT INTO project_deliverables (project_id, name, status) VALUES ($1, $2, $3)")
                    .bind(id)
                    .bind(d.name)
                    .bind(d.status)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }
    }

    // Insert the actual feed update
    sqlx::query("INSERT INTO project_updates (project_id, date, phase_id, content, author, author_name) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(id)
        .bind(&payload.date)
        .bind(payload.phase_id)
        .bind(&payload.content)
        .bind(&payload.author)
        .bind(&payload.author_name)
        .execute(&mut *tx)
        .await
        .unwrap();

    tx.commit().await.unwrap();

    Ok(Json(json!({"success": true})).into_response())
}
