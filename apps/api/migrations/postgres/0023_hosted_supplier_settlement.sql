-- Manual supplier settlements with append-only, balanced source journals.
CREATE TABLE hosted_supplier_payments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    supplier_id UUID NOT NULL,
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    payment_method TEXT NOT NULL CHECK (payment_method IN ('cash','card','mobile_money','bank_transfer','other')),
    external_reference TEXT CHECK (external_reference IS NULL OR length(btrim(external_reference)) BETWEEN 1 AND 100),
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    paid_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,supplier_id) REFERENCES hosted_suppliers(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key)
);
CREATE INDEX hosted_supplier_payments_branch_date ON hosted_supplier_payments(tenant_id,branch_id,paid_at DESC,id);

CREATE TABLE hosted_supplier_payment_allocations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    payment_id UUID NOT NULL,
    bill_id UUID NOT NULL,
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,payment_id) REFERENCES hosted_supplier_payments(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,bill_id) REFERENCES hosted_supplier_bills(tenant_id,branch_id,id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,payment_id,bill_id)
);
CREATE INDEX hosted_supplier_payment_allocations_bill ON hosted_supplier_payment_allocations(tenant_id,branch_id,bill_id);

CREATE TABLE hosted_accounting_journals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('supplier_bill','supplier_payment')),
    source_id UUID NOT NULL,
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    effective_at TIMESTAMPTZ NOT NULL,
    actor_membership_id UUID NOT NULL,
    description TEXT NOT NULL CHECK (length(btrim(description)) BETWEEN 1 AND 200),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,source_type,source_id)
);
CREATE INDEX hosted_accounting_journals_branch_date ON hosted_accounting_journals(tenant_id,branch_id,effective_at DESC,id);

CREATE TABLE hosted_accounting_journal_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    journal_id UUID NOT NULL,
    line_number SMALLINT NOT NULL CHECK (line_number BETWEEN 1 AND 100),
    account_code TEXT NOT NULL CHECK (account_code ~ '^[0-9]{4}$'),
    description TEXT NOT NULL CHECK (length(btrim(description)) BETWEEN 1 AND 200),
    debit_minor BIGINT NOT NULL DEFAULT 0 CHECK (debit_minor >= 0),
    credit_minor BIGINT NOT NULL DEFAULT 0 CHECK (credit_minor >= 0),
    FOREIGN KEY (tenant_id,branch_id,journal_id) REFERENCES hosted_accounting_journals(tenant_id,branch_id,id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,journal_id,line_number),
    CHECK ((debit_minor > 0 AND credit_minor = 0) OR (credit_minor > 0 AND debit_minor = 0))
);
CREATE INDEX hosted_accounting_journal_lines_journal ON hosted_accounting_journal_lines(tenant_id,branch_id,journal_id);

CREATE FUNCTION hosted_assert_balanced_journal() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    journal_tenant UUID;
    journal_branch UUID;
    journal_uuid UUID;
    debit_total NUMERIC;
    credit_total NUMERIC;
    line_count BIGINT;
BEGIN
    IF TG_TABLE_NAME='hosted_accounting_journals' THEN
        journal_tenant := NEW.tenant_id;
        journal_branch := NEW.branch_id;
        journal_uuid := NEW.id;
    ELSE
        journal_tenant := NEW.tenant_id;
        journal_branch := NEW.branch_id;
        journal_uuid := NEW.journal_id;
    END IF;
    SELECT COALESCE(SUM(debit_minor),0),COALESCE(SUM(credit_minor),0),COUNT(*)
      INTO debit_total,credit_total,line_count
      FROM hosted_accounting_journal_lines
      WHERE tenant_id=journal_tenant AND branch_id=journal_branch AND journal_id=journal_uuid;
    IF line_count < 2 OR debit_total <> credit_total THEN
        RAISE EXCEPTION 'Accounting journal must contain balanced debit and credit lines' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER hosted_accounting_journal_balance_header
AFTER INSERT ON hosted_accounting_journals DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION hosted_assert_balanced_journal();
CREATE CONSTRAINT TRIGGER hosted_accounting_journal_balance_line
AFTER INSERT ON hosted_accounting_journal_lines DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION hosted_assert_balanced_journal();

ALTER TABLE hosted_supplier_payments ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_payments FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_payment_allocations ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_payment_allocations FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_accounting_journals ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_accounting_journals FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_accounting_journal_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_accounting_journal_lines FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_supplier_payments_branch_read ON hosted_supplier_payments FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_supplier_payments.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_supplier_payments.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_supplier_payments_branch_insert ON hosted_supplier_payments FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_supplier_payments.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_supplier_payments.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_supplier_payment_allocations_branch_read ON hosted_supplier_payment_allocations FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_supplier_payments p WHERE p.tenant_id=hosted_supplier_payment_allocations.tenant_id AND p.id=hosted_supplier_payment_allocations.payment_id)
);
CREATE POLICY hosted_supplier_payment_allocations_branch_insert ON hosted_supplier_payment_allocations FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_supplier_payments p WHERE p.tenant_id=hosted_supplier_payment_allocations.tenant_id AND p.id=hosted_supplier_payment_allocations.payment_id)
    AND EXISTS (SELECT 1 FROM hosted_supplier_bills b WHERE b.tenant_id=hosted_supplier_payment_allocations.tenant_id AND b.branch_id=hosted_supplier_payment_allocations.branch_id AND b.id=hosted_supplier_payment_allocations.bill_id)
);
CREATE POLICY hosted_accounting_journals_branch_read ON hosted_accounting_journals FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_accounting_journals.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_accounting_journals.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_accounting_journals_branch_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND ((source_type='supplier_bill' AND EXISTS (SELECT 1 FROM hosted_supplier_bills b WHERE b.tenant_id=hosted_accounting_journals.tenant_id AND b.branch_id=hosted_accounting_journals.branch_id AND b.id=source_id)
          AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active' AND g.role_key IN ('owner','admin','manager') AND (g.branch_id IS NULL OR g.branch_id=hosted_accounting_journals.branch_id) AND g.station_id IS NULL))
      OR (source_type='supplier_payment' AND EXISTS (SELECT 1 FROM hosted_supplier_payments p WHERE p.tenant_id=hosted_accounting_journals.tenant_id AND p.branch_id=hosted_accounting_journals.branch_id AND p.id=source_id)
          AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active' AND g.role_key IN ('owner','admin') AND (g.branch_id IS NULL OR g.branch_id=hosted_accounting_journals.branch_id) AND g.station_id IS NULL)))
);
CREATE POLICY hosted_accounting_journal_lines_branch_read ON hosted_accounting_journal_lines FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_accounting_journals j WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id AND j.id=hosted_accounting_journal_lines.journal_id)
);
CREATE POLICY hosted_accounting_journal_lines_branch_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_accounting_journals j
      JOIN memberships m ON m.id=j.actor_membership_id AND m.tenant_id=j.tenant_id
      JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
        AND j.id=hosted_accounting_journal_lines.journal_id
        AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
        AND (g.branch_id IS NULL OR g.branch_id=j.branch_id) AND g.station_id IS NULL
        AND ((j.source_type='supplier_bill' AND g.role_key IN ('owner','admin','manager'))
          OR (j.source_type='supplier_payment' AND g.role_key IN ('owner','admin'))))
);
