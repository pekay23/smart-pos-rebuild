-- Explicit branch cash cutover. Historical physical balances are never inferred
-- from accounting journals; the owner records the measured total once, and any
-- opening adjustment is posted to opening equity with a retained explanation.
ALTER TABLE hosted_accounting_journals
  DROP CONSTRAINT hosted_accounting_journals_source_type_check,
  ADD CONSTRAINT hosted_accounting_journals_source_type_check
    CHECK (source_type IN ('supplier_bill','supplier_payment','expense','expense_reimbursement','sale','sale_refund','accounting_adjustment','cash_reconciliation'));

CREATE TABLE hosted_cash_reconciliations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL,
  branch_id UUID NOT NULL,
  actor_membership_id UUID NOT NULL,
  currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
  tracked_cash_minor BIGINT NOT NULL CHECK (tracked_cash_minor BETWEEN 0 AND 9007199254740991),
  ledger_cash_before_minor BIGINT NOT NULL CHECK (ledger_cash_before_minor BETWEEN -9007199254740991 AND 9007199254740991),
  adjustment_minor BIGINT NOT NULL CHECK (adjustment_minor BETWEEN -9007199254740991 AND 9007199254740991),
  reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 3 AND 200),
  idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
  request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
  FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
  UNIQUE (tenant_id,branch_id,id),
  UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key),
  CHECK (adjustment_minor=tracked_cash_minor-ledger_cash_before_minor)
);
CREATE INDEX hosted_cash_reconciliations_latest ON hosted_cash_reconciliations(tenant_id,branch_id,created_at DESC,id DESC);

ALTER TABLE hosted_cash_reconciliations ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_cash_reconciliations FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_cash_reconciliations_branch_read ON hosted_cash_reconciliations FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_cash_reconciliations.tenant_id AND m.status='active'
      AND g.role_key IN ('owner','admin','manager')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_reconciliations.branch_id) AND g.station_id IS NULL
  )
);
CREATE POLICY hosted_cash_reconciliations_owner_insert ON hosted_cash_reconciliations FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_cash_reconciliations.tenant_id AND m.status='active'
      AND g.role_key IN ('owner','admin')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_reconciliations.branch_id) AND g.station_id IS NULL
  )
);

CREATE POLICY hosted_accounting_journals_cash_reconciliation_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
  source_type='cash_reconciliation'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_cash_reconciliations r WHERE r.tenant_id=hosted_accounting_journals.tenant_id
    AND r.branch_id=hosted_accounting_journals.branch_id AND r.id=hosted_accounting_journals.source_id
    AND r.actor_membership_id=hosted_accounting_journals.actor_membership_id)
);
CREATE POLICY hosted_accounting_journal_lines_cash_reconciliation_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_accounting_journals j JOIN memberships m ON m.tenant_id=j.tenant_id AND m.id=j.actor_membership_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
      AND j.id=hosted_accounting_journal_lines.journal_id AND j.source_type='cash_reconciliation'
      AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key IN ('owner','admin') AND (g.branch_id IS NULL OR g.branch_id=j.branch_id) AND g.station_id IS NULL)
);

GRANT SELECT (id,tenant_id,branch_id,actor_membership_id,currency_code,tracked_cash_minor,ledger_cash_before_minor,adjustment_minor,reason,idempotency_key,request_hash,created_at)
  ON hosted_cash_reconciliations TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,actor_membership_id,currency_code,tracked_cash_minor,ledger_cash_before_minor,adjustment_minor,reason,idempotency_key,request_hash)
  ON hosted_cash_reconciliations TO pos_api_runtime;
