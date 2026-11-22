BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS cash_opening_balances (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    currency_code TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0 AND amount_minor <= 9000000000000),
    effective_at TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 3 AND 200),
    idempotency_key TEXT NOT NULL CHECK (length(trim(idempotency_key)) BETWEEN 1 AND 100),
    journal_id TEXT NOT NULL UNIQUE REFERENCES accounting_journals(id) ON DELETE RESTRICT,
    recorded_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    recorded_at TEXT NOT NULL,
    UNIQUE (business_id, store_id),
    UNIQUE (business_id, store_id, idempotency_key)
);

CREATE TRIGGER IF NOT EXISTS cash_opening_balances_no_update
BEFORE UPDATE ON cash_opening_balances BEGIN
    SELECT RAISE(ABORT, 'cash opening balances are immutable');
END;
CREATE TRIGGER IF NOT EXISTS cash_opening_balances_no_delete
BEFORE DELETE ON cash_opening_balances BEGIN
    SELECT RAISE(ABORT, 'cash opening balances are immutable');
END;

CREATE TRIGGER IF NOT EXISTS cash_opening_balances_scope_guard
BEFORE INSERT ON cash_opening_balances
WHEN NOT EXISTS (
    SELECT 1 FROM stores s
    WHERE s.id=NEW.store_id AND s.business_id=NEW.business_id
) OR NOT EXISTS (
    SELECT 1 FROM users u
    WHERE u.id=NEW.recorded_by AND u.business_id=NEW.business_id
) OR NOT EXISTS (
    SELECT 1 FROM accounting_journals j
    WHERE j.id=NEW.journal_id AND j.business_id=NEW.business_id AND j.store_id=NEW.store_id
      AND j.source_type='adjustment' AND j.source_id='cash-opening-'||NEW.id
) OR NOT EXISTS (
    SELECT 1 FROM accounting_journal_lines l
    JOIN accounting_accounts a ON a.id=l.account_id AND a.business_id=l.business_id
    WHERE l.journal_id=NEW.journal_id AND l.business_id=NEW.business_id
      AND a.code='1000' AND l.debit_minor=NEW.amount_minor AND l.credit_minor=0
) OR NOT EXISTS (
    SELECT 1 FROM accounting_journal_lines l
    JOIN accounting_accounts a ON a.id=l.account_id AND a.business_id=l.business_id
    WHERE l.journal_id=NEW.journal_id AND l.business_id=NEW.business_id
      AND a.code='3000' AND l.credit_minor=NEW.amount_minor AND l.debit_minor=0
)
BEGIN
    SELECT RAISE(ABORT, 'cash opening balance must match its same-store journal and owner record');
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (27);
COMMIT;
