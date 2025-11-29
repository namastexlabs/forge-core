use axum::{
    Extension, Json, Router,
    extract::{
        Query, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    middleware::from_fn_with_state,
    response::{IntoResponse, Json as ResponseJson},
    routing::{get, post},
};
use db::models::{
    execution_process::{ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus},
    execution_run::{CreateExecutionRun, ExecutionRun},
    project::Project,
};
use deployment::Deployment;
use executors::{
    actions::{
        ExecutorAction, ExecutorActionType,
        coding_agent_follow_up::CodingAgentFollowUpRequest,
        coding_agent_initial::CodingAgentInitialRequest,
    },
    profile::ExecutorProfileId,
};
use serde::{Deserialize, Serialize};
use services::services::container::ContainerService;
use sqlx::Error as SqlxError;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError, middleware::load_execution_run_middleware};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct CreateExecutionRunRequest {
    pub project_id: Uuid,
    pub prompt: String,
    pub executor_profile_id: ExecutorProfileId,
    pub base_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExecutionRunQuery {
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, TS)]
pub struct FollowUpRequest {
    pub prompt: String,
    pub variant: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct ExecutionRunResponse {
    pub execution_run: ExecutionRun,
    pub execution_process: Option<ExecutionProcess>,
}

// ============================================================================
// Route Handlers
// ============================================================================

/// List all execution runs, optionally filtered by project_id
pub async fn list_execution_runs(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ExecutionRunQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<ExecutionRun>>>, ApiError> {
    let runs = ExecutionRun::fetch_all(&deployment.db().pool, query.project_id).await?;
    Ok(ResponseJson(ApiResponse::success(runs)))
}

/// Get a specific execution run by ID
pub async fn get_execution_run(
    Extension(execution_run): Extension<ExecutionRun>,
    State(_deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<ExecutionRun>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(execution_run)))
}

/// Create and start a new execution run
#[axum::debug_handler]
pub async fn create_execution_run(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateExecutionRunRequest>,
) -> Result<ResponseJson<ApiResponse<ExecutionRunResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    // Validate project exists
    let _project = Project::find_by_id(pool, payload.project_id)
        .await?
        .ok_or(SqlxError::RowNotFound)?;

    // Determine base branch (defaults to "main")
    let base_branch = payload.base_branch.unwrap_or_else(|| "main".to_string());

    // Generate branch name for the run
    let run_id = Uuid::new_v4();
    let branch_name = format!("run/{}", &run_id.to_string()[..8]);

    // Create the execution run record
    let create_run = CreateExecutionRun {
        executor: payload.executor_profile_id.executor,
        base_branch: base_branch.clone(),
        prompt: payload.prompt.clone(),
    };

    let execution_run = ExecutionRun::create(pool, &create_run, run_id, payload.project_id, &branch_name).await?;

    // Start the run using container service
    let execution_process = match deployment
        .container()
        .start_run(&execution_run, payload.executor_profile_id.clone())
        .await
    {
        Ok(process) => Some(process),
        Err(e) => {
            tracing::error!("Failed to start execution run {}: {}", run_id, e);
            None
        }
    };

    // Reload execution run to get updated container_ref
    let execution_run = ExecutionRun::find_by_id(pool, run_id)
        .await?
        .ok_or(SqlxError::RowNotFound)?;

    deployment
        .track_if_analytics_allowed(
            "execution_run_started",
            serde_json::json!({
                "run_id": run_id.to_string(),
                "project_id": payload.project_id.to_string(),
                "executor": &payload.executor_profile_id.executor,
                "variant": &payload.executor_profile_id.variant,
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(ExecutionRunResponse {
        execution_run,
        execution_process,
    })))
}

/// Send a follow-up message to an execution run
pub async fn follow_up(
    Extension(execution_run): Extension<ExecutionRun>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<FollowUpRequest>,
) -> Result<ResponseJson<ApiResponse<ExecutionProcess>>, ApiError> {
    let pool = &deployment.db().pool;

    // Get the latest session for this run
    let latest_session_id = ExecutionProcess::find_latest_session_id_by_execution_run(
        pool,
        execution_run.id,
    )
    .await?;

    // Get executor profile from the latest process
    let initial_executor_profile_id = ExecutionProcess::latest_executor_profile_for_run(
        pool,
        execution_run.id,
    )
    .await?;

    let executor_profile_id = ExecutorProfileId {
        executor: initial_executor_profile_id.executor,
        variant: payload.variant.or(initial_executor_profile_id.variant.clone()),
    };

    let action_type = if let Some(session_id) = latest_session_id {
        ExecutorActionType::CodingAgentFollowUpRequest(CodingAgentFollowUpRequest {
            prompt: payload.prompt.clone(),
            session_id,
            executor_profile_id: executor_profile_id.clone(),
        })
    } else {
        // No session exists, start fresh
        ExecutorActionType::CodingAgentInitialRequest(CodingAgentInitialRequest {
            prompt: payload.prompt.clone(),
            executor_profile_id: executor_profile_id.clone(),
        })
    };

    let action = ExecutorAction::new(action_type, None);

    let execution_process = deployment
        .container()
        .start_execution_for_run(
            &execution_run,
            &action,
            &ExecutionProcessRunReason::CodingAgent,
        )
        .await?;

    Ok(ResponseJson(ApiResponse::success(execution_process)))
}

/// Stream logs for an execution run via WebSocket
pub async fn stream_logs_ws(
    ws: WebSocketUpgrade,
    Extension(execution_run): Extension<ExecutionRun>,
    State(deployment): State<DeploymentImpl>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_logs_ws(socket, deployment, execution_run).await {
            tracing::warn!("Execution run logs WS closed: {}", e);
        }
    })
}

async fn handle_logs_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    execution_run: ExecutionRun,
) -> anyhow::Result<()> {
    use futures_util::{SinkExt, StreamExt, TryStreamExt};
    use utils::log_msg::LogMsg;

    let stream = deployment
        .container()
        .stream_raw_logs_for_run(&execution_run.id)
        .await
        .ok_or_else(|| anyhow::anyhow!("No active process for execution run"))?;

    let mut stream = stream.map_ok(|msg: LogMsg| msg.to_ws_message_unchecked());

    let (mut sender, mut receiver) = socket.split();

    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(msg)) => {
                        if sender.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("stream error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            msg = receiver.next() => {
                if msg.is_none() {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Stop an execution run
pub async fn stop_execution_run(
    Extension(execution_run): Extension<ExecutionRun>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let pool = &deployment.db().pool;

    // Find the latest running process for this run
    let process = ExecutionProcess::find_latest_by_execution_run_and_run_reason(
        pool,
        execution_run.id,
        &ExecutionProcessRunReason::CodingAgent,
    )
    .await?;

    if let Some(process) = process {
        deployment
            .container()
            .stop_execution(&process, ExecutionProcessStatus::Killed)
            .await?;
    }

    deployment
        .track_if_analytics_allowed(
            "execution_run_stopped",
            serde_json::json!({
                "run_id": execution_run.id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(())))
}

/// Get execution processes for a run
pub async fn get_execution_run_processes(
    Extension(execution_run): Extension<ExecutionRun>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<ExecutionProcess>>>, ApiError> {
    let processes = ExecutionProcess::find_by_execution_run_id(
        &deployment.db().pool,
        execution_run.id,
        false, // don't show soft-deleted
    )
    .await?;

    Ok(ResponseJson(ApiResponse::success(processes)))
}

// ============================================================================
// Router
// ============================================================================

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let execution_run_id_router = Router::new()
        .route("/", get(get_execution_run))
        .route("/follow-up", post(follow_up))
        .route("/logs/ws", get(stream_logs_ws))
        .route("/stop", post(stop_execution_run))
        .route("/processes", get(get_execution_run_processes))
        .layer(from_fn_with_state(
            deployment.clone(),
            load_execution_run_middleware,
        ));

    let execution_runs_router = Router::new()
        .route("/", get(list_execution_runs).post(create_execution_run))
        .nest("/{id}", execution_run_id_router);

    Router::new().nest("/execution-runs", execution_runs_router)
}
