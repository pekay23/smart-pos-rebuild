CREATE INDEX IF NOT EXISTS idx_expenses_store_occurred_created_id
    ON expenses(store_id, occurred_at DESC, created_at DESC, id DESC);
PRAGMA user_version = 14;
