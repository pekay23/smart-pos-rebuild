BEGIN IMMEDIATE;

CREATE TABLE entity_images (
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL CHECK (entity_type IN ('product','customer','supplier')),
    entity_id TEXT NOT NULL,
    image_data BLOB NOT NULL CHECK (length(image_data) BETWEEN 1 AND 900000),
    thumbnail_data BLOB NOT NULL CHECK (length(thumbnail_data) BETWEEN 1 AND 80000),
    width INTEGER NOT NULL CHECK (width BETWEEN 1 AND 960),
    height INTEGER NOT NULL CHECK (height BETWEEN 1 AND 960),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (store_id, entity_type, entity_id)
);

CREATE INDEX idx_entity_images_store_type ON entity_images(store_id, entity_type);

CREATE TRIGGER entity_image_product_cleanup AFTER DELETE ON products
BEGIN
    DELETE FROM entity_images WHERE store_id=OLD.store_id AND entity_type='product' AND entity_id=OLD.id;
END;

CREATE TRIGGER entity_image_customer_cleanup AFTER DELETE ON customers
BEGIN
    DELETE FROM entity_images WHERE store_id=OLD.store_id AND entity_type='customer' AND entity_id=OLD.id;
END;

CREATE TRIGGER entity_image_supplier_cleanup AFTER DELETE ON suppliers
BEGIN
    DELETE FROM entity_images WHERE store_id=OLD.store_id AND entity_type='supplier' AND entity_id=OLD.id;
END;

INSERT INTO local_schema_migrations(version) VALUES (33);
COMMIT;
