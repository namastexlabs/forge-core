use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json as ResponseJson,
    routing::{get, post},
    Extension, Json, Router,
};
use db::models::project::Project;
use serde::{Deserialize, Serialize};
use services::services::git_remote::{
    BranchSyncStatus, FetchResult, GitRemoteService, PullResult, PullStrategy,
};
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{error::ApiError, middleware::load_project_middleware, DeploymentImpl};

/// POST /projects/:id/fetch
///
/// Manually fetch all tracked branches from origin.
/// Runs in background and returns immediately.
pub async fn fetch_project(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<FetchTaskResponse>>, ApiError> {
    tracing::info!("Fetching remote for project: {}", project.id);

    // Get GitHub token
    let github_token = {
        let config = deployment.config().read().await;
        config.github.token.clone()
    };

    if github_token.is_none() {
        return Ok(ResponseJson(ApiResponse::error(
            "GitHub token not configured. Please authenticate with GitHub first.",
        )));
    }

    let token = github_token.unwrap();
    let repo_path = project.git_repo_path.clone();
    let project_id = project.id.clone();

    // Spawn background task (don't block request)
    tokio::task::spawn_blocking(move || {
        let git_remote_service = GitRemoteService::new();
        let path = std::path::Path::new(&repo_path);

        match git_remote_service.fetch_project(path, &token) {
            Ok(result) => {
                tracing::info!(
                    "Fetched {} branches for project {} in {}ms",
                    result.branches_fetched,
                    project_id,
                    result.duration_ms
                );
            }
            Err(e) => {
                tracing::error!("Fetch failed for project {}: {}", project_id, e);
            }
        }
    });

    Ok(ResponseJson(ApiResponse::success(FetchTaskResponse {
        message: "Fetch started in background".to_string(),
    })))
}

/// GET /projects/:id/sync-status
///
/// Get current sync status for all branches.
/// Always returns fresh data (no cache).
pub async fn get_sync_status(
    Extension(project): Extension<Project>,
) -> Result<ResponseJson<ApiResponse<ProjectSyncStatusResponse>>, ApiError> {
    tracing::debug!("Getting sync status for project: {}", project.id);

    let repo_path = project.git_repo_path.clone();
    let project_id = project.id.clone();

    // MEASURE: Start timing
    let start = std::time::Instant::now();

    // Get sync status (always fresh, no cache)
    let git_remote_service = GitRemoteService::new();

    let status = tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&repo_path);
        git_remote_service.get_sync_status(path)
    })
    .await
    .map_err(|e| {
        tracing::error!("Task join error: {}", e);
        ApiError::InternalServerError
    })?
    .map_err(|e| {
        tracing::error!("Failed to get sync status: {}", e);
        ApiError::from(e)
    })?;

    // MEASURE: Log timing
    let duration_ms = start.elapsed().as_millis();
    tracing::info!(
        "Sync status for project {} took {}ms ({} branches)",
        project_id,
        duration_ms,
        status.branches.len()
    );

    Ok(ResponseJson(ApiResponse::success(
        ProjectSyncStatusResponse {
            project_id,
            current_branch: status.current_branch,
            branches: status.branches,
            response_time_ms: duration_ms as u64,
        },
    )))
}

/// POST /projects/:id/branches/:branch_name/pull
///
/// Pull a specific branch with conflict detection.
/// Supports merge, rebase, or fast-forward strategies.
pub async fn pull_branch(
    Extension(project): Extension<Project>,
    Path((_project_id, branch_name)): Path<(Uuid, String)>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<PullRequest>,
) -> Result<ResponseJson<ApiResponse<PullResult>>, ApiError> {
    tracing::info!(
        "Pulling branch {} for project: {}",
        branch_name,
        project.id
    );

    // Get GitHub token
    let github_token = {
        let config = deployment.config().read().await;
        config.github.token.clone()
    };

    if github_token.is_none() {
        return Ok(ResponseJson(ApiResponse::error(
            "GitHub token not configured. Please authenticate with GitHub first.",
        )));
    }

    let token = github_token.unwrap();
    let repo_path = project.git_repo_path.clone();

    // Pull branch
    let git_remote_service = GitRemoteService::new();
    let strategy = payload.strategy.unwrap_or(PullStrategy::FastForward);

    let result = tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&repo_path);
        git_remote_service.pull_branch(path, &branch_name, &token, strategy)
    })
    .await
    .map_err(|e| {
        tracing::error!("Task join error: {}", e);
        ApiError::InternalServerError
    })?
    .map_err(|e| {
        tracing::error!("Pull failed: {}", e);
        // Return error as JSON response instead of HTTP error
        return ApiError::from(e);
    })?;

    Ok(ResponseJson(ApiResponse::success(result)))
}

// Request/Response Types

#[derive(Debug, Deserialize, TS)]
pub struct PullRequest {
    pub strategy: Option<PullStrategy>,
}

#[derive(Debug, Serialize, TS)]
pub struct FetchTaskResponse {
    pub message: String,
}

#[derive(Debug, Serialize, TS)]
pub struct ProjectSyncStatusResponse {
    pub project_id: String,
    pub current_branch: String,
    pub branches: Vec<BranchSyncStatus>,
    pub response_time_ms: u64, // MEASURE: Include timing in response
}

// Router

pub fn git_remote_routes() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/projects/:id/fetch",
            post(fetch_project).route_layer(from_fn_with_state(
                (),
                load_project_middleware::<DeploymentImpl>,
            )),
        )
        .route(
            "/projects/:id/sync-status",
            get(get_sync_status).route_layer(from_fn_with_state(
                (),
                load_project_middleware::<DeploymentImpl>,
            )),
        )
        .route(
            "/projects/:id/branches/:branch_name/pull",
            post(pull_branch).route_layer(from_fn_with_state(
                (),
                load_project_middleware::<DeploymentImpl>,
            )),
        )
}
