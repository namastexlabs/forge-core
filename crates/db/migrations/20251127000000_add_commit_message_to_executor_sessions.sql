-- Add commit_message column to executor_sessions table
-- Stores executor-generated conventional commit messages
-- Nullable: existing rows will have NULL, fallback to sanitization
-- Safe: follows same pattern as summary column (20250701120000)
ALTER TABLE executor_sessions ADD COLUMN commit_message TEXT;
