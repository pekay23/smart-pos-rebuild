CREATE TABLE IF NOT EXISTS cash_movements (
    id TEXT PRIMARY KEY,
    shift_id TEXT NOT NULL REFERENCES cash_shifts(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    movement_type TEXT NOT NULL CHECK (movement_type IN ('paid_in', 'paid_out', 'cash_drop')),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    note TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cash_movements_shift_created ON cash_movements(shift_id, created_at, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_shifts_one_open_per_station
    ON cash_shifts(store_id, station_key) WHERE status = 'open';

CREATE TABLE IF NOT EXISTS cash_close_reports (
    id TEXT PRIMARY KEY,
    shift_id TEXT NOT NULL UNIQUE REFERENCES cash_shifts(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    closed_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    close_type TEXT NOT NULL CHECK (close_type = 'z'),
    expected_cash_minor INTEGER NOT NULL,
    counted_cash_minor INTEGER NOT NULL,
    difference_minor INTEGER NOT NULL,
    report_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cash_close_reports_store_created ON cash_close_reports(store_id, created_at DESC);
