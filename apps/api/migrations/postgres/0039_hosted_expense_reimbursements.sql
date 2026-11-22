-- Record external expense repayments as immutable sources and cash-location inflows.
CREATE TABLE hosted_expense_reimbursements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    expense_id UUID NOT NULL,
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    amount_minor BIGINT NOT NULL CHECK (amount_minor BETWEEN 1 AND 9007199254740991),
    return_method TEXT NOT NULL CHECK (return_method IN ('cash','card','mobile_money','bank_transfer','other')),
    cash_location_id UUID,
    external_reference TEXT,
    reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 5 AND 500),
    returned_on DATE NOT NULL,
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,expense_id) REFERENCES hosted_expenses(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,cash_location_id) REFERENCES hosted_cash_locations(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    CHECK (
      (return_method='cash' AND cash_location_id IS NOT NULL AND external_reference IS NULL)
      OR (return_method<>'cash' AND cash_location_id IS NULL AND external_reference IS NOT NULL AND length(btrim(external_reference)) BETWEEN 1 AND 160)
    ),
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key)
);
CREATE INDEX hosted_expense_reimbursements_source ON hosted_expense_reimbursements(tenant_id,branch_id,expense_id,created_at,id);
CREATE UNIQUE INDEX hosted_expense_reimbursements_reference
  ON hosted_expense_reimbursements(tenant_id,branch_id,return_method,external_reference)
  WHERE external_reference IS NOT NULL;

ALTER TABLE hosted_accounting_journals DROP CONSTRAINT hosted_accounting_journals_source_type_check;
ALTER TABLE hosted_accounting_journals ADD CONSTRAINT hosted_accounting_journals_source_type_check
  CHECK (source_type IN ('supplier_bill','supplier_payment','expense','expense_reimbursement'));

ALTER TABLE hosted_cash_location_events
  ADD COLUMN source_expense_reimbursement_id UUID,
  ADD CONSTRAINT hosted_cash_event_expense_reimbursement_fk
    FOREIGN KEY (tenant_id,branch_id,source_expense_reimbursement_id)
    REFERENCES hosted_expense_reimbursements(tenant_id,branch_id,id) ON DELETE RESTRICT,
  DROP CONSTRAINT hosted_cash_location_event_type_check,
  DROP CONSTRAINT hosted_cash_location_event_shape_check;
ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow','expense_reimbursement_inflow')),
  ADD CONSTRAINT hosted_cash_location_event_shape_check
    CHECK (
      (event_type='opening' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND signed_amount_minor>=0)
      OR (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND signed_amount_minor<0)
      OR (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND signed_amount_minor>0)
      OR (event_type='expense_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NOT NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND signed_amount_minor<0)
      OR (event_type='supplier_payment_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NOT NULL AND source_expense_reimbursement_id IS NULL AND signed_amount_minor<0)
      OR (event_type='expense_reimbursement_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NOT NULL AND signed_amount_minor>0)
    );
CREATE UNIQUE INDEX hosted_cash_event_expense_reimbursement_once
  ON hosted_cash_location_events(tenant_id,source_expense_reimbursement_id)
  WHERE source_expense_reimbursement_id IS NOT NULL;

CREATE FUNCTION check_hosted_expense_reimbursement_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE event_row hosted_cash_location_events%ROWTYPE; event_count INTEGER;
BEGIN
  IF NEW.return_method<>'cash' THEN RETURN NULL; END IF;
  SELECT count(*) INTO event_count FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id
      AND source_expense_reimbursement_id=NEW.id AND event_type='expense_reimbursement_inflow';
  IF event_count<>1 THEN RAISE EXCEPTION 'cash expense reimbursement requires one location inflow' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id
      AND source_expense_reimbursement_id=NEW.id AND event_type='expense_reimbursement_inflow';
  IF event_row.location_id<>NEW.cash_location_id OR event_row.signed_amount_minor<>NEW.amount_minor
     OR event_row.actor_membership_id<>NEW.actor_membership_id
  THEN RAISE EXCEPTION 'cash expense reimbursement inflow does not match its source' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_expense_reimbursement_event_complete
  AFTER INSERT ON hosted_expense_reimbursements DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_expense_reimbursement_event();

ALTER TABLE hosted_expense_reimbursements ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_expense_reimbursements FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_expense_reimbursements_branch_read ON hosted_expense_reimbursements FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_expense_reimbursements.tenant_id AND m.status='active'
      AND g.role_key IN ('owner','admin','manager')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_expense_reimbursements.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_expense_reimbursements_branch_insert ON hosted_expense_reimbursements FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_expense_reimbursements.tenant_id AND m.status='active'
      AND g.role_key IN ('owner','admin','manager')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_expense_reimbursements.branch_id) AND g.station_id IS NULL)
);

CREATE POLICY hosted_accounting_journals_expense_reimbursement_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
  source_type='expense_reimbursement'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_expense_reimbursements r WHERE r.tenant_id=hosted_accounting_journals.tenant_id
    AND r.branch_id=hosted_accounting_journals.branch_id AND r.id=source_id)
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.status='active' AND g.role_key IN ('owner','admin','manager')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_accounting_journals.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_accounting_journal_lines_expense_reimbursement_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_accounting_journals j
    JOIN hosted_expense_reimbursements r ON r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id AND r.id=j.source_id
    JOIN memberships m ON m.id=j.actor_membership_id AND m.tenant_id=j.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
      AND j.id=hosted_accounting_journal_lines.journal_id AND j.source_type='expense_reimbursement'
      AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key IN ('owner','admin','manager')
      AND (g.branch_id IS NULL OR g.branch_id=j.branch_id) AND g.station_id IS NULL)
);

GRANT SELECT (id,tenant_id,branch_id,expense_id,currency_code,amount_minor,return_method,cash_location_id,external_reference,reason,returned_on,actor_membership_id,idempotency_key,request_hash,created_at)
  ON hosted_expense_reimbursements TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,expense_id,currency_code,amount_minor,return_method,cash_location_id,external_reference,reason,returned_on,actor_membership_id,idempotency_key,request_hash)
  ON hosted_expense_reimbursements TO pos_api_runtime;
GRANT SELECT (source_expense_reimbursement_id) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (source_expense_reimbursement_id) ON hosted_cash_location_events TO pos_api_runtime;
