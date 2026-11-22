CREATE TABLE accounting_period_closures (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    from_utc TEXT NOT NULL,
    to_utc_exclusive TEXT NOT NULL,
    timezone TEXT NOT NULL,
    currency_code TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    snapshot_sha256 TEXT NOT NULL CHECK (length(snapshot_sha256) = 64),
    closed_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    closed_at TEXT NOT NULL,
    CHECK (from_utc < to_utc_exclusive),
    UNIQUE (business_id, store_id, from_utc, to_utc_exclusive),
    UNIQUE (business_id, store_id, id)
);
CREATE INDEX idx_accounting_period_closures_store_date
    ON accounting_period_closures(business_id, store_id, from_utc DESC);

CREATE TRIGGER accounting_period_closures_no_overlap
BEFORE INSERT ON accounting_period_closures
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.business_id=NEW.business_id AND c.store_id=NEW.store_id
      AND NEW.from_utc < c.to_utc_exclusive
      AND NEW.to_utc_exclusive > c.from_utc
)
BEGIN
    SELECT RAISE(ABORT, 'accounting close periods cannot overlap');
END;
CREATE TRIGGER accounting_period_closures_no_update
BEFORE UPDATE ON accounting_period_closures BEGIN
    SELECT RAISE(ABORT, 'accounting period closes are immutable');
END;
CREATE TRIGGER accounting_period_closures_no_delete
BEFORE DELETE ON accounting_period_closures BEGIN
    SELECT RAISE(ABORT, 'accounting period closes are immutable');
END;

CREATE TRIGGER accounting_journals_reject_closed_period
BEFORE INSERT ON accounting_journals
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.business_id=NEW.business_id AND c.store_id=NEW.store_id
      AND NEW.effective_at>=c.from_utc AND NEW.effective_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'accounting journal date is in a closed period');
END;

CREATE TRIGGER accounting_sales_reject_closed_period
BEFORE INSERT ON sales
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id AND NEW.created_at>=c.from_utc AND NEW.created_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'sale date is in a closed accounting period');
END;
CREATE TRIGGER accounting_sales_update_reject_closed_period
BEFORE UPDATE OF store_id,created_at ON sales
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.created_at>=c.from_utc AND OLD.created_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.created_at>=c.from_utc AND NEW.created_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'sale date is in a closed accounting period');
END;
CREATE TRIGGER accounting_refunds_reject_closed_period
BEFORE INSERT ON refunds
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id AND NEW.created_at>=c.from_utc AND NEW.created_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'refund date is in a closed accounting period');
END;
CREATE TRIGGER accounting_refunds_update_reject_closed_period
BEFORE UPDATE OF store_id,created_at ON refunds
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.created_at>=c.from_utc AND OLD.created_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.created_at>=c.from_utc AND NEW.created_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'refund date is in a closed accounting period');
END;
CREATE TRIGGER accounting_expenses_reject_closed_period
BEFORE INSERT ON expenses
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id AND NEW.occurred_at>=c.from_utc AND NEW.occurred_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'expense date is in a closed accounting period');
END;
CREATE TRIGGER accounting_expenses_update_reject_closed_period
BEFORE UPDATE OF store_id,occurred_at ON expenses
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.occurred_at>=c.from_utc AND OLD.occurred_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.occurred_at>=c.from_utc AND NEW.occurred_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'expense date is in a closed accounting period');
END;
CREATE TRIGGER accounting_purchases_reject_closed_period
BEFORE INSERT ON purchases
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id AND NEW.received_at>=c.from_utc AND NEW.received_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'purchase date is in a closed accounting period');
END;
CREATE TRIGGER accounting_purchases_update_reject_closed_period
BEFORE UPDATE OF store_id,received_at ON purchases
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.received_at>=c.from_utc AND OLD.received_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.received_at>=c.from_utc AND NEW.received_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'purchase date is in a closed accounting period');
END;
CREATE TRIGGER accounting_supplier_bills_reject_closed_period
BEFORE INSERT ON supplier_bills
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id
      AND NEW.created_at>=c.from_utc AND NEW.created_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'supplier bill date is in a closed accounting period');
END;
CREATE TRIGGER accounting_supplier_bills_update_reject_closed_period
BEFORE UPDATE OF store_id,created_at,invoice_date ON supplier_bills
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.created_at>=c.from_utc AND OLD.created_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.created_at>=c.from_utc AND NEW.created_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'supplier bill date is in a closed accounting period');
END;
CREATE TRIGGER accounting_supplier_payments_reject_closed_period
BEFORE INSERT ON supplier_payments
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id AND NEW.paid_at>=c.from_utc AND NEW.paid_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'supplier payment date is in a closed accounting period');
END;
CREATE TRIGGER accounting_supplier_payments_update_reject_closed_period
BEFORE UPDATE OF store_id,paid_at ON supplier_payments
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.paid_at>=c.from_utc AND OLD.paid_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.paid_at>=c.from_utc AND NEW.paid_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'supplier payment date is in a closed accounting period');
END;
CREATE TRIGGER accounting_cash_shifts_reject_closed_period
BEFORE INSERT ON cash_shifts
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE c.store_id=NEW.store_id AND NEW.opened_at>=c.from_utc AND NEW.opened_at<c.to_utc_exclusive
)
BEGIN
    SELECT RAISE(ABORT, 'cash drawer shift date is in a closed accounting period');
END;
CREATE TRIGGER accounting_cash_shifts_update_reject_closed_period
BEFORE UPDATE OF store_id,opened_at ON cash_shifts
WHEN EXISTS (
    SELECT 1 FROM accounting_period_closures c
    WHERE (c.store_id=OLD.store_id AND OLD.opened_at>=c.from_utc AND OLD.opened_at<c.to_utc_exclusive)
       OR (c.store_id=NEW.store_id AND NEW.opened_at>=c.from_utc AND NEW.opened_at<c.to_utc_exclusive)
)
BEGIN
    SELECT RAISE(ABORT, 'cash drawer shift date is in a closed accounting period');
END;

UPDATE roles
SET permissions_json = json_insert(permissions_json, '$[#]', 'accounting:close')
WHERE role_key IN ('owner','admin')
  AND NOT EXISTS (SELECT 1 FROM json_each(roles.permissions_json) WHERE value='accounting:close');

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (39);
