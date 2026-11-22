-- Post hosted sales and refunds into the same append-only accounting ledger
-- used by purchasing and expenses. Cost snapshots apply only to new sales.
ALTER TABLE hosted_sale_lines
  ADD COLUMN unit_cost_minor_snapshot BIGINT CHECK (unit_cost_minor_snapshot IS NULL OR unit_cost_minor_snapshot >= 0);

ALTER TABLE hosted_accounting_journals
  ADD COLUMN coverage_status TEXT NOT NULL DEFAULT 'complete'
    CHECK (coverage_status IN ('complete','partial')),
  DROP CONSTRAINT hosted_accounting_journals_source_type_check,
  ADD CONSTRAINT hosted_accounting_journals_source_type_check
    CHECK (source_type IN ('supplier_bill','supplier_payment','expense','expense_reimbursement','sale','sale_refund'));

CREATE POLICY hosted_accounting_journals_cashier_sale_read ON hosted_accounting_journals FOR SELECT USING (
  source_type='sale'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM hosted_sales s
    JOIN memberships m ON m.tenant_id=s.tenant_id AND m.id=s.cashier_membership_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE s.tenant_id=hosted_accounting_journals.tenant_id AND s.branch_id=hosted_accounting_journals.branch_id
      AND s.id=hosted_accounting_journals.source_id
      AND s.cashier_membership_id=hosted_accounting_journals.actor_membership_id
      AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=s.branch_id)
      AND (g.station_id IS NULL OR g.station_id=s.station_id))
);

CREATE POLICY hosted_accounting_journals_sale_refund_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND ((source_type='sale' AND EXISTS (
      SELECT 1 FROM hosted_sales s
      JOIN memberships m ON m.tenant_id=s.tenant_id AND m.id=s.cashier_membership_id
      JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE s.tenant_id=hosted_accounting_journals.tenant_id AND s.branch_id=hosted_accounting_journals.branch_id
        AND s.id=source_id AND s.cashier_membership_id=actor_membership_id
        AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
        AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=s.branch_id)
        AND (g.station_id IS NULL OR g.station_id=s.station_id)))
    OR (source_type='sale_refund' AND EXISTS (
      SELECT 1 FROM hosted_sale_refunds r
      JOIN memberships m ON m.tenant_id=r.tenant_id AND m.id=r.actor_membership_id
      JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE r.tenant_id=hosted_accounting_journals.tenant_id AND r.branch_id=hosted_accounting_journals.branch_id
        AND r.id=source_id AND r.actor_membership_id=actor_membership_id
        AND r.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=r.branch_id) AND g.station_id IS NULL)))
);

CREATE POLICY hosted_accounting_journal_lines_sale_refund_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM hosted_accounting_journals j
    JOIN memberships m ON m.tenant_id=j.tenant_id AND m.id=j.actor_membership_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
      AND j.id=hosted_accounting_journal_lines.journal_id
      AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND ((j.source_type='sale' AND g.role_key='cashier'
            AND EXISTS (SELECT 1 FROM hosted_sales s WHERE s.tenant_id=j.tenant_id AND s.branch_id=j.branch_id
              AND s.id=j.source_id AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
            AND (g.branch_id IS NULL OR g.branch_id=j.branch_id)
            AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
        OR (j.source_type='sale_refund' AND g.role_key IN ('owner','admin','manager')
            AND EXISTS (SELECT 1 FROM hosted_sale_refunds r WHERE r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id
              AND r.id=j.source_id AND r.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
            AND (g.branch_id IS NULL OR g.branch_id=j.branch_id) AND g.station_id IS NULL)))
);

-- New sale and refund rows must have exactly one balanced source journal at commit.
CREATE FUNCTION require_hosted_sale_accounting_journal() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE source_kind TEXT; journal_count INTEGER;
BEGIN
  IF TG_TABLE_NAME='hosted_sales' THEN source_kind:='sale'; ELSE source_kind:='sale_refund'; END IF;
  SELECT count(*) INTO journal_count FROM hosted_accounting_journals
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_type=source_kind AND source_id=NEW.id;
  IF journal_count<>1 THEN RAISE EXCEPTION 'hosted sale source requires exactly one accounting journal' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_sale_accounting_journal_complete
  AFTER INSERT ON hosted_sales DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION require_hosted_sale_accounting_journal();
CREATE CONSTRAINT TRIGGER hosted_sale_refund_accounting_journal_complete
  AFTER INSERT ON hosted_sale_refunds DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION require_hosted_sale_accounting_journal();

GRANT SELECT (unit_cost_minor_snapshot) ON hosted_sale_lines TO pos_api_runtime;
GRANT INSERT (unit_cost_minor_snapshot) ON hosted_sale_lines TO pos_api_runtime;
GRANT SELECT (cost_minor) ON branch_catalog_items TO pos_api_runtime;
GRANT SELECT (coverage_status) ON hosted_accounting_journals TO pos_api_runtime;
GRANT INSERT (coverage_status) ON hosted_accounting_journals TO pos_api_runtime;
