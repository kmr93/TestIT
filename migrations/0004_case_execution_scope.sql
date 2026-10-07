ALTER TABLE case_runs
ADD COLUMN execution_scope TEXT NOT NULL DEFAULT 'case'
CHECK (execution_scope IN ('case', 'suite_setup', 'suite_cleanup'));
