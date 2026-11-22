BEGIN IMMEDIATE;

CREATE TABLE product_brands (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (business_id, id)
);
CREATE UNIQUE INDEX idx_product_brands_business_name
    ON product_brands(business_id, name COLLATE NOCASE);

CREATE TABLE product_tag_catalog (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (business_id, id)
);
CREATE UNIQUE INDEX idx_product_tag_catalog_business_name
    ON product_tag_catalog(business_id, name COLLATE NOCASE);

ALTER TABLE products ADD COLUMN brand_id TEXT REFERENCES product_brands(id) ON DELETE SET NULL;
CREATE INDEX idx_products_brand ON products(brand_id);

CREATE TABLE product_tag_links (
    product_id TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES product_tag_catalog(id) ON DELETE RESTRICT,
    PRIMARY KEY (product_id, tag_id)
);
CREATE INDEX idx_product_tag_links_tag ON product_tag_links(tag_id, product_id);

CREATE TRIGGER product_brand_same_business_insert
BEFORE INSERT ON products
WHEN NEW.brand_id IS NOT NULL
 AND NOT EXISTS (
    SELECT 1 FROM product_brands b JOIN stores s ON s.id=NEW.store_id
    WHERE b.id=NEW.brand_id AND b.business_id=s.business_id
 )
BEGIN
    SELECT RAISE(ABORT, 'product brand must belong to the product business');
END;

CREATE TRIGGER product_brand_same_business_update
BEFORE UPDATE OF brand_id,store_id ON products
WHEN NEW.brand_id IS NOT NULL
 AND NOT EXISTS (
    SELECT 1 FROM product_brands b JOIN stores s ON s.id=NEW.store_id
    WHERE b.id=NEW.brand_id AND b.business_id=s.business_id
 )
BEGIN
    SELECT RAISE(ABORT, 'product brand must belong to the product business');
END;

CREATE TRIGGER product_tag_same_business_insert
BEFORE INSERT ON product_tag_links
WHEN NOT EXISTS (
    SELECT 1 FROM products p
    JOIN stores s ON s.id=p.store_id
    JOIN product_tag_catalog t ON t.id=NEW.tag_id
    WHERE p.id=NEW.product_id AND t.business_id=s.business_id
 )
BEGIN
    SELECT RAISE(ABORT, 'product tag must belong to the product business');
END;

INSERT INTO local_schema_migrations(version) VALUES (36);
COMMIT;
