//! Forge-specific API routes
//!
//! These routes handle forge-specific functionality that extends the base VK capabilities:
//! - Global and per-project settings
//! - Omni notification integration
//! - Project branch status and git operations
//! - GitHub releases
//! - Agent task management

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use forge_core_db::models::project::Project;
use forge_core_deployment::Deployment;
use forge_core_services::services::{
    forge_config::ForgeProjectSettings,
    omni::{OmniConfig, OmniInstance, OmniService},
};
use forge_core_utils::response::ApiResponse;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    Router::new()
        // Config routes
        .route(
            "/forge/config",
            get(get_forge_config).put(update_forge_config),
        )
        // Project settings routes
        .route(
            "/forge/projects/{project_id}/settings",
            get(get_project_settings).put(update_project_settings),
        )
        .route(
            "/forge/projects/{project_id}/branch-status",
            get(get_project_branch_status),
        )
        .route("/forge/projects/{project_id}/pull", post(post_project_pull))
        // Omni routes
        .route("/forge/omni/status", get(get_omni_status))
        .route("/forge/omni/instances", get(list_omni_instances))
        .route("/forge/omni/validate", post(validate_omni_config))
        .route("/forge/omni/notifications", get(list_omni_notifications))
        // GitHub releases
        .route("/forge/releases", get(get_github_releases))
        // Agent management
        .route(
            "/forge/agents",
            get(get_forge_agents).post(create_forge_agent),
        )
        .with_state(deployment.clone())
}

// ============================================================================
// Config endpoints
// ============================================================================

async fn get_forge_config(
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<ForgeProjectSettings>>, StatusCode> {
    deployment
        .forge_config()
        .get_global_settings()
        .await
        .map(|settings| Json(ApiResponse::success(settings)))
        .map_err(|e| {
            tracing::error!("Failed to load forge config: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn update_forge_config(
    State(deployment): State<DeploymentImpl>,
    Json(settings): Json<ForgeProjectSettings>,
) -> Result<Json<ApiResponse<ForgeProjectSettings>>, StatusCode> {
    deployment
        .forge_config()
        .set_global_settings(&settings)
        .await
        .map_err(|e| {
            tracing::error!("Failed to persist forge config: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Apply omni config changes
    if let Some(omni_config) = &settings.omni_config {
        let mut omni = deployment.omni().write().await;
        let mut config = omni_config.clone();
        config.enabled = settings.omni_enabled;
        omni.apply_config(config);
    }

    Ok(Json(ApiResponse::success(settings)))
}

async fn get_project_settings(
    Path(project_id): Path<Uuid>,
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<ForgeProjectSettings>>, StatusCode> {
    deployment
        .forge_config()
        .get_forge_settings(project_id)
        .await
        .map(|settings| Json(ApiResponse::success(settings)))
        .map_err(|e| {
            tracing::error!("Failed to load project settings {}: {}", project_id, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn update_project_settings(
    Path(project_id): Path<Uuid>,
    State(deployment): State<DeploymentImpl>,
    Json(settings): Json<ForgeProjectSettings>,
) -> Result<Json<ApiResponse<ForgeProjectSettings>>, StatusCode> {
    deployment
        .forge_config()
        .set_forge_settings(project_id, &settings)
        .await
        .map_err(|e| {
            tracing::error!("Failed to persist project settings {}: {}", project_id, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(ApiResponse::success(settings)))
}

// ============================================================================
// Branch status endpoints
// ============================================================================

#[derive(Deserialize)]
struct BranchStatusQuery {
    base: Option<String>,
}

async fn get_project_branch_status(
    Path(project_id): Path<Uuid>,
    Query(query): Query<BranchStatusQuery>,
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<Value>>, StatusCode> {
    use std::process::Command;

    let project = match Project::find_by_id(&deployment.db().pool, project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::error!("Project {} not found", project_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Database error finding project {}: {}", project_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get current branch
    let current_branch_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();

    let current_branch = match current_branch_output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "main".to_string(),
    };

    let target_branch = query.base.as_deref().unwrap_or("main");

    // Fetch from remote
    let _ = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["fetch", "origin"])
        .output();

    // Compare against remote tracking branch
    let remote_branch = format!("origin/{target_branch}");
    let commits_behind_ahead_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args([
            "rev-list",
            "--left-right",
            "--count",
            &format!("{remote_branch}...{current_branch}"),
        ])
        .output();

    let (commits_behind, commits_ahead) = match commits_behind_ahead_output {
        Ok(output) if output.status.success() => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = output_str.split_whitespace().collect();
            if parts.len() == 2 {
                (parts[0].parse::<i32>().ok(), parts[1].parse::<i32>().ok())
            } else {
                (None, None)
            }
        }
        _ => (None, None),
    };

    // Get remote commits behind/ahead
    let upstream_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["rev-parse", "--abbrev-ref", "@{u}"])
        .output();

    let (remote_commits_behind, remote_commits_ahead) = match upstream_output {
        Ok(output) if output.status.success() => {
            let remote_tracking_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let remote_commits_output = Command::new("git")
                .current_dir(&project.git_repo_path)
                .args([
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{remote_tracking_branch}...{current_branch}"),
                ])
                .output();

            match remote_commits_output {
                Ok(output) if output.status.success() => {
                    let output_str = String::from_utf8_lossy(&output.stdout);
                    let parts: Vec<&str> = output_str.split_whitespace().collect();
                    if parts.len() == 2 {
                        (parts[0].parse::<i32>().ok(), parts[1].parse::<i32>().ok())
                    } else {
                        (None, None)
                    }
                }
                _ => (None, None),
            }
        }
        _ => (None, None),
    };

    // Check for uncommitted changes
    let status_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["status", "--porcelain"])
        .output();

    let (has_uncommitted_changes, uncommitted_count, untracked_count) = match status_output {
        Ok(output) if output.status.success() => {
            let status_str = String::from_utf8_lossy(&output.stdout).to_string();
            let status_lines: Vec<&str> = status_str.lines().collect();
            let uncommitted = status_lines.iter().filter(|l| !l.starts_with("??")).count();
            let untracked = status_lines.iter().filter(|l| l.starts_with("??")).count();
            (
                !status_lines.is_empty(),
                Some(uncommitted as i32),
                Some(untracked as i32),
            )
        }
        _ => (false, None, None),
    };

    // Get HEAD commit OID
    let head_oid_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["rev-parse", "HEAD"])
        .output();

    let head_oid = match head_oid_output {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => None,
    };

    let response = json!({
        "commits_behind": commits_behind,
        "commits_ahead": commits_ahead,
        "has_uncommitted_changes": has_uncommitted_changes,
        "head_oid": head_oid,
        "uncommitted_count": uncommitted_count,
        "untracked_count": untracked_count,
        "target_branch_name": target_branch,
        "remote_commits_behind": remote_commits_behind,
        "remote_commits_ahead": remote_commits_ahead,
        "merges": [],
        "is_rebase_in_progress": false,
        "conflict_op": null,
        "conflicted_files": []
    });

    Ok(Json(ApiResponse::success(response)))
}

async fn post_project_pull(
    Path(project_id): Path<Uuid>,
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<Value>, StatusCode> {
    use std::process::Command;

    let project = match Project::find_by_id(&deployment.db().pool, project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::error!("Project {} not found", project_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Database error finding project {}: {}", project_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let branch_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();

    let current_branch = match branch_output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(
                "Failed to get current branch for project {}: {}",
                project_id,
                stderr
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        Err(e) => {
            tracing::error!(
                "Failed to execute git rev-parse for project {}: {}",
                project_id,
                e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    tracing::info!(
        "Pulling updates for project {} branch {} at {:?}",
        project_id,
        current_branch,
        project.git_repo_path
    );

    let pull_output = Command::new("git")
        .current_dir(&project.git_repo_path)
        .args(["pull", "--rebase", "origin", &current_branch])
        .output();

    match pull_output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            tracing::info!(
                "Successfully pulled updates for project {}: {}",
                project_id,
                stdout
            );
            Ok(Json(json!({
                "success": true,
                "message": format!("Successfully pulled updates from origin/{}", current_branch)
            })))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            if stderr.contains("conflict") || stderr.contains("Cannot rebase") {
                tracing::warn!(
                    "Git pull conflict for project {}: {} {}",
                    project_id,
                    stdout,
                    stderr
                );
                Ok(Json(json!({
                    "success": false,
                    "message": "Cannot pull: working tree has conflicts or uncommitted changes. Please resolve manually.",
                    "details": stderr.to_string()
                })))
            } else {
                tracing::error!(
                    "Git pull failed for project {}: {} {}",
                    project_id,
                    stdout,
                    stderr
                );
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
        Err(e) => {
            tracing::error!(
                "Failed to execute git pull for project {}: {}",
                project_id,
                e
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ============================================================================
// Omni endpoints
// ============================================================================

async fn get_omni_status(
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<Value>, StatusCode> {
    let omni = deployment.omni().read().await;
    let config = omni.config();

    Ok(Json(json!({
        "enabled": config.enabled,
        "config": if config.enabled {
            serde_json::to_value(config).ok()
        } else {
            None
        }
    })))
}

async fn list_omni_instances(
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<Value>, StatusCode> {
    let omni = deployment.omni().read().await;
    match omni.list_instances().await {
        Ok(instances) => Ok(Json(json!({ "instances": instances }))),
        Err(e) => {
            tracing::error!("Failed to list Omni instances: {}", e);
            Ok(Json(json!({
                "instances": [],
                "error": "Failed to connect to Omni service"
            })))
        }
    }
}

async fn list_omni_notifications(
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<Value>, StatusCode> {
    let rows = sqlx::query(
        r#"SELECT
                id,
                task_id,
                notification_type,
                status,
                message,
                error_message,
                sent_at,
                created_at,
                metadata
           FROM forge_omni_notifications
          ORDER BY created_at DESC
          LIMIT 50"#,
    )
    .fetch_all(&deployment.db().pool)
    .await
    .map_err(|error| {
        tracing::error!("Failed to fetch Omni notifications: {}", error);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut notifications = Vec::with_capacity(rows.len());

    for row in rows {
        let metadata = match row.try_get::<Option<String>, _>("metadata") {
            Ok(Some(raw)) => serde_json::from_str::<Value>(&raw).ok(),
            _ => None,
        };

        let record = json!({
            "id": row.try_get::<String, _>("id").unwrap_or_default(),
            "task_id": row.try_get::<Option<String>, _>("task_id").unwrap_or(None),
            "notification_type": row
                .try_get::<String, _>("notification_type")
                .unwrap_or_else(|_| "unknown".to_string()),
            "status": row
                .try_get::<String, _>("status")
                .unwrap_or_else(|_| "pending".to_string()),
            "message": row.try_get::<Option<String>, _>("message").unwrap_or(None),
            "error_message": row
                .try_get::<Option<String>, _>("error_message")
                .unwrap_or(None),
            "sent_at": row.try_get::<Option<String>, _>("sent_at").unwrap_or(None),
            "created_at": row
                .try_get::<String, _>("created_at")
                .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339()),
            "metadata": metadata,
        });

        notifications.push(record);
    }

    Ok(Json(json!({ "notifications": notifications })))
}

#[derive(Debug, Deserialize)]
struct ValidateOmniRequest {
    host: String,
    api_key: String,
}

#[derive(Debug, Serialize)]
struct ValidateOmniResponse {
    valid: bool,
    instances: Vec<OmniInstance>,
    error: Option<String>,
}

async fn validate_omni_config(
    State(_deployment): State<DeploymentImpl>,
    Json(req): Json<ValidateOmniRequest>,
) -> Result<Json<ValidateOmniResponse>, StatusCode> {
    let temp_config = OmniConfig {
        enabled: false,
        host: Some(req.host),
        api_key: Some(req.api_key),
        instance: None,
        recipient: None,
        recipient_type: None,
    };

    let temp_service = OmniService::new(temp_config);
    match temp_service.list_instances().await {
        Ok(instances) => Ok(Json(ValidateOmniResponse {
            valid: true,
            instances,
            error: None,
        })),
        Err(e) => Ok(Json(ValidateOmniResponse {
            valid: false,
            instances: vec![],
            error: Some(format!("Configuration validation failed: {e}")),
        })),
    }
}

// ============================================================================
// GitHub releases endpoint
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: String,
    body: Option<String>,
    prerelease: bool,
    created_at: String,
    published_at: Option<String>,
    html_url: String,
}

async fn get_github_releases() -> Result<Json<ApiResponse<Vec<GitHubRelease>>>, StatusCode> {
    let client = reqwest::Client::new();

    match client
        .get("https://api.github.com/repos/automagik-dev/automagik-forge/releases")
        .header("User-Agent", "automagik-forge")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<Vec<GitHubRelease>>().await {
                    Ok(releases) => Ok(Json(ApiResponse::success(releases))),
                    Err(e) => {
                        tracing::error!("Failed to parse GitHub releases: {}", e);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            } else {
                tracing::error!("GitHub API returned error: {}", response.status());
                Err(StatusCode::BAD_GATEWAY)
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch GitHub releases: {}", e);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

// ============================================================================
// Agent management endpoints
// ============================================================================

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
struct ForgeAgent {
    id: Uuid,
    project_id: Uuid,
    agent_type: String,
    task_id: Uuid,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct GetForgeAgentsParams {
    project_id: Uuid,
    agent_type: Option<String>,
}

async fn get_forge_agents(
    State(deployment): State<DeploymentImpl>,
    Query(params): Query<GetForgeAgentsParams>,
) -> Result<Json<ApiResponse<Vec<ForgeAgent>>>, ApiError> {
    let pool = &deployment.db().pool;

    let agents = if let Some(agent_type) = params.agent_type {
        sqlx::query_as::<_, ForgeAgent>(
            "SELECT * FROM forge_agents WHERE project_id = ? AND agent_type = ?",
        )
        .bind(params.project_id)
        .bind(agent_type)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, ForgeAgent>("SELECT * FROM forge_agents WHERE project_id = ?")
            .bind(params.project_id)
            .fetch_all(pool)
            .await?
    };

    Ok(Json(ApiResponse::success(agents)))
}

#[derive(Debug, Deserialize)]
struct CreateForgeAgentBody {
    project_id: Uuid,
    agent_type: String,
}

async fn create_forge_agent(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateForgeAgentBody>,
) -> Result<Json<ApiResponse<ForgeAgent>>, ApiError> {
    let pool = &deployment.db().pool;
    let agent_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let title = "Genie".to_string();

    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, title, description, status, created_at, updated_at)
           VALUES (?, ?, ?, NULL, 'agent', datetime('now'), datetime('now'))"#,
    )
    .bind(task_id)
    .bind(payload.project_id)
    .bind(&title)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"INSERT INTO forge_agents (id, project_id, agent_type, task_id, created_at, updated_at)
           VALUES (?, ?, ?, ?, datetime('now'), datetime('now'))"#,
    )
    .bind(agent_id)
    .bind(payload.project_id)
    .bind(&payload.agent_type)
    .bind(task_id)
    .execute(pool)
    .await?;

    let agent: ForgeAgent = sqlx::query_as("SELECT * FROM forge_agents WHERE id = ?")
        .bind(agent_id)
        .fetch_one(pool)
        .await?;

    Ok(Json(ApiResponse::success(agent)))
}
