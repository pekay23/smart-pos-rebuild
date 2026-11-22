CREATE TABLE IF NOT EXISTS store_preferences (
    store_id TEXT PRIMARY KEY REFERENCES stores(id) ON DELETE CASCADE,
    cashier_start_at_register INTEGER NOT NULL DEFAULT 1 CHECK (cashier_start_at_register IN (0, 1)),
    cashier_auto_fullscreen INTEGER NOT NULL DEFAULT 1 CHECK (cashier_auto_fullscreen IN (0, 1)),
    customer_display_enabled INTEGER NOT NULL DEFAULT 0 CHECK (customer_display_enabled IN (0, 1)),
    customer_display_fullscreen INTEGER NOT NULL DEFAULT 0 CHECK (customer_display_fullscreen IN (0, 1)),
    customer_display_monitor_key TEXT,
    customer_display_idle_message TEXT NOT NULL DEFAULT 'Welcome to our store.' CHECK (length(customer_display_idle_message) <= 160),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT OR IGNORE INTO store_preferences (store_id)
SELECT id FROM stores;
