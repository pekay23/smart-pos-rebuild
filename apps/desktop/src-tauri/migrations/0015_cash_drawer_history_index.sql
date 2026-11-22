CREATE INDEX IF NOT EXISTS idx_cash_shifts_closed_history
    ON cash_shifts(store_id, status, closed_at DESC, id DESC);
PRAGMA user_version = 15;
