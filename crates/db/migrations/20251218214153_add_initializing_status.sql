-- ============================================================================
-- Add 'initializing' status to execution_processes
-- ============================================================================
-- This migration adds the 'initializing' status to track early startup lifecycle.
-- ExecutionProcess is created with 'initializing' before container/worktree setup,
-- then transitions to 'running' on success or 'failed' on error.
--
-- This enables clients to see startup began via SSE, even if startup fails
-- before the executor actually starts running.
-- ============================================================================

PRAGMA foreign_keys = OFF;

-- Rebuild execution_processes with updated status CHECK constraint
CREATE TABLE execution_processes_new (
    id                   BLOB PRIMARY KEY,
    task_attempt_id      BLOB,
    execution_run_id     BLOB,
    run_reason           TEXT NOT NULL DEFAULT 'codingagent'
                            CHECK (run_reason IN ('setupscript','cleanupscript','codingagent','devserver')),
    executor_action      TEXT NOT NULL,
    before_head_commit   TEXT,
    after_head_commit    TEXT,
    status               TEXT NOT NULL DEFAULT 'running'
                            CHECK (status IN ('initializing','running','completed','failed','killed')),
    exit_code            INTEGER,
    dropped              BOOLEAN NOT NULL DEFAULT FALSE,
    started_at           TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    completed_at         TEXT,
    created_at           TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at           TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_run_id) REFERENCES execution_runs(id) ON DELETE CASCADE,
    CHECK (task_attempt_id IS NOT NULL OR execution_run_id IS NOT NULL)
);

INSERT INTO execution_processes_new (
    id, task_attempt_id, execution_run_id, run_reason, executor_action,
    before_head_commit, after_head_commit, status, exit_code, dropped,
    started_at, completed_at, created_at, updated_at
)
SELECT
    id, task_attempt_id, execution_run_id, run_reason, executor_action,
    before_head_commit, after_head_commit, status, exit_code, dropped,
    started_at, completed_at, created_at, updated_at
FROM execution_processes;

DROP TABLE execution_processes;
ALTER TABLE execution_processes_new RENAME TO execution_processes;

-- Recreate indexes
CREATE INDEX idx_execution_processes_task_attempt_id ON execution_processes(task_attempt_id);
CREATE INDEX idx_execution_processes_execution_run_id ON execution_processes(execution_run_id);
CREATE INDEX idx_execution_processes_status ON execution_processes(status);

-- Composite indexes for ORDER BY created_at DESC LIMIT 1 queries
CREATE INDEX idx_ep_run_created_desc
  ON execution_processes(execution_run_id, created_at DESC)
  WHERE dropped = FALSE AND execution_run_id IS NOT NULL;

CREATE INDEX idx_ep_run_reason_created_desc
  ON execution_processes(execution_run_id, run_reason, created_at DESC)
  WHERE dropped = FALSE AND execution_run_id IS NOT NULL;

CREATE INDEX idx_ep_attempt_created_desc
  ON execution_processes(task_attempt_id, created_at DESC)
  WHERE dropped = FALSE AND task_attempt_id IS NOT NULL;

PRAGMA foreign_keys = ON;
