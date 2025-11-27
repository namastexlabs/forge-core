-- Execution Runs: Lightweight executor invocation without Task overhead
-- Used for serverless micro-tasks like generating commit messages, PR descriptions, etc.
-- Reuses 100% of existing executor infrastructure - same profiles, same streaming, same everything.

PRAGMA foreign_keys = ON;

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

-- Add execution_run_id to execution_processes (nullable - one of task_attempt_id or execution_run_id must be set)
ALTER TABLE execution_processes ADD COLUMN execution_run_id BLOB REFERENCES execution_runs(id) ON DELETE CASCADE;

-- Index for finding processes by execution_run_id
CREATE INDEX idx_execution_processes_execution_run_id ON execution_processes(execution_run_id);
