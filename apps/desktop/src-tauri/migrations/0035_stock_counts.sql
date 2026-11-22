BEGIN IMMEDIATE;

CREATE TABLE stock_counts (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    status TEXT NOT NULL CHECK (status IN ('draft','posted')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    started_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    started_at TEXT NOT NULL,
    posted_at TEXT
);

CREATE INDEX idx_stock_counts_store_started ON stock_counts(store_id, started_at DESC);

CREATE TABLE stock_count_lines (
    count_id TEXT NOT NULL REFERENCES stock_counts(id) ON DELETE CASCADE,
    product_id TEXT NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    expected_quantity INTEGER NOT NULL CHECK (expected_quantity >= 0),
    counted_quantity INTEGER CHECK (counted_quantity IS NULL OR counted_quantity >= 0),
    movement_cursor INTEGER CHECK (movement_cursor IS NULL OR movement_cursor >= 0),
    counted_at TEXT,
    PRIMARY KEY (count_id, product_id),
    CHECK ((counted_quantity IS NULL AND movement_cursor IS NULL AND counted_at IS NULL)
        OR (counted_quantity IS NOT NULL AND movement_cursor IS NOT NULL AND counted_at IS NOT NULL))
);

CREATE INDEX idx_stock_count_lines_product ON stock_count_lines(product_id, count_id);

CREATE TABLE stock_movement_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    movement_id TEXT NOT NULL UNIQUE REFERENCES stock_movements(id) ON DELETE CASCADE,
    store_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    quantity_delta INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_stock_movement_events_product_sequence
    ON stock_movement_events(store_id, product_id, sequence);

INSERT INTO stock_movement_events(movement_id,store_id,product_id,quantity_delta,created_at)
SELECT id,store_id,product_id,quantity_delta,created_at
FROM stock_movements
ORDER BY created_at,rowid;

CREATE TRIGGER stock_movement_event_after_insert
AFTER INSERT ON stock_movements
BEGIN
    INSERT INTO stock_movement_events(movement_id,store_id,product_id,quantity_delta,created_at)
    VALUES(NEW.id,NEW.store_id,NEW.product_id,NEW.quantity_delta,NEW.created_at);
END;

INSERT INTO local_schema_migrations(version) VALUES (35);
COMMIT;
