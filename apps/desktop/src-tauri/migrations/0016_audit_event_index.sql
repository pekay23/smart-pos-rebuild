CREATE INDEX IF NOT EXISTS idx_audit_events_store_created
    ON audit_events(business_id, store_id, created_at DESC, id DESC);
