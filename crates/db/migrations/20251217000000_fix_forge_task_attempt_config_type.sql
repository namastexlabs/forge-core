-- Fix type mismatch: task_attempt_id should be BLOB to match task_attempts.id
-- This fixes FK constraint failure (error 787) when inserting into forge_task_attempt_config
-- Upstream pattern: All FK columns that reference BLOB PKs use BLOB type

DROP TABLE IF EXISTS forge_task_attempt_config;

CREATE TABLE IF NOT EXISTS forge_task_attempt_config (
    task_attempt_id BLOB PRIMARY KEY NOT NULL,
    use_worktree BOOLEAN NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_forge_task_attempt_config_task_attempt_id
ON forge_task_attempt_config(task_attempt_id);
