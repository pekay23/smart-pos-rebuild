PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE accounting_journals_new (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    source_type TEXT NOT NULL CHECK (source_type IN ('sale','refund','expense','purchase','supplier_bill','supplier_payment','supplier_credit','reversal','adjustment')),
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
DROP TRIGGER IF EXISTS accounting_journals_reject_closed_period;
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

CREATE TABLE supplier_bill_credit_notes (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    bill_id TEXT NOT NULL,
    supplier_id TEXT NOT NULL,
    credit_reference TEXT NOT NULL,
    reference_key TEXT NOT NULL,
    effective_date TEXT NOT NULL,
    reason TEXT NOT NULL,
    currency_code TEXT NOT NULL,
    total_minor INTEGER NOT NULL CHECK (total_minor > 0),
    idempotency_key TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (bill_id, business_id, store_id)
        REFERENCES supplier_bills(id, business_id, store_id) ON DELETE RESTRICT,
    FOREIGN KEY (supplier_id, store_id)
        REFERENCES suppliers(id, store_id) ON DELETE RESTRICT,
    UNIQUE (id, business_id, store_id),
    UNIQUE (store_id, idempotency_key),
    UNIQUE (business_id, supplier_id, reference_key)
);
CREATE INDEX idx_supplier_credit_notes_store_date
    ON supplier_bill_credit_notes(store_id, effective_date DESC, id DESC);
CREATE INDEX idx_supplier_credit_notes_bill
    ON supplier_bill_credit_notes(store_id, bill_id, effective_date DESC, id DESC);

CREATE UNIQUE INDEX idx_products_id_store
    ON products(id, store_id);

CREATE TABLE supplier_bill_credit_items (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    credit_note_id TEXT NOT NULL,
    purchase_id TEXT NOT NULL,
    purchase_item_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_cost_minor INTEGER NOT NULL CHECK (unit_cost_minor > 0),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    created_at TEXT NOT NULL,
    CHECK (amount_minor = quantity * unit_cost_minor),
    FOREIGN KEY (credit_note_id, business_id, store_id)
        REFERENCES supplier_bill_credit_notes(id, business_id, store_id) ON DELETE RESTRICT,
    FOREIGN KEY (purchase_id, store_id)
        REFERENCES purchases(id, store_id) ON DELETE RESTRICT,
    FOREIGN KEY (purchase_item_id)
        REFERENCES purchase_items(id) ON DELETE RESTRICT,
    FOREIGN KEY (product_id, store_id)
        REFERENCES products(id, store_id) ON DELETE RESTRICT,
    UNIQUE (credit_note_id, purchase_item_id)
);
CREATE INDEX idx_supplier_credit_items_purchase
    ON supplier_bill_credit_items(store_id, purchase_id, purchase_item_id);

CREATE TRIGGER supplier_bill_credit_notes_no_update BEFORE UPDATE ON supplier_bill_credit_notes BEGIN
    SELECT RAISE(ABORT, 'supplier credit notes are immutable');
END;
CREATE TRIGGER supplier_bill_credit_notes_no_delete BEFORE DELETE ON supplier_bill_credit_notes BEGIN
    SELECT RAISE(ABORT, 'supplier credit notes are immutable');
END;
CREATE TRIGGER supplier_bill_credit_items_no_update BEFORE UPDATE ON supplier_bill_credit_items BEGIN
    SELECT RAISE(ABORT, 'supplier credit note lines are immutable');
END;
CREATE TRIGGER supplier_bill_credit_items_no_delete BEFORE DELETE ON supplier_bill_credit_items BEGIN
    SELECT RAISE(ABORT, 'supplier credit note lines are immutable');
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (40);
COMMIT;
PRAGMA foreign_keys = ON;
