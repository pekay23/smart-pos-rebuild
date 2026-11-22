PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE expenses
    ADD COLUMN cash_origin TEXT NOT NULL DEFAULT 'legacy_unknown'
        CHECK (cash_origin IN ('drawer','other_cash','legacy_unknown','not_applicable'));
ALTER TABLE expenses ADD COLUMN cash_source_note TEXT;
ALTER TABLE expenses ADD COLUMN idempotency_key TEXT;
UPDATE expenses
SET cash_origin = CASE WHEN payment_method='cash' THEN 'legacy_unknown' ELSE 'not_applicable' END;
CREATE UNIQUE INDEX idx_expenses_idempotency
    ON expenses(store_id,idempotency_key) WHERE idempotency_key IS NOT NULL;

ALTER TABLE supplier_payments
    ADD COLUMN cash_origin TEXT NOT NULL DEFAULT 'legacy_unknown'
        CHECK (cash_origin IN ('drawer','other_cash','legacy_unknown','not_applicable'));
ALTER TABLE supplier_payments ADD COLUMN cash_source_note TEXT;
UPDATE supplier_payments
SET cash_origin = CASE WHEN payment_method='cash' THEN 'legacy_unknown' ELSE 'not_applicable' END;

ALTER TABLE cash_movements ADD COLUMN source_type TEXT;
ALTER TABLE cash_movements ADD COLUMN source_id TEXT;
CREATE UNIQUE INDEX idx_cash_movements_source
    ON cash_movements(store_id,source_type,source_id)
    WHERE source_type IS NOT NULL AND source_id IS NOT NULL;

CREATE TRIGGER cash_movements_source_pair_insert
BEFORE INSERT ON cash_movements
WHEN (NEW.source_type IS NULL) <> (NEW.source_id IS NULL)
BEGIN
    SELECT RAISE(ABORT,'cash movement source identity must be complete');
END;
CREATE TRIGGER cash_movements_source_pair_update
BEFORE UPDATE OF source_type,source_id ON cash_movements
WHEN (NEW.source_type IS NULL) <> (NEW.source_id IS NULL)
BEGIN
    SELECT RAISE(ABORT,'cash movement source identity must be complete');
END;

CREATE TRIGGER cash_movements_source_validate_insert
BEFORE INSERT ON cash_movements
WHEN NEW.source_type IS NOT NULL
BEGIN
    SELECT CASE WHEN NEW.movement_type <> 'paid_out'
        THEN RAISE(ABORT,'source-linked cash movements must be paid out') END;
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM cash_shifts s WHERE s.id=NEW.shift_id AND s.store_id=NEW.store_id
    ) THEN RAISE(ABORT,'cash movement shift must belong to its store') END;
    SELECT CASE WHEN NEW.source_type='expense' AND NOT EXISTS (
        SELECT 1 FROM expenses e WHERE e.id=NEW.source_id AND e.store_id=NEW.store_id
          AND e.payment_method='cash' AND e.cash_origin='drawer' AND e.amount_minor=NEW.amount_minor
          AND e.user_id=NEW.user_id AND e.created_at=NEW.created_at
    ) THEN RAISE(ABORT,'cash movement does not match its drawer-funded expense') END;
    SELECT CASE WHEN NEW.source_type='supplier_payment' AND NOT EXISTS (
        SELECT 1 FROM supplier_payments p WHERE p.id=NEW.source_id AND p.store_id=NEW.store_id
          AND p.payment_method='cash' AND p.cash_origin='drawer' AND p.amount_minor=NEW.amount_minor
          AND p.recorded_by=NEW.user_id AND p.paid_at=NEW.created_at
    ) THEN RAISE(ABORT,'cash movement does not match its drawer-funded supplier payment') END;
    SELECT CASE WHEN NEW.source_type NOT IN ('expense','supplier_payment')
        THEN RAISE(ABORT,'unsupported source-linked cash movement') END;
END;

CREATE TRIGGER cash_movements_source_immutable_update
BEFORE UPDATE ON cash_movements
WHEN OLD.source_type IS NOT NULL
BEGIN
    SELECT RAISE(ABORT,'source-linked cash movements are immutable');
END;
CREATE TRIGGER cash_movements_source_immutable_delete
BEFORE DELETE ON cash_movements
WHEN OLD.source_type IS NOT NULL
BEGIN
    SELECT RAISE(ABORT,'source-linked cash movements are immutable');
END;

CREATE TRIGGER expenses_cash_origin_validate_insert
BEFORE INSERT ON expenses
WHEN (NEW.payment_method='cash' AND NEW.cash_origin NOT IN ('drawer','other_cash'))
  OR (NEW.payment_method<>'cash' AND (NEW.cash_origin<>'not_applicable' OR NEW.cash_source_note IS NOT NULL))
  OR (NEW.cash_origin='other_cash' AND length(trim(COALESCE(NEW.cash_source_note,'')))=0)
  OR (NEW.cash_origin='drawer' AND NEW.cash_source_note IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT,'expense cash origin does not match its payment method');
END;

CREATE TRIGGER supplier_payments_cash_origin_validate_insert
BEFORE INSERT ON supplier_payments
WHEN (NEW.payment_method='cash' AND NEW.cash_origin NOT IN ('drawer','other_cash'))
  OR (NEW.payment_method<>'cash' AND (NEW.cash_origin<>'not_applicable' OR NEW.cash_source_note IS NOT NULL))
  OR (NEW.cash_origin='other_cash' AND length(trim(COALESCE(NEW.cash_source_note,'')))=0)
  OR (NEW.cash_origin='drawer' AND NEW.cash_source_note IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT,'supplier payment cash origin does not match its payment method');
END;

CREATE TRIGGER expenses_no_update
BEFORE UPDATE ON expenses
BEGIN
    SELECT RAISE(ABORT,'expense records are immutable');
END;
CREATE TRIGGER expenses_no_delete
BEFORE DELETE ON expenses
BEGIN
    SELECT RAISE(ABORT,'expense records are immutable');
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (26);
COMMIT;
