CREATE TABLE IF NOT EXISTS purchases (
    id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    supplier_id TEXT REFERENCES suppliers(id) ON DELETE SET NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    reference TEXT,
    total_minor INTEGER NOT NULL CHECK (total_minor >= 0),
    received_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_purchases_store_received ON purchases(store_id, received_at DESC);

CREATE TABLE IF NOT EXISTS purchase_items (
    id TEXT PRIMARY KEY,
    purchase_id TEXT NOT NULL REFERENCES purchases(id) ON DELETE CASCADE,
    product_id TEXT NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    sku TEXT NOT NULL,
    product_name TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_cost_minor INTEGER NOT NULL CHECK (unit_cost_minor >= 0),
    line_total_minor INTEGER NOT NULL CHECK (line_total_minor >= 0),
    UNIQUE (purchase_id, product_id)
);

UPDATE roles
SET permissions_json = replace(
    permissions_json,
    '"inventory:write"',
    '"inventory:write","purchases:read","purchases:write"'
)
WHERE role_key IN ('admin', 'manager')
  AND instr(permissions_json, '"purchases:read"') = 0;
