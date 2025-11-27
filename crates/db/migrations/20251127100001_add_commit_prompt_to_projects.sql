-- Add commit_prompt column to projects table
-- This allows customizing the prompt template used for generating commit messages for this project

ALTER TABLE projects ADD COLUMN commit_prompt TEXT;
