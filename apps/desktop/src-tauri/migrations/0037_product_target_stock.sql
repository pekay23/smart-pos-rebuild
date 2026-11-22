BEGIN IMMEDIATE;

ALTER TABLE products
    ADD COLUMN target_stock_quantity INTEGER NOT NULL DEFAULT 0
    CHECK (target_stock_quantity >= 0 AND target_stock_quantity <= 1000000);

CREATE INDEX idx_products_reorder_queue
    ON products(store_id, is_active, reorder_point, target_stock_quantity, stock_quantity);

INSERT INTO local_schema_migrations(version) VALUES (37);
COMMIT;
