BEGIN IMMEDIATE;

CREATE TABLE purchase_orders (
    id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    supplier_id TEXT REFERENCES suppliers(id) ON DELETE RESTRICT,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    order_number TEXT NOT NULL,
    reference TEXT,
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft','placed','partially_received','received','cancelled')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    placed_at TEXT,
    cancelled_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (store_id, order_number)
);
CREATE INDEX idx_purchase_orders_store_status ON purchase_orders(store_id, status, updated_at DESC);

CREATE TABLE purchase_order_lines (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES purchase_orders(id) ON DELETE CASCADE,
    product_id TEXT NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    product_name TEXT NOT NULL,
    sku TEXT NOT NULL,
    ordered_quantity INTEGER NOT NULL CHECK (ordered_quantity > 0 AND ordered_quantity <= 1000000),
    received_quantity INTEGER NOT NULL DEFAULT 0 CHECK (received_quantity >= 0),
    expected_unit_cost_minor INTEGER NOT NULL CHECK (expected_unit_cost_minor >= 0),
    UNIQUE (order_id, product_id)
);
CREATE INDEX idx_purchase_order_lines_product ON purchase_order_lines(product_id, order_id);

CREATE TRIGGER purchase_orders_store_guard_insert
BEFORE INSERT ON purchase_orders
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM stores s JOIN users u ON u.id=NEW.created_by AND u.business_id=s.business_id
        WHERE s.id=NEW.store_id
    ) THEN RAISE(ABORT, 'purchase order user and store must belong to the same business') END;
    SELECT CASE WHEN NEW.supplier_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM suppliers s WHERE s.id=NEW.supplier_id AND s.store_id=NEW.store_id
    ) THEN RAISE(ABORT, 'purchase order supplier must belong to the same store') END;
END;

CREATE TRIGGER purchase_orders_store_guard_update
BEFORE UPDATE OF store_id,supplier_id ON purchase_orders
BEGIN
    SELECT CASE WHEN NEW.supplier_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM suppliers s WHERE s.id=NEW.supplier_id AND s.store_id=NEW.store_id
    ) THEN RAISE(ABORT, 'purchase order supplier must belong to the same store') END;
END;

CREATE TRIGGER purchase_order_lines_store_guard_insert
BEFORE INSERT ON purchase_order_lines
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM purchase_orders po JOIN products p ON p.id=NEW.product_id
        WHERE po.id=NEW.order_id AND po.store_id=p.store_id
    ) THEN RAISE(ABORT, 'purchase order product must belong to the same store') END;
END;

CREATE TRIGGER purchase_order_lines_store_guard_update
BEFORE UPDATE OF order_id,product_id ON purchase_order_lines
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM purchase_orders po JOIN products p ON p.id=NEW.product_id
        WHERE po.id=NEW.order_id AND po.store_id=p.store_id
    ) THEN RAISE(ABORT, 'purchase order product must belong to the same store') END;
END;

CREATE TRIGGER purchase_order_lines_received_quantity_guard
BEFORE UPDATE OF received_quantity ON purchase_order_lines
BEGIN
    SELECT CASE WHEN NEW.received_quantity > NEW.ordered_quantity
        THEN RAISE(ABORT, 'received quantity cannot exceed ordered quantity') END;
END;

ALTER TABLE purchases ADD COLUMN purchase_order_id TEXT REFERENCES purchase_orders(id) ON DELETE RESTRICT;
ALTER TABLE purchases ADD COLUMN idempotency_key TEXT;
ALTER TABLE purchase_items ADD COLUMN purchase_order_line_id TEXT REFERENCES purchase_order_lines(id) ON DELETE RESTRICT;
CREATE INDEX idx_purchases_order ON purchases(purchase_order_id);
CREATE UNIQUE INDEX idx_purchases_store_idempotency ON purchases(store_id,idempotency_key);
CREATE INDEX idx_purchase_items_order_line ON purchase_items(purchase_order_line_id);

CREATE TRIGGER purchases_order_store_guard_insert
BEFORE INSERT ON purchases WHEN NEW.purchase_order_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM purchase_orders po
        WHERE po.id=NEW.purchase_order_id AND po.store_id=NEW.store_id
          AND po.status IN ('placed','partially_received')
          AND (po.supplier_id IS NULL OR po.supplier_id=NEW.supplier_id)
    ) THEN RAISE(ABORT, 'receipt must match an open purchase order in the same store') END;
END;

CREATE TRIGGER purchase_items_order_line_guard_insert
BEFORE INSERT ON purchase_items
BEGIN
    SELECT CASE WHEN NEW.purchase_order_line_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM purchases p JOIN purchase_order_lines pol ON pol.id=NEW.purchase_order_line_id
        WHERE p.id=NEW.purchase_id AND p.purchase_order_id=pol.order_id AND pol.product_id=NEW.product_id
    ) THEN RAISE(ABORT, 'receipt item must match its linked order line') END;
    SELECT CASE WHEN NEW.purchase_order_line_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM purchase_order_lines pol
        WHERE pol.id=NEW.purchase_order_line_id
          AND pol.received_quantity+NEW.quantity>pol.ordered_quantity
    ) THEN RAISE(ABORT, 'receipt quantity exceeds the order line balance') END;
    SELECT CASE WHEN NEW.purchase_order_line_id IS NULL AND EXISTS (
        SELECT 1 FROM purchases p WHERE p.id=NEW.purchase_id AND p.purchase_order_id IS NOT NULL
    ) THEN RAISE(ABORT, 'all linked receipt items must reference order lines') END;
END;

INSERT INTO local_schema_migrations(version) VALUES (38);
COMMIT;
