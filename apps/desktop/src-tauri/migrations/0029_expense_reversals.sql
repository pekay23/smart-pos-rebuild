BEGIN IMMEDIATE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_expenses_id_store
    ON expenses(id, store_id);

CREATE TABLE expense_reversals (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    expense_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    payment_method TEXT NOT NULL CHECK (payment_method IN ('mobile_money','bank_transfer','other')),
    return_reference TEXT NOT NULL CHECK (length(trim(return_reference)) BETWEEN 1 AND 120),
    reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 5 AND 500),
    returned_at TEXT NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(trim(idempotency_key)) BETWEEN 1 AND 100),
    recorded_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (expense_id, store_id) REFERENCES expenses(id, store_id) ON DELETE RESTRICT,
    UNIQUE (id, business_id, store_id),
    UNIQUE (store_id, expense_id),
    UNIQUE (store_id, idempotency_key)
);

CREATE UNIQUE INDEX idx_expense_reversals_return_reference
    ON expense_reversals(store_id, payment_method, return_reference);
CREATE INDEX idx_expense_reversals_store_returned
    ON expense_reversals(store_id, returned_at DESC, created_at DESC, id DESC);

CREATE TRIGGER expense_reversals_validate_source
BEFORE INSERT ON expense_reversals
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM expenses e
        JOIN stores s ON s.id=e.store_id
        WHERE e.id=NEW.expense_id
          AND e.store_id=NEW.store_id
          AND s.business_id=NEW.business_id
          AND e.amount_minor=NEW.amount_minor
          AND e.payment_method=NEW.payment_method
          AND e.payment_method<>'cash'
    ) THEN RAISE(ABORT,'expense reversal source must be a matching non-cash expense in the same store') END;
END;

CREATE TRIGGER expense_reversals_no_update
BEFORE UPDATE ON expense_reversals
BEGIN
    SELECT RAISE(ABORT,'expense reversals are immutable');
END;
CREATE TRIGGER expense_reversals_no_delete
BEFORE DELETE ON expense_reversals
BEGIN
    SELECT RAISE(ABORT,'expense reversals are immutable');
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (29);
COMMIT;
