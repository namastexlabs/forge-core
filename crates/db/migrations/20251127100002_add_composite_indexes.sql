-- Performance indexes for ORDER BY created_at DESC LIMIT 1 queries
-- These cover the hot paths in find_latest_by_* methods
-- Tech Council review: oettam (performance) recommendation

-- For ExecutionRun-based process lookups
-- Covers: find_by_execution_run_id, find_latest_by_execution_run_and_run_reason
CREATE INDEX idx_ep_run_created_desc
  ON execution_processes(execution_run_id, created_at DESC)
  WHERE dropped = FALSE AND execution_run_id IS NOT NULL;

-- For ExecutionRun + run_reason queries (follow-up operations)
-- Covers: find_latest_by_execution_run_and_run_reason with run_reason filter
CREATE INDEX idx_ep_run_reason_created_desc
  ON execution_processes(execution_run_id, run_reason, created_at DESC)
  WHERE dropped = FALSE AND execution_run_id IS NOT NULL;

-- For TaskAttempt-based process lookups (existing hot path)
-- Covers: find_latest_by_task_attempt_and_run_reason
CREATE INDEX idx_ep_attempt_created_desc
  ON execution_processes(task_attempt_id, created_at DESC)
  WHERE dropped = FALSE AND task_attempt_id IS NOT NULL;
