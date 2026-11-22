CREATE TABLE IF NOT EXISTS staff_switch_attempts (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    failed_attempts INTEGER NOT NULL DEFAULT 0 CHECK (failed_attempts >= 0),
    locked_until TEXT
);

CREATE TABLE IF NOT EXISTS register_workspaces (
    store_id TEXT PRIMARY KEY REFERENCES stores(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    workspace_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_sales_source_ticket
    ON sales(source_ticket_id) WHERE source_ticket_id IS NOT NULL;
