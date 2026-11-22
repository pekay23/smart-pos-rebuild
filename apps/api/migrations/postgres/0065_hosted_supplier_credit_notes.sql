-- Physical supplier returns reduce stock and open payables through immutable credits.
ALTER TABLE hosted_accounting_journals
  DROP CONSTRAINT hosted_accounting_journals_source_type_check,
  ADD CONSTRAINT hosted_accounting_journals_source_type_check
    CHECK (source_type IN ('supplier_bill','supplier_payment','supplier_credit','expense','expense_reimbursement','sale','sale_refund','accounting_adjustment','cash_reconciliation'));

CREATE UNIQUE INDEX hosted_purchase_receipt_lines_return_source
  ON hosted_purchase_receipt_lines(tenant_id,branch_id,receipt_id,id,product_id);

CREATE TABLE hosted_supplier_credit_notes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL,
  branch_id UUID NOT NULL,
  bill_id UUID NOT NULL,
  supplier_id UUID NOT NULL,
  credit_reference TEXT NOT NULL CHECK (length(btrim(credit_reference)) BETWEEN 1 AND 120),
  reference_key TEXT NOT NULL,
  effective_date DATE NOT NULL,
  reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 500),
  currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
  total_minor BIGINT NOT NULL CHECK (total_minor BETWEEN 1 AND 9007199254740991),
  actor_membership_id UUID NOT NULL,
  idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
  request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (tenant_id,branch_id,bill_id) REFERENCES hosted_supplier_bills(tenant_id,branch_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,supplier_id) REFERENCES hosted_suppliers(tenant_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
  UNIQUE (tenant_id,branch_id,id),
  UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key),
  UNIQUE (tenant_id,branch_id,supplier_id,reference_key)
);
CREATE INDEX hosted_supplier_credit_notes_bill ON hosted_supplier_credit_notes(tenant_id,branch_id,bill_id,created_at DESC,id);

CREATE TABLE hosted_supplier_credit_lines (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL,
  branch_id UUID NOT NULL,
  credit_note_id UUID NOT NULL,
  receipt_id UUID NOT NULL,
  receipt_line_id UUID NOT NULL,
  product_id UUID NOT NULL,
  quantity BIGINT NOT NULL CHECK (quantity BETWEEN 1 AND 1000000),
  unit_cost_minor BIGINT NOT NULL CHECK (unit_cost_minor BETWEEN 1 AND 9007199254740991),
  amount_minor BIGINT NOT NULL CHECK (amount_minor BETWEEN 1 AND 9007199254740991),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  CHECK (amount_minor=quantity*unit_cost_minor),
  FOREIGN KEY (tenant_id,branch_id,credit_note_id) REFERENCES hosted_supplier_credit_notes(tenant_id,branch_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,branch_id,receipt_id) REFERENCES hosted_purchase_receipts(tenant_id,branch_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,branch_id,receipt_id,receipt_line_id,product_id) REFERENCES hosted_purchase_receipt_lines(tenant_id,branch_id,receipt_id,id,product_id) ON DELETE RESTRICT,
  FOREIGN KEY (tenant_id,branch_id,product_id) REFERENCES branch_catalog_items(tenant_id,branch_id,product_id) ON DELETE RESTRICT,
  UNIQUE (tenant_id,credit_note_id,receipt_line_id)
);
CREATE INDEX hosted_supplier_credit_lines_receipt ON hosted_supplier_credit_lines(tenant_id,branch_id,receipt_id,receipt_line_id);

CREATE FUNCTION hosted_supplier_credit_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Supplier credit notes and lines are immutable' USING ERRCODE='55000';
END;
$$;
CREATE TRIGGER hosted_supplier_credit_notes_no_update_delete
  BEFORE UPDATE OR DELETE ON hosted_supplier_credit_notes
  FOR EACH ROW EXECUTE FUNCTION hosted_supplier_credit_immutable();
CREATE TRIGGER hosted_supplier_credit_lines_no_update_delete
  BEFORE UPDATE OR DELETE ON hosted_supplier_credit_lines
  FOR EACH ROW EXECUTE FUNCTION hosted_supplier_credit_immutable();

CREATE FUNCTION hosted_supplier_credit_total_matches_lines() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE credit_id UUID;
DECLARE expected_total BIGINT;
DECLARE line_total BIGINT;
BEGIN
  IF TG_TABLE_NAME='hosted_supplier_credit_notes' THEN
    credit_id := NEW.id;
  ELSE
    credit_id := NEW.credit_note_id;
  END IF;
  SELECT c.total_minor,COALESCE(SUM(l.amount_minor),0)::bigint
    INTO expected_total,line_total
    FROM hosted_supplier_credit_notes c
    LEFT JOIN hosted_supplier_credit_lines l
      ON l.tenant_id=c.tenant_id AND l.branch_id=c.branch_id AND l.credit_note_id=c.id
    WHERE c.id=credit_id
    GROUP BY c.total_minor;
  IF expected_total IS NULL OR expected_total<>line_total THEN
    RAISE EXCEPTION 'Supplier credit note total must equal its immutable lines' USING ERRCODE='23514';
  END IF;
  RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER hosted_supplier_credit_note_total_matches_lines
  AFTER INSERT ON hosted_supplier_credit_notes DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION hosted_supplier_credit_total_matches_lines();
CREATE CONSTRAINT TRIGGER hosted_supplier_credit_line_total_matches_note
  AFTER INSERT ON hosted_supplier_credit_lines DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION hosted_supplier_credit_total_matches_lines();

ALTER TABLE hosted_supplier_credit_notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_credit_notes FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_credit_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_credit_lines FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_supplier_credit_notes_branch_read ON hosted_supplier_credit_notes FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_supplier_credit_notes.tenant_id AND m.status='active'
      AND g.role_key IN ('owner','admin','manager')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_supplier_credit_notes.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_supplier_credit_notes_owner_insert ON hosted_supplier_credit_notes FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_supplier_credit_notes.tenant_id AND m.status='active'
      AND g.role_key IN ('owner','admin')
      AND (g.branch_id IS NULL OR g.branch_id=hosted_supplier_credit_notes.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_supplier_credit_lines_branch_read ON hosted_supplier_credit_lines FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_supplier_credit_notes c WHERE c.tenant_id=hosted_supplier_credit_lines.tenant_id
    AND c.branch_id=hosted_supplier_credit_lines.branch_id AND c.id=hosted_supplier_credit_lines.credit_note_id)
);
CREATE POLICY hosted_supplier_credit_lines_owner_insert ON hosted_supplier_credit_lines FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_supplier_credit_notes c
    JOIN memberships m ON m.id=c.actor_membership_id AND m.tenant_id=c.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE c.tenant_id=hosted_supplier_credit_lines.tenant_id AND c.branch_id=hosted_supplier_credit_lines.branch_id
      AND c.id=hosted_supplier_credit_lines.credit_note_id
      AND c.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key IN ('owner','admin') AND (g.branch_id IS NULL OR g.branch_id=c.branch_id) AND g.station_id IS NULL)
);

CREATE POLICY hosted_accounting_journals_supplier_credit_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
  source_type='supplier_credit'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_supplier_credit_notes c WHERE c.tenant_id=hosted_accounting_journals.tenant_id
    AND c.branch_id=hosted_accounting_journals.branch_id AND c.id=hosted_accounting_journals.source_id
    AND c.actor_membership_id=hosted_accounting_journals.actor_membership_id)
);
CREATE POLICY hosted_accounting_journal_lines_supplier_credit_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_accounting_journals j
    JOIN memberships m ON m.id=j.actor_membership_id AND m.tenant_id=j.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
      AND j.id=hosted_accounting_journal_lines.journal_id AND j.source_type='supplier_credit'
      AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key IN ('owner','admin') AND (g.branch_id IS NULL OR g.branch_id=j.branch_id) AND g.station_id IS NULL)
);

GRANT SELECT (id,tenant_id,branch_id,bill_id,supplier_id,credit_reference,reference_key,effective_date,reason,currency_code,total_minor,actor_membership_id,idempotency_key,request_hash,created_at)
  ON hosted_supplier_credit_notes TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,bill_id,supplier_id,credit_reference,reference_key,effective_date,reason,currency_code,total_minor,actor_membership_id,idempotency_key,request_hash)
  ON hosted_supplier_credit_notes TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,credit_note_id,receipt_id,receipt_line_id,product_id,quantity,unit_cost_minor,amount_minor,created_at)
  ON hosted_supplier_credit_lines TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,credit_note_id,receipt_id,receipt_line_id,product_id,quantity,unit_cost_minor,amount_minor)
  ON hosted_supplier_credit_lines TO pos_api_runtime;
