CREATE TABLE IF NOT EXISTS refunds (
    id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    sale_id TEXT NOT NULL REFERENCES sales(id) ON DELETE RESTRICT,
    cashier_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL,
    refund_type TEXT NOT NULL CHECK (refund_type IN ('refund', 'void')),
    total_minor INTEGER NOT NULL CHECK (total_minor > 0),
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (store_id, idempotency_key)
);
CREATE INDEX IF NOT EXISTS idx_refunds_sale_created ON refunds(sale_id, created_at, id);

CREATE TABLE IF NOT EXISTS refund_items (
    id TEXT PRIMARY KEY,
    refund_id TEXT NOT NULL REFERENCES refunds(id) ON DELETE RESTRICT,
    sale_item_id TEXT NOT NULL REFERENCES sale_items(id) ON DELETE RESTRICT,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    subtotal_minor INTEGER NOT NULL CHECK (subtotal_minor >= 0),
    discount_minor INTEGER NOT NULL CHECK (discount_minor >= 0),
    tax_minor INTEGER NOT NULL CHECK (tax_minor >= 0),
    amount_minor INTEGER NOT NULL CHECK (amount_minor >= 0),
    UNIQUE (refund_id, sale_item_id)
);

CREATE TABLE IF NOT EXISTS refund_tenders (
    id TEXT PRIMARY KEY,
    refund_id TEXT NOT NULL REFERENCES refunds(id) ON DELETE RESTRICT,
    method TEXT NOT NULL CHECK (method IN ('cash', 'card', 'mobile_money', 'other')),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    reference TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_refund_tenders_refund ON refund_tenders(refund_id);
