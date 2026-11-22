BEGIN IMMEDIATE;

ALTER TABLE expenses
    ADD COLUMN cost_center_kind TEXT NOT NULL DEFAULT 'legacy_unassigned'
    CHECK (cost_center_kind IN ('station_direct','store_overhead','legacy_unassigned'));
ALTER TABLE expenses ADD COLUMN cost_center_station_key TEXT;
ALTER TABLE expenses ADD COLUMN cost_center_station_name_snapshot TEXT;

CREATE INDEX idx_expenses_cost_center_period
    ON expenses(store_id,cost_center_kind,cost_center_station_key,occurred_at);

CREATE TRIGGER expenses_cost_center_validate_insert
BEFORE INSERT ON expenses
WHEN
    (NEW.cost_center_kind='station_direct' AND (
        NEW.cost_center_station_key IS NULL OR
        NEW.cost_center_station_name_snapshot IS NULL OR
        NOT EXISTS (
            SELECT 1 FROM register_stations rs
            WHERE rs.store_id=NEW.store_id
              AND rs.station_key=NEW.cost_center_station_key
              AND rs.display_name=NEW.cost_center_station_name_snapshot
              AND rs.is_active=1
        )
    )) OR
    (NEW.cost_center_kind<>'station_direct' AND (
        NEW.cost_center_station_key IS NOT NULL OR
        NEW.cost_center_station_name_snapshot IS NOT NULL
    ))
BEGIN
    SELECT RAISE(ABORT,'expense cost center is invalid');
END;

CREATE TRIGGER expenses_cost_center_validate_update
BEFORE UPDATE OF cost_center_kind,cost_center_station_key,cost_center_station_name_snapshot ON expenses
WHEN
    NEW.cost_center_kind IS NOT OLD.cost_center_kind OR
    NEW.cost_center_station_key IS NOT OLD.cost_center_station_key OR
    NEW.cost_center_station_name_snapshot IS NOT OLD.cost_center_station_name_snapshot
BEGIN
    SELECT RAISE(ABORT,'expense cost centers are immutable; use an audited reclassification');
END;

INSERT INTO local_schema_migrations(version) VALUES (31);
COMMIT;
