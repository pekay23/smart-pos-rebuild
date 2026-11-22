PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE store_preferences
    ADD COLUMN tax_rate_bps INTEGER NOT NULL DEFAULT 0 CHECK (tax_rate_bps BETWEEN 0 AND 10000);

UPDATE store_preferences
SET tax_rate_bps = COALESCE((
    SELECT CAST(app_settings.setting_value AS INTEGER)
    FROM stores
    JOIN app_settings ON app_settings.business_id = stores.business_id
    WHERE stores.id = store_preferences.store_id
      AND app_settings.setting_key = 'tax_rate_bps'
), 0);

DELETE FROM app_settings WHERE setting_key = 'tax_rate_bps';
INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (24);
COMMIT;
