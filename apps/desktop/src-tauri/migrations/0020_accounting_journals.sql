CREATE TABLE IF NOT EXISTS accounting_accounts (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    account_type TEXT NOT NULL CHECK (account_type IN ('asset','liability','equity','revenue','contra_revenue','expense')),
    is_system INTEGER NOT NULL DEFAULT 1 CHECK (is_system IN (0,1)),
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    created_at TEXT NOT NULL,
    UNIQUE (business_id, code),
    UNIQUE (id, business_id)
);

CREATE TABLE IF NOT EXISTS accounting_journals (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    source_type TEXT NOT NULL CHECK (source_type IN ('sale','refund','expense','reversal','adjustment')),
    source_id TEXT NOT NULL,
    currency_code TEXT NOT NULL,
    effective_at TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    created_by TEXT REFERENCES users(id) ON DELETE RESTRICT,
    description TEXT NOT NULL,
    coverage_status TEXT NOT NULL CHECK (coverage_status IN ('complete','partial')),
    reversal_of TEXT REFERENCES accounting_journals(id) ON DELETE RESTRICT,
    UNIQUE (business_id, source_type, source_id),
    UNIQUE (id, business_id)
);
CREATE INDEX IF NOT EXISTS idx_accounting_journals_store_effective
    ON accounting_journals(business_id, store_id, effective_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS accounting_journal_lines (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    journal_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    line_number INTEGER NOT NULL CHECK (line_number > 0),
    memo TEXT NOT NULL,
    debit_minor INTEGER NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
    credit_minor INTEGER NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
    CHECK ((debit_minor > 0 AND credit_minor = 0) OR (credit_minor > 0 AND debit_minor = 0)),
    FOREIGN KEY (journal_id, business_id) REFERENCES accounting_journals(id, business_id) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, business_id) REFERENCES accounting_accounts(id, business_id) ON DELETE RESTRICT,
    UNIQUE (journal_id, line_number)
);
CREATE INDEX IF NOT EXISTS idx_accounting_lines_account_journal
    ON accounting_journal_lines(business_id, account_id, journal_id);

CREATE TRIGGER IF NOT EXISTS accounting_journals_no_update
BEFORE UPDATE ON accounting_journals BEGIN
    SELECT RAISE(ABORT, 'posted accounting journals are immutable');
END;
CREATE TRIGGER IF NOT EXISTS accounting_journals_no_delete
BEFORE DELETE ON accounting_journals BEGIN
    SELECT RAISE(ABORT, 'posted accounting journals are immutable');
END;
CREATE TRIGGER IF NOT EXISTS accounting_lines_no_update
BEFORE UPDATE ON accounting_journal_lines BEGIN
    SELECT RAISE(ABORT, 'posted accounting lines are immutable');
END;
CREATE TRIGGER IF NOT EXISTS accounting_lines_no_delete
BEFORE DELETE ON accounting_journal_lines BEGIN
    SELECT RAISE(ABORT, 'posted accounting lines are immutable');
END;
