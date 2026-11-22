CREATE TABLE IF NOT EXISTS payment_integration_requests (
    id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    requested_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    country TEXT NOT NULL,
    provider_name TEXT NOT NULL,
    terminal_model TEXT NOT NULL DEFAULT '',
    capability TEXT NOT NULL,
    details TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'local_only'
        CHECK (status IN ('local_only', 'queued', 'acknowledged', 'closed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS payment_integration_requests_store_created
    ON payment_integration_requests(store_id, created_at DESC);
