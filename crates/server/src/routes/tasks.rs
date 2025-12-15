use std::{path::PathBuf, sync::Arc};

use anyhow;
use axum::{
    Extension, Json, Router,
    extract::{
        Query, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    middleware::from_fn_with_state,
    response::{IntoResponse, Json as ResponseJson},
    routing::{get, post},
};
use forge_core_db::models::{
    image::TaskImage,
    task::{CreateTask, Task, TaskStatus, TaskWithAttemptStatus, UpdateTask},
    task_attempt::{CreateTaskAttempt, TaskAttempt},
};
use forge_core_deployment::Deployment;
use forge_core_executors::profile::ExecutorProfileId;
use forge_core_services::services::container::{
    ContainerService, WorktreeCleanupData, cleanup_worktrees_direct,
};
use forge_core_utils::response::ApiResponse;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sqlx::Error as SqlxError;
use ts_rs_forge::TS;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError, middleware::load_task_middleware};

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskQuery {
    pub project_id: Uuid,
}

/// Get kanban tasks (excludes agent tasks)
/// Agent tasks are in their own endpoint: /projects/{id}/agents/tasks
pub async fn get_tasks(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<TaskQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<TaskWithAttemptStatus>>>, ApiError> {
    // Kanban endpoint always excludes agent tasks
    // Agent tasks have their own dedicated endpoint
    let tasks = get_kanban_tasks(&deployment.db().pool, query.project_id).await?;
    Ok(ResponseJson(ApiResponse::success(tasks)))
}

/// Get kanban tasks (excludes agent tasks in forge_agents table)
async fn get_kanban_tasks(
    pool: &sqlx::SqlitePool,
    project_id: Uuid,
) -> Result<Vec<TaskWithAttemptStatus>, sqlx::Error> {
    let query_str = r#"SELECT
  t.id                            AS "id",
  t.project_id                    AS "project_id",
  t.title,
  t.description,
  t.status                        AS "status",
  t.parent_task_attempt           AS "parent_task_attempt",
  t.dev_server_id                 AS "dev_server_id",
  t.created_at                    AS "created_at",
  t.updated_at                    AS "updated_at",

  CASE WHEN EXISTS (
    SELECT 1
      FROM task_attempts ta
      JOIN execution_processes ep
        ON ep.task_attempt_id = ta.id
     WHERE ta.task_id       = t.id
       AND ep.status        = 'running'
       AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
     LIMIT 1
  ) THEN 1 ELSE 0 END            AS has_in_progress_attempt,

  CASE WHEN (
    SELECT ep.status
      FROM task_attempts ta
      JOIN execution_processes ep
        ON ep.task_attempt_id = ta.id
     WHERE ta.task_id       = t.id
     AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
     ORDER BY ep.created_at DESC
     LIMIT 1
  ) IN ('failed','killed') THEN 1 ELSE 0 END
                                 AS last_attempt_failed,

  ( SELECT ta.executor
      FROM task_attempts ta
      WHERE ta.task_id = t.id
     ORDER BY ta.created_at DESC
      LIMIT 1
    )                               AS executor,

  ( SELECT COUNT(*)
      FROM task_attempts ta
      WHERE ta.task_id = t.id
    )                               AS attempt_count

FROM tasks t
WHERE t.project_id = ?
  AND t.id NOT IN (SELECT task_id FROM forge_agents)
ORDER BY t.created_at DESC"#;

    let rows = sqlx::query(query_str)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

    let mut items: Vec<TaskWithAttemptStatus> = Vec::with_capacity(rows.len());
    for row in rows {
        use sqlx::Row;

        // Build Task directly from row (eliminates N+1 query)
        let status_str: String = row.try_get("status")?;
        let task = Task {
            id: row.try_get("id")?,
            project_id: row.try_get("project_id")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            status: status_str.parse().unwrap_or(TaskStatus::Todo),
            parent_task_attempt: row.try_get("parent_task_attempt").ok().flatten(),
            dev_server_id: row.try_get("dev_server_id").ok().flatten(),
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        };

        let has_in_progress_attempt = row
            .try_get::<i64, _>("has_in_progress_attempt")
            .map(|v| v != 0)
            .unwrap_or(false);
        let last_attempt_failed = row
            .try_get::<i64, _>("last_attempt_failed")
            .map(|v| v != 0)
            .unwrap_or(false);
        let executor: String = row.try_get("executor").unwrap_or_else(|_| String::new());
        let attempt_count: i64 = row.try_get::<i64, _>("attempt_count").unwrap_or(0);

        items.push(TaskWithAttemptStatus {
            task,
            has_in_progress_attempt,
            has_merged_attempt: false,
            last_attempt_failed,
            executor,
            attempt_count,
        });
    }

    Ok(items)
}

/// WebSocket for kanban tasks (excludes agent tasks)
/// Agent tasks have their own dedicated WebSocket endpoint
pub async fn stream_tasks_ws(
    ws: WebSocketUpgrade,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<TaskQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        // Kanban WebSocket always filters out agent tasks
        let result = handle_kanban_tasks_ws(socket, deployment, query.project_id).await;
        if let Err(e) = result {
            tracing::warn!("kanban tasks WS closed: {}", e);
        }
    })
}

/// Handle kanban WebSocket (excludes agent tasks)
/// Uses a cache with periodic refresh to minimize DB queries
async fn handle_kanban_tasks_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    project_id: Uuid,
) -> anyhow::Result<()> {
    use std::{collections::HashSet, sync::Arc, time::Duration};

    use forge_core_utils::log_msg::LogMsg;
    use serde_json::json;
    use tokio::sync::RwLock;

    let pool = deployment.db().pool.clone();

    // Batch query for all agent task IDs at initialization
    let agent_task_ids: Arc<RwLock<HashSet<Uuid>>> = {
        let agent_tasks: Vec<Uuid> = sqlx::query_scalar(
            "SELECT task_id FROM forge_agents fa
             INNER JOIN tasks t ON fa.task_id = t.id
             WHERE t.project_id = ?",
        )
        .bind(project_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to fetch initial agent task IDs for project {}: {}",
                project_id,
                e
            );
            Vec::new()
        });

        Arc::new(RwLock::new(agent_tasks.into_iter().collect()))
    };

    // Spawn background task to refresh agent task IDs periodically
    let refresh_cache = agent_task_ids.clone();
    let refresh_pool = pool.clone();
    let refresh_project_id = project_id;
    let refresh_task_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;

            match sqlx::query_scalar::<_, Uuid>(
                "SELECT task_id FROM forge_agents fa
                 INNER JOIN tasks t ON fa.task_id = t.id
                 WHERE t.project_id = ?",
            )
            .bind(refresh_project_id)
            .fetch_all(&refresh_pool)
            .await
            {
                Ok(tasks) => {
                    let mut cache = refresh_cache.write().await;
                    cache.clear();
                    cache.extend(tasks);
                    tracing::trace!(
                        "Refreshed agent task cache for project {}: {} tasks",
                        refresh_project_id,
                        cache.len()
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to refresh agent task cache for project {}: {}",
                        refresh_project_id,
                        e
                    );
                }
            }
        }
    });

    // Get the raw stream and filter out agent tasks
    let stream = deployment
        .events()
        .stream_tasks_raw(project_id)
        .await?
        .filter_map(move |msg_result| {
            let agent_task_ids = agent_task_ids.clone();
            let pool = pool.clone();
            async move {
                match msg_result {
                    Ok(LogMsg::JsonPatch(patch)) => {
                        if let Some(patch_op) = patch.0.first() {
                            // Handle direct task patches
                            if patch_op.path().starts_with("/tasks/") {
                                match patch_op {
                                    json_patch::PatchOperation::Add(op) => {
                                        if let Ok(task_with_status) =
                                            serde_json::from_value::<TaskWithAttemptStatus>(
                                                op.value.clone(),
                                            )
                                        {
                                            let task_id = task_with_status.task.id;
                                            // Filter by forge_agents cache OR by task status
                                            // The status check is a backup for race conditions
                                            if is_agent_task(&agent_task_ids, &pool, task_id).await
                                                || task_with_status.task.status == TaskStatus::Agent
                                            {
                                                return None;
                                            }
                                            return Some(Ok(LogMsg::JsonPatch(patch)));
                                        }
                                    }
                                    json_patch::PatchOperation::Replace(op) => {
                                        if let Ok(task_with_status) =
                                            serde_json::from_value::<TaskWithAttemptStatus>(
                                                op.value.clone(),
                                            )
                                        {
                                            let task_id = task_with_status.task.id;
                                            // Filter by forge_agents cache OR by task status
                                            // The status check is a backup for race conditions
                                            if is_agent_task(&agent_task_ids, &pool, task_id).await
                                                || task_with_status.task.status == TaskStatus::Agent
                                            {
                                                return None;
                                            }
                                            return Some(Ok(LogMsg::JsonPatch(patch)));
                                        }
                                    }
                                    json_patch::PatchOperation::Remove(_) => {
                                        return Some(Ok(LogMsg::JsonPatch(patch)));
                                    }
                                    _ => {}
                                }
                            }
                            // Handle initial snapshot
                            else if patch_op.path() == "/tasks"
                                && let json_patch::PatchOperation::Replace(op) = patch_op
                                && let Some(tasks_obj) = op.value.as_object()
                            {
                                let mut filtered_tasks = serde_json::Map::new();
                                for (task_id_str, task_value) in tasks_obj {
                                    if let Ok(task_with_status) =
                                        serde_json::from_value::<TaskWithAttemptStatus>(
                                            task_value.clone(),
                                        )
                                    {
                                        let task_id = task_with_status.task.id;
                                        // Filter by forge_agents cache OR by task status
                                        // The status check is a backup for race conditions
                                        let is_agent =
                                            is_agent_task(&agent_task_ids, &pool, task_id).await
                                                || task_with_status.task.status
                                                    == TaskStatus::Agent;
                                        if !is_agent {
                                            filtered_tasks.insert(
                                                task_id_str.to_string(),
                                                task_value.clone(),
                                            );
                                        }
                                    }
                                }

                                let filtered_patch = json!([{
                                    "op": "replace",
                                    "path": "/tasks",
                                    "value": filtered_tasks
                                }]);
                                return Some(Ok(LogMsg::JsonPatch(
                                    serde_json::from_value(filtered_patch).unwrap(),
                                )));
                            }
                        }
                        Some(Ok(LogMsg::JsonPatch(patch)))
                    }
                    Ok(other) => Some(Ok(other)),
                    Err(e) => Some(Err(e)),
                }
            }
        })
        .map_ok(|msg| msg.to_ws_message_unchecked());

    futures_util::pin_mut!(stream);

    let (mut sender, mut receiver) = socket.split();

    tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    while let Some(item) = stream.next().await {
        match item {
            Ok(msg) => {
                if sender.send(msg).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::error!("stream error: {}", e);
                break;
            }
        }
    }

    refresh_task_handle.abort();

    Ok(())
}

/// Check if a task is an agent task using cache with DB fallback
async fn is_agent_task(
    agent_task_ids: &Arc<tokio::sync::RwLock<std::collections::HashSet<Uuid>>>,
    pool: &sqlx::SqlitePool,
    task_id: Uuid,
) -> bool {
    // Check cache first
    {
        let cache = agent_task_ids.read().await;
        if cache.contains(&task_id) {
            return true;
        }
    }

    // Fallback to DB query for tasks not in cache
    let is_agent_db: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM forge_agents WHERE task_id = ?)")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap_or(false);

    // If it's an agent, update cache
    if is_agent_db {
        let mut cache = agent_task_ids.write().await;
        cache.insert(task_id);
    }

    is_agent_db
}

pub async fn get_task(
    Extension(task): Extension<Task>,
    State(_deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Task>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(task)))
}

pub async fn create_task(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateTask>,
) -> Result<ResponseJson<ApiResponse<Task>>, ApiError> {
    let id = Uuid::new_v4();

    tracing::debug!(
        "Creating task '{}' in project {}",
        payload.title,
        payload.project_id
    );

    let task = Task::create(&deployment.db().pool, &payload, id).await?;

    if let Some(image_ids) = &payload.image_ids {
        TaskImage::associate_many_dedup(&deployment.db().pool, task.id, image_ids).await?;
    }

    deployment
        .track_if_analytics_allowed(
            "task_created",
            serde_json::json!({
            "task_id": task.id.to_string(),
            "project_id": payload.project_id,
            "has_description": task.description.is_some(),
            "has_images": payload.image_ids.is_some(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(task)))
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateAndStartTaskRequest {
    pub task: CreateTask,
    pub executor_profile_id: ExecutorProfileId,
    pub base_branch: String,
    /// Whether to use a git worktree for isolation (default: true)
    pub use_worktree: Option<bool>,
}

pub async fn create_task_and_start(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateAndStartTaskRequest>,
) -> Result<ResponseJson<ApiResponse<TaskWithAttemptStatus>>, ApiError> {
    let task_id = Uuid::new_v4();
    let use_worktree = payload.use_worktree.unwrap_or(true);

    // Set initial status based on use_worktree to avoid race condition with WebSocket broadcasts.
    // Agent tasks (use_worktree: false) must be created with status 'agent' from the start,
    // so the first WebSocket broadcast already has the correct status for filtering.
    let initial_status = if use_worktree {
        TaskStatus::Todo
    } else {
        TaskStatus::Agent
    };
    let task = Task::create_with_status(
        &deployment.db().pool,
        &payload.task,
        task_id,
        initial_status,
    )
    .await?;

    if let Some(image_ids) = &payload.task.image_ids {
        TaskImage::associate_many(&deployment.db().pool, task.id, image_ids).await?;
    }

    // If non-worktree task (e.g., agent chat), register in forge_agents to hide from kanban
    if !use_worktree {
        sqlx::query(
            r#"INSERT INTO forge_agents (id, project_id, agent_type, task_id, created_at, updated_at)
               VALUES (?, ?, 'genie_chat', ?, datetime('now'), datetime('now'))"#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(task.project_id.to_string())
        .bind(task.id.to_string())
        .execute(&deployment.db().pool)
        .await?;
        // Note: Status is already set to 'agent' at task creation time above
    }

    deployment
        .track_if_analytics_allowed(
            "task_created",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "project_id": task.project_id,
                "has_description": task.description.is_some(),
                "has_images": payload.task.image_ids.is_some(),
            }),
        )
        .await;

    // Load and cache workspace-specific .genie profiles (per-workspace, thread-safe)
    // Note: Profiles are cached in ProfileCacheManager per-workspace, NOT in global static cache
    // This avoids race conditions when multiple projects are accessed concurrently
    let project = task
        .parent_project(&deployment.db().pool)
        .await?
        .ok_or(SqlxError::RowNotFound)?;
    if let Ok(_cache) = deployment
        .profile_cache()
        .get_or_create(project.git_repo_path.clone())
        .await
    {
        tracing::debug!(
            "Cached .genie profiles for workspace: {}",
            project.git_repo_path.display()
        );
        deployment
            .profile_cache()
            .register_project(project.id, project.git_repo_path.clone())
            .await;
    }

    let attempt_id = Uuid::new_v4();
    let git_branch_name = deployment
        .container()
        .git_branch_from_task_attempt(&attempt_id, &task.title)
        .await;

    let mut task_attempt = TaskAttempt::create(
        &deployment.db().pool,
        &CreateTaskAttempt {
            executor: payload.executor_profile_id.executor,
            base_branch: payload.base_branch,
            branch: git_branch_name,
        },
        attempt_id,
        task.id,
    )
    .await?;

    // Store executor with variant for filtering (executor:variant format)
    if let Some(variant) = &payload.executor_profile_id.variant {
        let executor_with_variant = format!("{}:{}", payload.executor_profile_id.executor, variant);
        sqlx::query(
            "UPDATE task_attempts SET executor = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&executor_with_variant)
        .bind(attempt_id.to_string())
        .execute(&deployment.db().pool)
        .await?;
        task_attempt.executor = executor_with_variant;
    }

    // Insert worktree config if explicitly specified (defaults to true when not present)
    if let Some(use_worktree) = payload.use_worktree {
        sqlx::query(
            "INSERT INTO forge_task_attempt_config (task_attempt_id, use_worktree) VALUES (?, ?)",
        )
        .bind(attempt_id.to_string())
        .bind(use_worktree)
        .execute(&deployment.db().pool)
        .await?;
    }

    let is_attempt_running = deployment
        .container()
        .start_attempt(&task_attempt, payload.executor_profile_id.clone())
        .await
        .inspect_err(|err| tracing::error!("Failed to start task attempt: {}", err))
        .is_ok();
    deployment
        .track_if_analytics_allowed(
            "task_attempt_started",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "executor": &payload.executor_profile_id.executor,
                "variant": &payload.executor_profile_id.variant,
                "attempt_id": task_attempt.id.to_string(),
            }),
        )
        .await;

    let task = Task::find_by_id(&deployment.db().pool, task.id)
        .await?
        .ok_or(ApiError::Database(SqlxError::RowNotFound))?;

    tracing::info!("Started attempt for task {}", task.id);
    Ok(ResponseJson(ApiResponse::success(TaskWithAttemptStatus {
        task,
        has_in_progress_attempt: is_attempt_running,
        has_merged_attempt: false,
        last_attempt_failed: false,
        executor: task_attempt.executor,
        attempt_count: 1, // First attempt for a newly created task
    })))
}

pub async fn update_task(
    Extension(existing_task): Extension<Task>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<UpdateTask>,
) -> Result<ResponseJson<ApiResponse<Task>>, ApiError> {
    // Use existing values if not provided in update
    let title = payload.title.unwrap_or(existing_task.title);
    let description = match payload.description {
        Some(s) if s.trim().is_empty() => None, // Empty string = clear description
        Some(s) => Some(s),                     // Non-empty string = update description
        None => existing_task.description,      // Field omitted = keep existing
    };
    let status = payload.status.unwrap_or(existing_task.status);
    let parent_task_attempt = payload
        .parent_task_attempt
        .or(existing_task.parent_task_attempt);

    let task = Task::update(
        &deployment.db().pool,
        existing_task.id,
        existing_task.project_id,
        title,
        description,
        status,
        parent_task_attempt,
    )
    .await?;

    if let Some(image_ids) = &payload.image_ids {
        TaskImage::delete_by_task_id(&deployment.db().pool, task.id).await?;
        TaskImage::associate_many_dedup(&deployment.db().pool, task.id, image_ids).await?;
    }

    // Handle archive status transition
    if status == TaskStatus::Archived && existing_task.status != TaskStatus::Archived {
        // Task is being archived for the first time - spawn background cleanup
        handle_task_archive(&deployment, existing_task.id);
    }

    Ok(ResponseJson(ApiResponse::success(task)))
}

pub async fn delete_task(
    Extension(task): Extension<Task>,
    State(deployment): State<DeploymentImpl>,
) -> Result<(StatusCode, ResponseJson<ApiResponse<()>>), ApiError> {
    // Validate no running execution processes
    if deployment
        .container()
        .has_running_processes(task.id)
        .await?
    {
        return Err(ApiError::Conflict("Task has running execution processes. Please wait for them to complete or stop them first.".to_string()));
    }

    // Gather task attempts data needed for background cleanup
    let attempts = TaskAttempt::fetch_all(&deployment.db().pool, Some(task.id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch task attempts for task {}: {}", task.id, e);
            ApiError::TaskAttempt(e)
        })?;

    // Gather cleanup data before deletion
    let project = task
        .parent_project(&deployment.db().pool)
        .await?
        .ok_or_else(|| ApiError::Database(SqlxError::RowNotFound))?;

    let cleanup_data: Vec<WorktreeCleanupData> = attempts
        .iter()
        .filter_map(|attempt| {
            attempt
                .container_ref
                .as_ref()
                .map(|worktree_path| WorktreeCleanupData {
                    attempt_id: attempt.id,
                    worktree_path: PathBuf::from(worktree_path),
                    git_repo_path: Some(project.git_repo_path.clone()),
                })
        })
        .collect();

    // Use a transaction to ensure atomicity: either all operations succeed or all are rolled back
    let mut tx = deployment.db().pool.begin().await?;

    // Nullify parent_task_attempt for all child tasks before deletion
    // This breaks parent-child relationships to avoid foreign key constraint violations
    let mut total_children_affected = 0u64;
    for attempt in &attempts {
        let children_affected = Task::nullify_children_by_attempt_id(&mut *tx, attempt.id).await?;
        total_children_affected += children_affected;
    }

    // Delete task from database (FK CASCADE will handle task_attempts)
    let rows_affected = Task::delete(&mut *tx, task.id).await?;

    if rows_affected == 0 {
        return Err(ApiError::Database(SqlxError::RowNotFound));
    }

    // Commit the transaction - if this fails, all changes are rolled back
    tx.commit().await?;

    if total_children_affected > 0 {
        tracing::info!(
            "Nullified {} child task references before deleting task {}",
            total_children_affected,
            task.id
        );
    }

    deployment
        .track_if_analytics_allowed(
            "task_deleted",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "project_id": task.project_id.to_string(),
                "attempt_count": attempts.len(),
            }),
        )
        .await;

    // Spawn background worktree cleanup task
    let task_id = task.id;
    tokio::spawn(async move {
        let span = tracing::info_span!("background_worktree_cleanup", task_id = %task_id);
        let _enter = span.enter();

        tracing::info!(
            "Starting background cleanup for task {} ({} worktrees)",
            task_id,
            cleanup_data.len()
        );

        if let Err(e) = cleanup_worktrees_direct(&cleanup_data).await {
            tracing::error!(
                "Background worktree cleanup failed for task {}: {}",
                task_id,
                e
            );
        } else {
            tracing::info!("Background cleanup completed for task {}", task_id);
        }
    });

    // Return 202 Accepted to indicate deletion was scheduled
    Ok((StatusCode::ACCEPTED, ResponseJson(ApiResponse::success(()))))
}

/// Handle worktree cleanup when task is archived
fn handle_task_archive(deployment: &DeploymentImpl, task_id: Uuid) {
    let deployment = deployment.clone();
    tokio::spawn(async move {
        let span = tracing::info_span!("archive_task_worktree_cleanup", task_id = %task_id);
        let _enter = span.enter();

        // Fetch task
        let task = match Task::find_by_id(&deployment.db().pool, task_id).await {
            Ok(Some(t)) => t,
            _ => {
                tracing::error!("Failed to find task {} for archive cleanup", task_id);
                return;
            }
        };

        // Fetch all attempts
        let attempts = match TaskAttempt::fetch_all(&deployment.db().pool, Some(task_id)).await {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("Failed to fetch attempts for task {}: {}", task_id, e);
                return;
            }
        };

        // Fetch project for git repo path
        let project = match task.parent_project(&deployment.db().pool).await {
            Ok(Some(p)) => p,
            _ => {
                tracing::error!("Failed to find project for task {}", task_id);
                return;
            }
        };

        // Build cleanup data from attempts
        let cleanup_data: Vec<WorktreeCleanupData> = attempts
            .iter()
            .filter_map(|attempt| {
                attempt
                    .container_ref
                    .as_ref()
                    .map(|worktree_path| WorktreeCleanupData {
                        attempt_id: attempt.id,
                        worktree_path: PathBuf::from(worktree_path),
                        git_repo_path: Some(project.git_repo_path.clone()),
                    })
            })
            .collect();

        if cleanup_data.is_empty() {
            tracing::debug!("No worktrees to cleanup for archived task {}", task_id);
            return;
        }

        tracing::info!(
            "Starting worktree cleanup for archived task {} ({} worktrees)",
            task_id,
            cleanup_data.len()
        );

        // Perform cleanup
        match cleanup_worktrees_direct(&cleanup_data).await {
            Ok(_) => {
                // Mark worktrees as deleted in database
                for attempt in &attempts {
                    if let Err(e) = sqlx::query(
                        "UPDATE task_attempts SET worktree_deleted = TRUE, updated_at = datetime('now') WHERE id = ?"
                    )
                    .bind(attempt.id)
                    .execute(&deployment.db().pool)
                    .await
                    {
                        tracing::error!("Failed to mark worktree_deleted for attempt {}: {}", attempt.id, e);
                    }
                }
                tracing::info!("Completed worktree cleanup for archived task {}", task_id);
            }
            Err(e) => {
                tracing::error!(
                    "Failed to cleanup worktrees for archived task {}: {}",
                    task_id,
                    e
                );
            }
        }
    });
}

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let task_id_router = Router::new()
        .route("/", get(get_task).put(update_task).delete(delete_task))
        .layer(from_fn_with_state(deployment.clone(), load_task_middleware));

    let inner = Router::new()
        .route("/", get(get_tasks).post(create_task))
        .route("/stream/ws", get(stream_tasks_ws))
        .route("/create-and-start", post(create_task_and_start))
        .nest("/{task_id}", task_id_router);

    // mount under /projects/:project_id/tasks
    Router::new().nest("/tasks", inner)
}
