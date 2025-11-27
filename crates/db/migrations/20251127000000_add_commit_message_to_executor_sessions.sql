-- Add commit_message column to executor_sessions table
ALTER TABLE executor_sessions ADD COLUMN commit_message TEXT;
