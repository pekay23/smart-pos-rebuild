BEGIN IMMEDIATE;

CREATE TABLE expense_cost_center_reclassifications (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    expense_id TEXT NOT NULL REFERENCES expenses(id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    expected_revision INTEGER NOT NULL CHECK (expected_revision >= 0),
    previous_kind TEXT NOT NULL CHECK (previous_kind IN ('station_direct','store_overhead','legacy_unassigned')),
    previous_station_key TEXT,
    previous_station_name_snapshot TEXT,
    next_kind TEXT NOT NULL CHECK (next_kind IN ('station_direct','store_overhead','legacy_unassigned')),
    next_station_key TEXT,
    next_station_name_snapshot TEXT,
    reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 5 AND 500),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 100),
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    recorded_at TEXT NOT NULL,
    UNIQUE(expense_id, revision),
    UNIQUE(store_id, idempotency_key),
    CHECK (
        (previous_kind='station_direct' AND previous_station_key IS NOT NULL AND previous_station_name_snapshot IS NOT NULL)
        OR (previous_kind<>'station_direct' AND previous_station_key IS NULL AND previous_station_name_snapshot IS NULL)
    ),
    CHECK (
        (next_kind='station_direct' AND next_station_key IS NOT NULL AND next_station_name_snapshot IS NOT NULL)
        OR (next_kind<>'station_direct' AND next_station_key IS NULL AND next_station_name_snapshot IS NULL)
    )
);

CREATE INDEX idx_expense_cost_center_reclass_history
    ON expense_cost_center_reclassifications(store_id,expense_id,revision);

CREATE TRIGGER expense_cost_center_reclass_immutable_update
BEFORE UPDATE ON expense_cost_center_reclassifications
BEGIN
    SELECT RAISE(ABORT,'expense cost-center reclassifications are immutable');
END;

CREATE TRIGGER expense_cost_center_reclass_immutable_delete
BEFORE DELETE ON expense_cost_center_reclassifications
BEGIN
    SELECT RAISE(ABORT,'expense cost-center reclassifications are immutable');
END;

UPDATE roles
SET permissions_json=json_insert(permissions_json,'$[#]','expenses:reclassify')
WHERE role_key IN ('owner','admin')
  AND json_valid(permissions_json)
  AND NOT EXISTS (
      SELECT 1 FROM json_each(roles.permissions_json)
      WHERE json_each.value='expenses:reclassify'
  );

INSERT INTO local_schema_migrations(version) VALUES (32);
COMMIT;
