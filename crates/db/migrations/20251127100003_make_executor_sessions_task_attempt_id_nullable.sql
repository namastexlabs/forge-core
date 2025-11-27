-- ============================================================================
-- Make executor_sessions.task_attempt_id nullable for ExecutionRun support
-- ============================================================================
-- ExecutionRun creates executor sessions without TaskAttempt (serverless runs).
-- This migration makes task_attempt_id nullable, following the same pattern
-- as execution_processes (see 20251127100000_create_execution_runs.sql).
--
-- MIGRATION LOCK TIME: ~50ms per 10,000 rows + ~200ms overhead
-- SQLite doesn't support ALTER COLUMN, so we must recreate the table.
-- ============================================================================

PRAGMA foreign_keys = OFF;

-- Recreate executor_sessions with nullable task_attempt_id
CREATE TABLE executor_sessions_new (
    id                    BLOB PRIMARY KEY,
    task_attempt_id       BLOB,              -- NOW NULLABLE (was NOT NULL)
    execution_process_id  BLOB NOT NULL,
    session_id            TEXT,              -- External session ID from Claude/Amp
    prompt                TEXT,              -- The prompt sent to the executor
    summary               TEXT,              -- Final assistant message/summary
    commit_message        TEXT,              -- Generated conventional commit message
    created_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_process_id) REFERENCES execution_processes(id) ON DELETE CASCADE
);

-- Copy existing data (all existing sessions have task_attempt_id)
INSERT INTO executor_sessions_new (
    id, task_attempt_id, execution_process_id, session_id, prompt, summary, commit_message,
    created_at, updated_at
)
SELECT
    id, task_attempt_id, execution_process_id, session_id, prompt, summary, commit_message,
    created_at, updated_at
FROM executor_sessions;

-- Drop old table and rename new one
DROP TABLE executor_sessions;
ALTER TABLE executor_sessions_new RENAME TO executor_sessions;

-- Recreate indexes
CREATE INDEX idx_executor_sessions_task_attempt_id ON executor_sessions(task_attempt_id);
CREATE INDEX idx_executor_sessions_execution_process_id ON executor_sessions(execution_process_id);
CREATE INDEX idx_executor_sessions_session_id ON executor_sessions(session_id);

PRAGMA foreign_keys = ON;
