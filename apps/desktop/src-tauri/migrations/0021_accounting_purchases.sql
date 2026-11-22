PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE accounting_journals_new (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    source_type TEXT NOT NULL CHECK (source_type IN ('sale','refund','expense','purchase','reversal','adjustment')),
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

CREATE TABLE IF NOT EXISTS local_schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (21);

COMMIT;
PRAGMA foreign_keys = ON;
