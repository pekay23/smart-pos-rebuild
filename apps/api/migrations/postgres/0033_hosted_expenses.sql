-- Append-only hosted expense sources with balanced manual-payment journals.
-- Cash expenses wait for the branch cash-location ledger; this slice never
-- records a drawer or safe movement without an explicit, linked destination.

CREATE TABLE hosted_expenses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    category_code TEXT NOT NULL CHECK (category_code IN ('rent','utilities','wages','transport','supplies','maintenance','marketing','bank_fees','taxes_fees','other')),
    description TEXT NOT NULL CHECK (length(btrim(description)) BETWEEN 1 AND 240),
    payee TEXT CHECK (payee IS NULL OR length(btrim(payee)) BETWEEN 1 AND 160),
    amount_minor BIGINT NOT NULL CHECK (amount_minor BETWEEN 1 AND 9007199254740991),
    payment_method TEXT NOT NULL CHECK (payment_method IN ('card','mobile_money','bank_transfer','other')),
    external_reference TEXT CHECK (external_reference IS NULL OR length(btrim(external_reference)) BETWEEN 1 AND 160),
    occurred_on DATE NOT NULL,
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key)
);
CREATE INDEX hosted_expenses_branch_date ON hosted_expenses(tenant_id,branch_id,occurred_on DESC,id);

ALTER TABLE hosted_accounting_journals DROP CONSTRAINT hosted_accounting_journals_source_type_check;
ALTER TABLE hosted_accounting_journals ADD CONSTRAINT hosted_accounting_journals_source_type_check
    CHECK (source_type IN ('supplier_bill','supplier_payment','expense'));

ALTER TABLE hosted_expenses ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_expenses FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_expenses_branch_read ON hosted_expenses FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m
      JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_expenses.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_expenses.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_expenses_branch_insert ON hosted_expenses FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m
      JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=actor_membership_id
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_expenses.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_expenses.branch_id) AND g.station_id IS NULL)
);

CREATE POLICY hosted_accounting_journals_expense_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
    source_type='expense'
    AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_expenses e WHERE e.tenant_id=hosted_accounting_journals.tenant_id
      AND e.branch_id=hosted_accounting_journals.branch_id AND e.id=source_id)
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g
      ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.status='active' AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_accounting_journals.branch_id) AND g.station_id IS NULL)
);

CREATE POLICY hosted_accounting_journal_lines_expense_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_accounting_journals j
      JOIN hosted_expenses e ON e.tenant_id=j.tenant_id AND e.branch_id=j.branch_id AND e.id=j.source_id
      JOIN memberships m ON m.id=j.actor_membership_id AND m.tenant_id=j.tenant_id
      JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
        AND j.id=hosted_accounting_journal_lines.journal_id AND j.source_type='expense'
        AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=j.branch_id) AND g.station_id IS NULL)
);

GRANT SELECT (id,tenant_id,branch_id,currency_code,category_code,description,payee,amount_minor,payment_method,external_reference,occurred_on,actor_membership_id,idempotency_key,request_hash,created_at)
  ON hosted_expenses TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,currency_code,category_code,description,payee,amount_minor,payment_method,external_reference,occurred_on,actor_membership_id,idempotency_key,request_hash)
  ON hosted_expenses TO pos_api_runtime;
