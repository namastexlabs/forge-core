-- ============================================================================
-- Add variant column to execution_runs table
-- ============================================================================
-- Aligns ExecutionRuns with Tasks for profile selection.
-- The variant field stores the profile variant name (e.g., "GENIE", "PLAN", "APPROVALS")
-- This enables ExecutionRuns to use .genie profiles the same way Tasks do.
--
-- Related: PR council review finding - "ExecutionRuns missing profile injection"
-- ============================================================================

-- Add variant column (nullable for backwards compatibility with existing runs)
ALTER TABLE execution_runs ADD COLUMN variant TEXT;

-- Index for filtering by executor+variant combination
CREATE INDEX idx_execution_runs_executor_variant ON execution_runs(executor, variant);
