-- ============================================================================
-- ExecutionRun Model + Commit Message Support
-- ============================================================================
-- This migration introduces:
-- 1. execution_runs table - lightweight executor invocations without Task overhead
-- 2. execution_run_id FK in execution_processes (makes task_attempt_id nullable)
-- 3. commit_message column in executor_sessions (makes task_attempt_id nullable)
-- 4. commit_prompt column in projects
-- 5. Composite indexes for query performance
--
-- WARNING: MIGRATION LOCK TIME
-- SQLite acquires exclusive write lock during table rebuilds.
-- Expected: ~50ms per 10,000 rows + ~200ms overhead per table
-- ============================================================================

PRAGMA foreign_keys = OFF;

-- ============================================================================
-- 1. Create execution_runs table
-- ============================================================================
-- Lightweight executor invocation without Task/TaskAttempt overhead.
-- Used for serverless micro-tasks: commit messages, PR descriptions, quick refactors.

CREATE TABLE execution_runs (
    id              BLOB PRIMARY KEY,
    project_id      BLOB NOT NULL,
    branch          TEXT NOT NULL,              -- Git branch for this run
    target_branch   TEXT NOT NULL,              -- Target/base branch
    executor        TEXT NOT NULL,              -- CLAUDE_CODE, GEMINI, etc.
    container_ref   TEXT,                       -- Worktree path or container id
    prompt          TEXT NOT NULL,              -- Instruction for this run
    worktree_deleted BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX idx_execution_runs_project_id ON execution_runs(project_id);
CREATE INDEX idx_execution_runs_created_at ON execution_runs(created_at);

-- ============================================================================
-- 2. Rebuild execution_processes (nullable task_attempt_id + execution_run_id FK)
-- ============================================================================

CREATE TABLE execution_processes_new (
    id                   BLOB PRIMARY KEY,
    task_attempt_id      BLOB,              -- NOW NULLABLE
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
    CHECK (task_attempt_id IS NOT NULL OR execution_run_id IS NOT NULL)
);

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

DROP TABLE execution_processes;
ALTER TABLE execution_processes_new RENAME TO execution_processes;

-- Standard indexes
CREATE INDEX idx_execution_processes_task_attempt_id ON execution_processes(task_attempt_id);
CREATE INDEX idx_execution_processes_execution_run_id ON execution_processes(execution_run_id);
CREATE INDEX idx_execution_processes_status ON execution_processes(status);

-- Composite indexes for ORDER BY created_at DESC LIMIT 1 queries (Tech Council recommendation)
CREATE INDEX idx_ep_run_created_desc
  ON execution_processes(execution_run_id, created_at DESC)
  WHERE dropped = FALSE AND execution_run_id IS NOT NULL;

CREATE INDEX idx_ep_run_reason_created_desc
  ON execution_processes(execution_run_id, run_reason, created_at DESC)
  WHERE dropped = FALSE AND execution_run_id IS NOT NULL;

CREATE INDEX idx_ep_attempt_created_desc
  ON execution_processes(task_attempt_id, created_at DESC)
  WHERE dropped = FALSE AND task_attempt_id IS NOT NULL;

-- ============================================================================
-- 3. Rebuild executor_sessions (nullable task_attempt_id + commit_message)
-- ============================================================================

CREATE TABLE executor_sessions_new (
    id                    BLOB PRIMARY KEY,
    task_attempt_id       BLOB,              -- NOW NULLABLE (for ExecutionRun)
    execution_process_id  BLOB NOT NULL,
    session_id            TEXT,
    prompt                TEXT,
    summary               TEXT,
    commit_message        TEXT,              -- NEW: executor-generated commit message
    created_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_process_id) REFERENCES execution_processes(id) ON DELETE CASCADE
);

INSERT INTO executor_sessions_new (
    id, task_attempt_id, execution_process_id, session_id, prompt, summary,
    commit_message, created_at, updated_at
)
SELECT
    id, task_attempt_id, execution_process_id, session_id, prompt, summary,
    NULL, created_at, updated_at
FROM executor_sessions;

DROP TABLE executor_sessions;
ALTER TABLE executor_sessions_new RENAME TO executor_sessions;

CREATE INDEX idx_executor_sessions_task_attempt_id ON executor_sessions(task_attempt_id);
CREATE INDEX idx_executor_sessions_execution_process_id ON executor_sessions(execution_process_id);
CREATE INDEX idx_executor_sessions_session_id ON executor_sessions(session_id);

-- ============================================================================
-- 4. Add commit_prompt to projects (simple ALTER)
-- ============================================================================

ALTER TABLE projects ADD COLUMN commit_prompt TEXT;

PRAGMA foreign_keys = ON;
