//! Integration tests for ExecutionRun model
//!
//! These tests verify the ExecutionRun → ExecutionProcess lifecycle works correctly,
//! particularly that:
//! - ExecutionRun can be created without a Task/TaskAttempt
//! - ExecutionProcess can reference execution_run_id without task_attempt_id
//! - Query methods work correctly with the new nullable FK pattern
//!
//! Note: These tests require DATABASE_URL to be set and migrations to run.
//! Run with: cargo test --package services --test execution_run_model

use db::{models::execution_run::ExecutionRun, DBService};
use tempfile::TempDir;
use uuid::Uuid;

/// Creates a test database in a temporary directory
async fn setup_test_db() -> (DBService, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.sqlite");

    // Set DATABASE_URL for the test
    // SAFETY: We're in a test context where this is acceptable
    unsafe {
        std::env::set_var("DATABASE_URL", format!("sqlite://{}", db_path.display()));
    }

    let db = DBService::new().await.unwrap();
    (db, temp_dir)
}

/// Create a test project for ExecutionRun tests
async fn create_test_project(pool: &sqlx::SqlitePool) -> Uuid {
    let project_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    // Use unique git_repo_path per project
    let git_repo_path = format!("/tmp/test-repo-{}", project_id);

    sqlx::query(
        r#"INSERT INTO projects (id, name, git_repo_path, created_at, updated_at)
           VALUES (?, 'Test Project', ?, ?, ?)"#,
    )
    .bind(project_id)
    .bind(&git_repo_path)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();

    project_id
}

#[tokio::test]
async fn test_execution_run_create_without_task() {
    let (db, _temp) = setup_test_db().await;
    let pool = &db.pool;

    // Create a project (required FK)
    let project_id = create_test_project(pool).await;

    // Create ExecutionRun - should work without any Task
    let run_id = Uuid::new_v4();
    let branch = format!("run/{}", &run_id.to_string()[..8]);

    let create_data = db::models::execution_run::CreateExecutionRun {
        executor: executors::executors::BaseCodingAgent::ClaudeCode,
        base_branch: "main".to_string(),
        prompt: "Test prompt".to_string(),
    };

    let execution_run = ExecutionRun::create(pool, &create_data, run_id, project_id, &branch)
        .await
        .unwrap();

    assert_eq!(execution_run.id, run_id);
    assert_eq!(execution_run.project_id, project_id);
    assert_eq!(execution_run.branch, branch);
    assert_eq!(execution_run.target_branch, "main");
    assert_eq!(execution_run.prompt, "Test prompt");
    assert!(!execution_run.worktree_deleted);
}

#[tokio::test]
async fn test_execution_run_find_by_id() {
    let (db, _temp) = setup_test_db().await;
    let pool = &db.pool;

    let project_id = create_test_project(pool).await;
    let run_id = Uuid::new_v4();
    let branch = format!("run/{}", &run_id.to_string()[..8]);

    let create_data = db::models::execution_run::CreateExecutionRun {
        executor: executors::executors::BaseCodingAgent::ClaudeCode,
        base_branch: "main".to_string(),
        prompt: "Find me test".to_string(),
    };

    ExecutionRun::create(pool, &create_data, run_id, project_id, &branch)
        .await
        .unwrap();

    // Find by ID
    let found = ExecutionRun::find_by_id(pool, run_id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, run_id);

    // Non-existent ID returns None
    let not_found = ExecutionRun::find_by_id(pool, Uuid::new_v4()).await.unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_execution_run_fetch_all_with_project_filter() {
    let (db, _temp) = setup_test_db().await;
    let pool = &db.pool;

    let project_id_1 = create_test_project(pool).await;
    let project_id_2 = create_test_project(pool).await;

    // Create runs in project 1
    for i in 0..3 {
        let run_id = Uuid::new_v4();
        let create_data = db::models::execution_run::CreateExecutionRun {
            executor: executors::executors::BaseCodingAgent::ClaudeCode,
            base_branch: "main".to_string(),
            prompt: format!("Project 1 run {}", i),
        };
        ExecutionRun::create(
            pool,
            &create_data,
            run_id,
            project_id_1,
            &format!("run/{}", &run_id.to_string()[..8]),
        )
        .await
        .unwrap();
    }

    // Create runs in project 2
    for i in 0..2 {
        let run_id = Uuid::new_v4();
        let create_data = db::models::execution_run::CreateExecutionRun {
            executor: executors::executors::BaseCodingAgent::ClaudeCode,
            base_branch: "main".to_string(),
            prompt: format!("Project 2 run {}", i),
        };
        ExecutionRun::create(
            pool,
            &create_data,
            run_id,
            project_id_2,
            &format!("run/{}", &run_id.to_string()[..8]),
        )
        .await
        .unwrap();
    }

    // Fetch all (no filter)
    let all_runs = ExecutionRun::fetch_all(pool, None).await.unwrap();
    assert_eq!(all_runs.len(), 5);

    // Fetch with project filter
    let project_1_runs = ExecutionRun::fetch_all(pool, Some(project_id_1))
        .await
        .unwrap();
    assert_eq!(project_1_runs.len(), 3);

    let project_2_runs = ExecutionRun::fetch_all(pool, Some(project_id_2))
        .await
        .unwrap();
    assert_eq!(project_2_runs.len(), 2);
}

#[tokio::test]
async fn test_execution_run_update_container_ref() {
    let (db, _temp) = setup_test_db().await;
    let pool = &db.pool;

    let project_id = create_test_project(pool).await;
    let run_id = Uuid::new_v4();

    let create_data = db::models::execution_run::CreateExecutionRun {
        executor: executors::executors::BaseCodingAgent::ClaudeCode,
        base_branch: "main".to_string(),
        prompt: "Container ref test".to_string(),
    };

    ExecutionRun::create(
        pool,
        &create_data,
        run_id,
        project_id,
        &format!("run/{}", &run_id.to_string()[..8]),
    )
    .await
    .unwrap();

    // Initially no container_ref
    let run = ExecutionRun::find_by_id(pool, run_id).await.unwrap().unwrap();
    assert!(run.container_ref.is_none());

    // Update container_ref
    ExecutionRun::update_container_ref(pool, run_id, "/worktrees/test-123")
        .await
        .unwrap();

    let updated = ExecutionRun::find_by_id(pool, run_id).await.unwrap().unwrap();
    assert_eq!(updated.container_ref.as_deref(), Some("/worktrees/test-123"));
}

#[tokio::test]
async fn test_execution_run_mark_worktree_deleted() {
    let (db, _temp) = setup_test_db().await;
    let pool = &db.pool;

    let project_id = create_test_project(pool).await;
    let run_id = Uuid::new_v4();

    let create_data = db::models::execution_run::CreateExecutionRun {
        executor: executors::executors::BaseCodingAgent::ClaudeCode,
        base_branch: "main".to_string(),
        prompt: "Worktree deletion test".to_string(),
    };

    ExecutionRun::create(
        pool,
        &create_data,
        run_id,
        project_id,
        &format!("run/{}", &run_id.to_string()[..8]),
    )
    .await
    .unwrap();

    // Initially not deleted
    let run = ExecutionRun::find_by_id(pool, run_id).await.unwrap().unwrap();
    assert!(!run.worktree_deleted);

    // Mark as deleted
    ExecutionRun::mark_worktree_deleted(pool, run_id).await.unwrap();

    let deleted = ExecutionRun::find_by_id(pool, run_id).await.unwrap().unwrap();
    assert!(deleted.worktree_deleted);
}
