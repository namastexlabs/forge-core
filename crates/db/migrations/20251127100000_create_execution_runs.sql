-- Execution Runs: Lightweight executor invocation without Task overhead
-- Used for serverless micro-tasks like generating commit messages, PR descriptions, etc.
-- Reuses 100% of existing executor infrastructure - same profiles, same streaming, same everything.

PRAGMA foreign_keys = OFF;

-- Create execution_runs table - like task_attempts but references project directly
CREATE TABLE execution_runs (
    id              BLOB PRIMARY KEY,
    project_id      BLOB NOT NULL,
    branch          TEXT NOT NULL,              -- Git branch name for this run
    target_branch   TEXT NOT NULL,              -- Target/base branch
    executor        TEXT NOT NULL,              -- Base coding agent (CLAUDE_CODE, GEMINI, etc.)
    container_ref   TEXT,                       -- Path to worktree or container id
    prompt          TEXT NOT NULL,              -- The prompt/instruction for this run
    worktree_deleted BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

-- Indexes for common queries
CREATE INDEX idx_execution_runs_project_id ON execution_runs(project_id);
CREATE INDEX idx_execution_runs_created_at ON execution_runs(created_at);

-- Recreate execution_processes with nullable task_attempt_id
-- SQLite doesn't support ALTER COLUMN, so we must recreate the table
CREATE TABLE execution_processes_new (
    id                   BLOB PRIMARY KEY,
    task_attempt_id      BLOB,              -- NOW NULLABLE (was NOT NULL)
    execution_run_id     BLOB,              -- NEW: for serverless runs
    run_reason           TEXT NOT NULL DEFAULT 'codingagent'
                            CHECK (run_reason IN ('setupscript','cleanupscript','codingagent','devserver')),
    executor_action      TEXT NOT NULL,
    before_head_commit   TEXT,
    after_head_commit    TEXT,
    status               TEXT NOT NULL DEFAULT 'running'
                            CHECK (status IN ('running','completed','failed','killed')),
    exit_code            INTEGER,
    dropped              BOOLEAN NOT NULL DEFAULT FALSE,
    started_at           TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    completed_at         TEXT,
    created_at           TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at           TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_run_id) REFERENCES execution_runs(id) ON DELETE CASCADE,
    -- At least one reference must be set
    CHECK (task_attempt_id IS NOT NULL OR execution_run_id IS NOT NULL)
);

-- Copy existing data (execution_run_id will be NULL for all existing records)
INSERT INTO execution_processes_new (
    id, task_attempt_id, execution_run_id, run_reason, executor_action,
    before_head_commit, after_head_commit, status, exit_code, dropped,
    started_at, completed_at, created_at, updated_at
)
SELECT
    id, task_attempt_id, NULL, run_reason, executor_action,
    before_head_commit, after_head_commit, status, exit_code, dropped,
    started_at, completed_at, created_at, updated_at
FROM execution_processes;

-- Drop old table and rename new one
DROP TABLE execution_processes;
ALTER TABLE execution_processes_new RENAME TO execution_processes;

-- Recreate indexes
CREATE INDEX idx_execution_processes_task_attempt_id ON execution_processes(task_attempt_id);
CREATE INDEX idx_execution_processes_execution_run_id ON execution_processes(execution_run_id);
CREATE INDEX idx_execution_processes_status ON execution_processes(status);

PRAGMA foreign_keys = ON;
