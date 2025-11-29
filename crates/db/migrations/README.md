# Database Migrations

SQLite migration files for forge-core.

## Migration Order

Migrations run in lexicographic order by filename. Use timestamp prefixes: `YYYYMMDDHHMMSS_description.sql`

## Rollback Procedures

### 20251127100000_create_execution_runs.sql

This migration recreates the `execution_processes` table to make `task_attempt_id` nullable.

**If migration fails mid-execution:**

1. Check current state:
   ```sql
   SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'execution%';
   ```

2. If `execution_processes_new` exists but `execution_processes` is gone:
   ```sql
   ALTER TABLE execution_processes_new RENAME TO execution_processes;
   ```

3. If both tables exist (partial migration):
   ```sql
   DROP TABLE execution_processes_new;
   -- Re-run migration from clean state
   ```

4. To fully revert (removes ExecutionRun support):
   ```sql
   PRAGMA foreign_keys = OFF;

   -- Backup current data
   CREATE TABLE execution_processes_backup AS
     SELECT * FROM execution_processes WHERE task_attempt_id IS NOT NULL;

   -- Drop tables
   DROP TABLE execution_processes;
   DROP TABLE execution_runs;

   -- Recreate original schema
   CREATE TABLE execution_processes (
       id                   BLOB PRIMARY KEY,
       task_attempt_id      BLOB NOT NULL,
       run_reason           TEXT NOT NULL DEFAULT 'codingagent',
       executor_action      TEXT NOT NULL,
       before_head_commit   TEXT,
       after_head_commit    TEXT,
       status               TEXT NOT NULL DEFAULT 'running',
       exit_code            INTEGER,
       dropped              BOOLEAN NOT NULL DEFAULT FALSE,
       started_at           TEXT NOT NULL,
       completed_at         TEXT,
       created_at           TEXT NOT NULL,
       updated_at           TEXT NOT NULL,
       FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE
   );

   -- Restore data (only TaskAttempt-based processes)
   INSERT INTO execution_processes
     SELECT id, task_attempt_id, run_reason, executor_action,
            before_head_commit, after_head_commit, status, exit_code,
            dropped, started_at, completed_at, created_at, updated_at
     FROM execution_processes_backup;

   DROP TABLE execution_processes_backup;

   -- Recreate indexes
   CREATE INDEX idx_execution_processes_task_attempt_id ON execution_processes(task_attempt_id);
   CREATE INDEX idx_execution_processes_status ON execution_processes(status);

   PRAGMA foreign_keys = ON;
   ```

### 20251127100002_add_composite_indexes.sql

**Safe to rollback** - just drop the indexes:
```sql
DROP INDEX IF EXISTS idx_ep_run_created_desc;
DROP INDEX IF EXISTS idx_ep_run_reason_created_desc;
DROP INDEX IF EXISTS idx_ep_attempt_created_desc;
```

## Testing Migrations

Before deploying:
1. Backup production database
2. Test migration on production-sized copy
3. Measure lock time
4. Verify queries use new indexes: `EXPLAIN QUERY PLAN SELECT ...`
