PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE store_preferences
    ADD COLUMN receipt_header_text TEXT NOT NULL DEFAULT '' CHECK (length(receipt_header_text) <= 160);
ALTER TABLE store_preferences
    ADD COLUMN receipt_footer_text TEXT NOT NULL DEFAULT 'Thank you for shopping with us.' CHECK (length(receipt_footer_text) <= 240);
ALTER TABLE store_preferences
    ADD COLUMN receipt_paper_width_mm INTEGER NOT NULL DEFAULT 80 CHECK (receipt_paper_width_mm IN (58, 80));

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (25);
COMMIT;
