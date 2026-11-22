BEGIN IMMEDIATE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_stores_id_business
    ON stores(id, business_id);

CREATE TABLE register_stations (
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    station_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (store_id, station_key),
    UNIQUE (business_id, store_id, station_key),
    FOREIGN KEY (store_id, business_id) REFERENCES stores(id, business_id) ON DELETE RESTRICT
);

INSERT OR IGNORE INTO register_stations
    (business_id,store_id,station_key,display_name,is_active,created_at,updated_at)
SELECT st.business_id,st.id,cs.station_key,
       CASE WHEN cs.station_key='main' THEN 'Main register' ELSE 'Register ' || cs.station_key END,
       1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP
FROM stores st
JOIN (SELECT DISTINCT store_id,station_key FROM cash_shifts) cs ON cs.store_id=st.id;

INSERT OR IGNORE INTO register_stations
    (business_id,store_id,station_key,display_name,is_active,created_at,updated_at)
SELECT st.business_id,st.id,'main','Main register',1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP
FROM stores st
WHERE NOT EXISTS (SELECT 1 FROM register_stations rs WHERE rs.store_id=st.id);

ALTER TABLE cash_shifts
    ADD COLUMN station_name_snapshot TEXT NOT NULL DEFAULT 'Main register';

UPDATE cash_shifts
SET station_name_snapshot=COALESCE(
    (SELECT rs.display_name FROM register_stations rs
     WHERE rs.store_id=cash_shifts.store_id AND rs.station_key=cash_shifts.station_key),
    cash_shifts.station_key
);

ALTER TABLE sales ADD COLUMN station_key TEXT;
ALTER TABLE sales ADD COLUMN station_name_snapshot TEXT NOT NULL DEFAULT 'Main register';
UPDATE sales
SET station_key=(SELECT cs.station_key FROM cash_shifts cs
                 WHERE cs.id=sales.cash_shift_id AND cs.store_id=sales.store_id)
WHERE cash_shift_id IS NOT NULL;
UPDATE sales
SET station_name_snapshot=COALESCE(
    (SELECT rs.display_name FROM register_stations rs
     WHERE rs.store_id=sales.store_id AND rs.station_key=sales.station_key),
    'Main register'
);

ALTER TABLE refunds ADD COLUMN station_key TEXT;
ALTER TABLE refunds ADD COLUMN station_name_snapshot TEXT NOT NULL DEFAULT 'Main register';

CREATE TABLE station_devices (
    device_id TEXT NOT NULL,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    station_key TEXT NOT NULL,
    assigned_by TEXT REFERENCES users(id) ON DELETE RESTRICT,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (device_id, store_id),
    FOREIGN KEY (store_id, business_id) REFERENCES stores(id, business_id) ON DELETE RESTRICT,
    FOREIGN KEY (store_id, station_key) REFERENCES register_stations(store_id, station_key) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX idx_station_devices_station
    ON station_devices(store_id,station_key);
CREATE INDEX idx_station_devices_business_store
    ON station_devices(business_id,store_id);
CREATE INDEX idx_cash_shifts_station_closed
    ON cash_shifts(store_id,station_key,status,closed_at DESC,id DESC);
CREATE INDEX idx_sales_store_station_date
    ON sales(store_id,station_key,created_at DESC,id DESC);
CREATE INDEX idx_refunds_store_station_date
    ON refunds(store_id,station_key,created_at DESC,id DESC);

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (23);
COMMIT;
