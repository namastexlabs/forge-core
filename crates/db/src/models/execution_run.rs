use chrono::{DateTime, Utc};
use forge_core_executors::executors::BaseCodingAgent;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

use super::project::Project;

#[derive(Debug, Error)]
pub enum ExecutionRunError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Execution run not found")]
    ExecutionRunNotFound,
    #[error("Project not found")]
    ProjectNotFound,
    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Lightweight executor invocation without Task overhead.
/// Used for serverless micro-tasks like generating commit messages, PR descriptions, etc.
/// Reuses 100% of existing executor infrastructure.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct ExecutionRun {
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch: String,
    pub target_branch: String,
    pub executor: String,
    pub container_ref: Option<String>,
    pub prompt: String,
    pub worktree_deleted: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Parameters for creating a new execution run
#[derive(Debug, Deserialize, TS)]
pub struct CreateExecutionRun {
    pub executor: BaseCodingAgent,
    pub base_branch: String,
    pub prompt: String,
}

/// Context data for execution run operations
#[derive(Debug)]
pub struct ExecutionRunContext {
    pub execution_run: ExecutionRun,
    pub project: Project,
}

impl ExecutionRun {
    /// Find execution run by ID
    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ExecutionRun,
            r#"SELECT id AS "id!: Uuid",
                      project_id AS "project_id!: Uuid",
                      branch,
                      target_branch,
                      executor AS "executor!",
                      container_ref,
                      prompt,
                      worktree_deleted AS "worktree_deleted!: bool",
                      created_at AS "created_at!: DateTime<Utc>",
                      updated_at AS "updated_at!: DateTime<Utc>"
               FROM execution_runs
               WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    /// Fetch all execution runs, optionally filtered by project_id. Newest first.
    pub async fn fetch_all(
        pool: &SqlitePool,
        project_id: Option<Uuid>,
    ) -> Result<Vec<Self>, ExecutionRunError> {
        let runs = match project_id {
            Some(pid) => sqlx::query_as!(
                ExecutionRun,
                r#"SELECT id AS "id!: Uuid",
                          project_id AS "project_id!: Uuid",
                          branch,
                          target_branch,
                          executor AS "executor!",
                          container_ref,
                          prompt,
                          worktree_deleted AS "worktree_deleted!: bool",
                          created_at AS "created_at!: DateTime<Utc>",
                          updated_at AS "updated_at!: DateTime<Utc>"
                   FROM execution_runs
                   WHERE project_id = $1
                   ORDER BY created_at DESC"#,
                pid
            )
            .fetch_all(pool)
            .await
            .map_err(ExecutionRunError::Database)?,
            None => sqlx::query_as!(
                ExecutionRun,
                r#"SELECT id AS "id!: Uuid",
                          project_id AS "project_id!: Uuid",
                          branch,
                          target_branch,
                          executor AS "executor!",
                          container_ref,
                          prompt,
                          worktree_deleted AS "worktree_deleted!: bool",
                          created_at AS "created_at!: DateTime<Utc>",
                          updated_at AS "updated_at!: DateTime<Utc>"
                   FROM execution_runs
                   ORDER BY created_at DESC"#
            )
            .fetch_all(pool)
            .await
            .map_err(ExecutionRunError::Database)?,
        };

        Ok(runs)
    }

    /// Load execution run with project context
    pub async fn load_context(
        pool: &SqlitePool,
        run_id: Uuid,
        project_id: Uuid,
    ) -> Result<ExecutionRunContext, ExecutionRunError> {
        let execution_run = sqlx::query_as!(
            ExecutionRun,
            r#"SELECT er.id AS "id!: Uuid",
                      er.project_id AS "project_id!: Uuid",
                      er.branch,
                      er.target_branch,
                      er.executor AS "executor!",
                      er.container_ref,
                      er.prompt,
                      er.worktree_deleted AS "worktree_deleted!: bool",
                      er.created_at AS "created_at!: DateTime<Utc>",
                      er.updated_at AS "updated_at!: DateTime<Utc>"
               FROM execution_runs er
               JOIN projects p ON er.project_id = p.id
               WHERE er.id = $1 AND p.id = $2"#,
            run_id,
            project_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or(ExecutionRunError::ExecutionRunNotFound)?;

        let project = Project::find_by_id(pool, project_id)
            .await?
            .ok_or(ExecutionRunError::ProjectNotFound)?;

        Ok(ExecutionRunContext {
            execution_run,
            project,
        })
    }

    /// Create a new execution run
    pub async fn create(
        pool: &SqlitePool,
        data: &CreateExecutionRun,
        id: Uuid,
        project_id: Uuid,
        branch: &str,
    ) -> Result<Self, ExecutionRunError> {
        Ok(sqlx::query_as!(
            ExecutionRun,
            r#"INSERT INTO execution_runs (id, project_id, branch, target_branch, executor, container_ref, prompt, worktree_deleted)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               RETURNING id AS "id!: Uuid",
                         project_id AS "project_id!: Uuid",
                         branch,
                         target_branch,
                         executor AS "executor!",
                         container_ref,
                         prompt,
                         worktree_deleted AS "worktree_deleted!: bool",
                         created_at AS "created_at!: DateTime<Utc>",
                         updated_at AS "updated_at!: DateTime<Utc>""#,
            id,
            project_id,
            branch,
            data.base_branch,
            data.executor,
            Option::<String>::None,
            data.prompt,
            false
        )
        .fetch_one(pool)
        .await?)
    }

    /// Update container reference
    pub async fn update_container_ref(
        pool: &SqlitePool,
        run_id: Uuid,
        container_ref: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        sqlx::query!(
            "UPDATE execution_runs SET container_ref = $1, updated_at = $2 WHERE id = $3",
            container_ref,
            now,
            run_id
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Mark worktree as deleted
    pub async fn mark_worktree_deleted(pool: &SqlitePool, run_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE execution_runs SET worktree_deleted = TRUE, updated_at = datetime('now', 'subsec') WHERE id = ?",
            run_id
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Check if container_ref exists
    pub async fn container_ref_exists(
        pool: &SqlitePool,
        container_ref: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query!(
            r#"SELECT EXISTS(SELECT 1 FROM execution_runs WHERE container_ref = ?) as "exists!: bool""#,
            container_ref
        )
        .fetch_one(pool)
        .await?;

        Ok(result.exists)
    }

    /// Resolve container_ref to execution run and project IDs
    pub async fn resolve_container_ref(
        pool: &SqlitePool,
        container_ref: &str,
    ) -> Result<(Uuid, Uuid), sqlx::Error> {
        let result = sqlx::query!(
            r#"SELECT er.id AS "run_id!: Uuid",
                      er.project_id AS "project_id!: Uuid"
               FROM execution_runs er
               WHERE er.container_ref = ?"#,
            container_ref
        )
        .fetch_optional(pool)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        Ok((result.run_id, result.project_id))
    }
}
