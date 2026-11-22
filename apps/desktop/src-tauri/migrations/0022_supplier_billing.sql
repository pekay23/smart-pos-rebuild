PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE accounting_journals_new (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    source_type TEXT NOT NULL CHECK (source_type IN ('sale','refund','expense','purchase','supplier_bill','supplier_payment','reversal','adjustment')),
    source_id TEXT NOT NULL,
    currency_code TEXT NOT NULL,
    effective_at TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    created_by TEXT REFERENCES users(id) ON DELETE RESTRICT,
    description TEXT NOT NULL,
    coverage_status TEXT NOT NULL CHECK (coverage_status IN ('complete','partial')),
    reversal_of TEXT REFERENCES accounting_journals_new(id) ON DELETE RESTRICT,
    UNIQUE (business_id, source_type, source_id),
    UNIQUE (id, business_id)
);
INSERT INTO accounting_journals_new
    (id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,created_by,description,coverage_status,reversal_of)
SELECT id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,created_by,description,coverage_status,reversal_of
FROM accounting_journals;

CREATE TABLE accounting_journal_lines_new (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    journal_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    line_number INTEGER NOT NULL CHECK (line_number > 0),
    memo TEXT NOT NULL,
    debit_minor INTEGER NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
    credit_minor INTEGER NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
    CHECK ((debit_minor > 0 AND credit_minor = 0) OR (credit_minor > 0 AND debit_minor = 0)),
    FOREIGN KEY (journal_id, business_id) REFERENCES accounting_journals_new(id, business_id) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, business_id) REFERENCES accounting_accounts(id, business_id) ON DELETE RESTRICT,
    UNIQUE (journal_id, line_number)
);
INSERT INTO accounting_journal_lines_new
    (id,business_id,journal_id,account_id,line_number,memo,debit_minor,credit_minor)
SELECT id,business_id,journal_id,account_id,line_number,memo,debit_minor,credit_minor
FROM accounting_journal_lines;

DROP TRIGGER IF EXISTS accounting_lines_no_update;
DROP TRIGGER IF EXISTS accounting_lines_no_delete;
DROP TRIGGER IF EXISTS accounting_journals_no_update;
DROP TRIGGER IF EXISTS accounting_journals_no_delete;
DROP TABLE accounting_journal_lines;
DROP TABLE accounting_journals;
ALTER TABLE accounting_journals_new RENAME TO accounting_journals;
ALTER TABLE accounting_journal_lines_new RENAME TO accounting_journal_lines;
CREATE INDEX idx_accounting_journals_store_effective
    ON accounting_journals(business_id, store_id, effective_at DESC, id DESC);
CREATE UNIQUE INDEX idx_accounting_journals_one_reversal
    ON accounting_journals(reversal_of) WHERE reversal_of IS NOT NULL;
CREATE INDEX idx_accounting_lines_account_journal
    ON accounting_journal_lines(business_id, account_id, journal_id);
CREATE TRIGGER accounting_journals_no_update
BEFORE UPDATE ON accounting_journals BEGIN
    SELECT RAISE(ABORT, 'posted accounting journals are immutable');
END;
CREATE TRIGGER accounting_journals_no_delete
BEFORE DELETE ON accounting_journals BEGIN
    SELECT RAISE(ABORT, 'posted accounting journals are immutable');
END;
CREATE TRIGGER accounting_lines_no_update
BEFORE UPDATE ON accounting_journal_lines BEGIN
    SELECT RAISE(ABORT, 'posted accounting lines are immutable');
END;
CREATE TRIGGER accounting_lines_no_delete
BEFORE DELETE ON accounting_journal_lines BEGIN
    SELECT RAISE(ABORT, 'posted accounting lines are immutable');
END;

CREATE UNIQUE INDEX idx_suppliers_id_store ON suppliers(id, store_id);
CREATE UNIQUE INDEX idx_purchases_id_store ON purchases(id, store_id);

UPDATE roles
SET permissions_json = json_insert(permissions_json, '$[#]', 'purchases:invoice')
WHERE role_key IN ('owner','admin')
  AND NOT EXISTS (SELECT 1 FROM json_each(roles.permissions_json) WHERE value='purchases:invoice');
UPDATE roles
SET permissions_json = json_insert(permissions_json, '$[#]', 'suppliers:settle')
WHERE role_key IN ('owner','admin')
  AND NOT EXISTS (SELECT 1 FROM json_each(roles.permissions_json) WHERE value='suppliers:settle');

CREATE TABLE supplier_bills (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    supplier_id TEXT NOT NULL,
    supplier_reference TEXT NOT NULL,
    reference_key TEXT NOT NULL,
    invoice_date TEXT NOT NULL,
    due_date TEXT,
    currency_code TEXT NOT NULL,
    total_minor INTEGER NOT NULL CHECK (total_minor > 0),
    idempotency_key TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (supplier_id, store_id) REFERENCES suppliers(id, store_id) ON DELETE RESTRICT,
    UNIQUE (id, business_id, store_id),
    UNIQUE (business_id, supplier_id, reference_key),
    UNIQUE (store_id, idempotency_key)
);
CREATE INDEX idx_supplier_bills_store_date
    ON supplier_bills(store_id, invoice_date DESC, id DESC);

CREATE TABLE supplier_bill_allocations (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    bill_id TEXT NOT NULL,
    purchase_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    created_at TEXT NOT NULL,
    FOREIGN KEY (bill_id, business_id, store_id)
        REFERENCES supplier_bills(id, business_id, store_id) ON DELETE RESTRICT,
    FOREIGN KEY (purchase_id, store_id)
        REFERENCES purchases(id, store_id) ON DELETE RESTRICT,
    UNIQUE (bill_id, purchase_id)
);
CREATE INDEX idx_supplier_bill_allocations_purchase
    ON supplier_bill_allocations(store_id, purchase_id);

CREATE TABLE supplier_payments (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    supplier_id TEXT NOT NULL,
    currency_code TEXT NOT NULL,
    payment_method TEXT NOT NULL CHECK (payment_method IN ('cash','card','mobile_money','bank_transfer','other')),
    external_reference TEXT,
    reference_key TEXT,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    idempotency_key TEXT NOT NULL,
    recorded_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    paid_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (supplier_id, store_id) REFERENCES suppliers(id, store_id) ON DELETE RESTRICT,
    UNIQUE (id, business_id, store_id),
    UNIQUE (store_id, idempotency_key)
);
CREATE UNIQUE INDEX idx_supplier_payments_external_reference
    ON supplier_payments(store_id, payment_method, reference_key)
    WHERE reference_key IS NOT NULL;
CREATE INDEX idx_supplier_payments_store_paid
    ON supplier_payments(store_id, paid_at DESC, id DESC);

CREATE TABLE supplier_payment_allocations (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    payment_id TEXT NOT NULL,
    bill_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    created_at TEXT NOT NULL,
    FOREIGN KEY (payment_id, business_id, store_id)
        REFERENCES supplier_payments(id, business_id, store_id) ON DELETE RESTRICT,
    FOREIGN KEY (bill_id, business_id, store_id)
        REFERENCES supplier_bills(id, business_id, store_id) ON DELETE RESTRICT,
    UNIQUE (payment_id, bill_id)
);
CREATE INDEX idx_supplier_payment_allocations_bill
    ON supplier_payment_allocations(store_id, bill_id);

CREATE TRIGGER supplier_bills_no_update BEFORE UPDATE ON supplier_bills BEGIN
    SELECT RAISE(ABORT, 'supplier bills are immutable');
END;
CREATE TRIGGER supplier_bills_no_delete BEFORE DELETE ON supplier_bills BEGIN
    SELECT RAISE(ABORT, 'supplier bills are immutable');
END;
CREATE TRIGGER supplier_bill_allocations_no_update BEFORE UPDATE ON supplier_bill_allocations BEGIN
    SELECT RAISE(ABORT, 'supplier bill allocations are immutable');
END;
CREATE TRIGGER supplier_bill_allocations_no_delete BEFORE DELETE ON supplier_bill_allocations BEGIN
    SELECT RAISE(ABORT, 'supplier bill allocations are immutable');
END;
CREATE TRIGGER supplier_payments_no_update BEFORE UPDATE ON supplier_payments BEGIN
    SELECT RAISE(ABORT, 'supplier payments are immutable');
END;
CREATE TRIGGER supplier_payments_no_delete BEFORE DELETE ON supplier_payments BEGIN
    SELECT RAISE(ABORT, 'supplier payments are immutable');
END;
CREATE TRIGGER supplier_payment_allocations_no_update BEFORE UPDATE ON supplier_payment_allocations BEGIN
    SELECT RAISE(ABORT, 'supplier payment allocations are immutable');
END;
CREATE TRIGGER supplier_payment_allocations_no_delete BEFORE DELETE ON supplier_payment_allocations BEGIN
    SELECT RAISE(ABORT, 'supplier payment allocations are immutable');
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (22);
COMMIT;
PRAGMA foreign_keys = ON;
