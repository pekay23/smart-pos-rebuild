-- A station-recorded cash repayment is linked to its drawer, register cash
-- subledger and balanced accounting journal in the same transaction.

ALTER TABLE station_financial_proof_challenges
  DROP CONSTRAINT station_financial_proof_challenges_operation_check,
  ADD CONSTRAINT station_financial_proof_challenges_operation_check
    CHECK (operation IN ('sale.commit','sale.refund','drawer.open','drawer.movement','drawer.close','supplier.payment','expense.payment','receivable.collection'));

ALTER TABLE hosted_receivable_collections
  ADD COLUMN station_id UUID,
  ADD COLUMN drawer_shift_id UUID,
  ADD CONSTRAINT hosted_receivable_collection_station_fk
    FOREIGN KEY (station_id,branch_id,tenant_id) REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
  ADD CONSTRAINT hosted_receivable_collection_drawer_fk
    FOREIGN KEY (tenant_id,branch_id,station_id,drawer_shift_id)
    REFERENCES hosted_drawer_shifts(tenant_id,branch_id,station_id,id) ON DELETE RESTRICT,
  ADD CONSTRAINT hosted_receivable_collection_station_pair
    CHECK ((station_id IS NULL)=(drawer_shift_id IS NULL)),
  ADD CONSTRAINT hosted_receivable_collection_station_cash_only
    CHECK (station_id IS NULL OR (payment_method='cash' AND confirmation_state='cash_recorded'));

ALTER TABLE hosted_drawer_movements
  ADD COLUMN source_receivable_collection_id UUID,
  ADD CONSTRAINT hosted_drawer_movement_receivable_collection_fk
    FOREIGN KEY (tenant_id,branch_id,source_receivable_collection_id)
    REFERENCES hosted_receivable_collections(tenant_id,branch_id,id) ON DELETE RESTRICT,
  ADD CONSTRAINT hosted_drawer_movement_receivable_is_paid_in
    CHECK (source_receivable_collection_id IS NULL OR movement_type='paid_in');
CREATE UNIQUE INDEX hosted_drawer_receivable_collection_once
  ON hosted_drawer_movements(tenant_id,source_receivable_collection_id)
  WHERE source_receivable_collection_id IS NOT NULL;
GRANT SELECT (source_receivable_collection_id) ON hosted_drawer_movements TO pos_api_runtime;
GRANT INSERT (source_receivable_collection_id) ON hosted_drawer_movements TO pos_api_runtime;

ALTER TABLE hosted_cash_location_events
  ADD COLUMN source_receivable_collection_id UUID,
  ADD CONSTRAINT hosted_cash_event_receivable_collection_fk
    FOREIGN KEY (tenant_id,branch_id,source_receivable_collection_id)
    REFERENCES hosted_receivable_collections(tenant_id,branch_id,id) ON DELETE RESTRICT,
  DROP CONSTRAINT hosted_cash_location_event_type_check,
  DROP CONSTRAINT hosted_cash_location_event_shape_check;
ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow','expense_reimbursement_inflow','sale_tender_inflow','sale_refund_outflow','register_movement_inflow','register_movement_outflow','cash_drop_inflow','receivable_collection_inflow')),
  ADD CONSTRAINT hosted_cash_location_event_shape_check CHECK (
    (event_type='opening' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor>=0)
    OR (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor<0)
    OR (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor>0)
    OR (event_type='expense_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NOT NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor<0)
    OR (event_type='supplier_payment_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NOT NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor<0)
    OR (event_type='expense_reimbursement_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NOT NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor>0)
    OR (event_type='sale_tender_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NOT NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor>0)
    OR (event_type='sale_refund_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NOT NULL AND source_drawer_movement_id IS NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor<0)
    OR (event_type='register_movement_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor>0)
    OR (event_type='register_movement_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor<0)
    OR (event_type='cash_drop_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND source_receivable_collection_id IS NULL AND signed_amount_minor>0)
    OR (event_type='receivable_collection_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND source_receivable_collection_id IS NOT NULL AND signed_amount_minor>0)
  );
CREATE UNIQUE INDEX hosted_cash_event_receivable_collection_once
  ON hosted_cash_location_events(tenant_id,source_receivable_collection_id)
  WHERE source_receivable_collection_id IS NOT NULL;
GRANT SELECT (source_receivable_collection_id) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (source_receivable_collection_id) ON hosted_cash_location_events TO pos_api_runtime;

CREATE POLICY hosted_cash_events_cashier_collection_inflow_insert ON hosted_cash_location_events FOR INSERT TO pos_api_runtime WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type='receivable_collection_inflow'
  AND source_drawer_movement_id IS NOT NULL
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN hosted_drawer_movements d ON d.tenant_id=hosted_cash_location_events.tenant_id
      AND d.branch_id=hosted_cash_location_events.branch_id AND d.id=hosted_cash_location_events.source_drawer_movement_id
    JOIN hosted_receivable_collections c ON c.tenant_id=d.tenant_id AND c.branch_id=d.branch_id
      AND c.id=hosted_cash_location_events.source_receivable_collection_id AND c.id=d.source_receivable_collection_id
    JOIN memberships m ON m.id=actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type='register' AND l.status='active'
      AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND g.station_id=l.station_id
      AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND d.station_id=l.station_id AND d.movement_type='paid_in' AND d.actor_membership_id=actor_membership_id
      AND d.amount_minor=signed_amount_minor AND c.station_id=l.station_id AND c.drawer_shift_id=d.shift_id
      AND c.recorded_by_membership_id=actor_membership_id AND c.amount_minor=signed_amount_minor
      AND c.payment_method='cash' AND c.confirmation_state='cash_recorded')
);
CREATE POLICY hosted_cash_events_cashier_collection_inflow_read ON hosted_cash_location_events FOR SELECT TO pos_api_runtime USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type='receivable_collection_inflow'
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type='register'
      AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND g.station_id=l.station_id
      AND (g.branch_id IS NULL OR g.branch_id=l.branch_id))
);

CREATE OR REPLACE FUNCTION check_hosted_drawer_movement_cash_location_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  register_id UUID;
  initialized BOOLEAN;
  expected_event_type TEXT;
  expected_amount BIGINT;
  expected_expense_id UUID;
  expected_payment_id UUID;
  expected_collection_id UUID;
  matching_count INTEGER;
  event_row hosted_cash_location_events%ROWTYPE;
  destination_initialized BOOLEAN;
BEGIN
  SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
    INTO register_id,initialized FROM hosted_cash_locations l
    WHERE l.tenant_id=NEW.tenant_id AND l.branch_id=NEW.branch_id AND l.station_id=NEW.station_id
      AND l.location_type='register' AND l.status='active';
  IF NOT FOUND THEN
    IF NEW.cash_destination_location_id IS NOT NULL THEN
      RAISE EXCEPTION 'cash drop destination requires an initialized register location' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
  END IF;
  IF NOT initialized THEN RAISE EXCEPTION 'register cash location requires a cutover count' USING ERRCODE='23514'; END IF;

  IF NEW.source_expense_id IS NOT NULL THEN
    expected_event_type:='expense_outflow'; expected_amount:=-NEW.amount_minor; expected_expense_id:=NEW.source_expense_id;
  ELSIF NEW.source_payment_id IS NOT NULL THEN
    expected_event_type:='supplier_payment_outflow'; expected_amount:=-NEW.amount_minor; expected_payment_id:=NEW.source_payment_id;
  ELSIF NEW.source_refund_id IS NOT NULL THEN
    RETURN NULL;
  ELSIF NEW.source_receivable_collection_id IS NOT NULL THEN
    expected_event_type:='receivable_collection_inflow'; expected_amount:=NEW.amount_minor; expected_collection_id:=NEW.source_receivable_collection_id;
  ELSIF NEW.movement_type='paid_in' THEN
    expected_event_type:='register_movement_inflow'; expected_amount:=NEW.amount_minor;
  ELSE
    expected_event_type:='register_movement_outflow'; expected_amount:=-NEW.amount_minor;
  END IF;

  SELECT count(*) INTO matching_count FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id
      AND event_type=expected_event_type
      AND source_expense_id IS NOT DISTINCT FROM expected_expense_id
      AND source_supplier_payment_id IS NOT DISTINCT FROM expected_payment_id
      AND source_receivable_collection_id IS NOT DISTINCT FROM expected_collection_id;
  IF matching_count<>1 THEN RAISE EXCEPTION 'drawer movement requires one matching register cash event' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id
    AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id AND event_type=expected_event_type;
  IF event_row.location_id<>register_id OR event_row.signed_amount_minor<>expected_amount
     OR event_row.actor_membership_id<>NEW.actor_membership_id THEN
    RAISE EXCEPTION 'drawer movement register cash event does not match its source' USING ERRCODE='23514';
  END IF;

  IF NEW.movement_type='cash_drop' THEN
    IF NEW.cash_destination_location_id IS NULL THEN RAISE EXCEPTION 'cash drop requires a destination location' USING ERRCODE='23514'; END IF;
    SELECT EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id
      AND e.branch_id=l.branch_id AND e.location_id=l.id AND e.event_type='opening')
      INTO destination_initialized FROM hosted_cash_locations l
      WHERE l.tenant_id=NEW.tenant_id AND l.branch_id=NEW.branch_id
        AND l.id=NEW.cash_destination_location_id AND l.location_type<>'register' AND l.status='active';
    IF NOT FOUND OR NOT destination_initialized THEN RAISE EXCEPTION 'cash drop destination must be active and initialized' USING ERRCODE='23514'; END IF;
    SELECT count(*) INTO matching_count FROM hosted_cash_location_events
      WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id
        AND event_type='cash_drop_inflow' AND location_id=NEW.cash_destination_location_id
        AND signed_amount_minor=NEW.amount_minor AND actor_membership_id=NEW.actor_membership_id;
    IF matching_count<>1 THEN RAISE EXCEPTION 'cash drop requires one matching destination inflow' USING ERRCODE='23514'; END IF;
  ELSIF NEW.cash_destination_location_id IS NOT NULL THEN
    RAISE EXCEPTION 'only a cash drop may specify a destination location' USING ERRCODE='23514';
  END IF;
  RETURN NULL;
END $$;

ALTER TABLE hosted_accounting_journals
  DROP CONSTRAINT hosted_accounting_journals_source_type_check,
  ADD CONSTRAINT hosted_accounting_journals_source_type_check
    CHECK (source_type IN ('supplier_bill','supplier_payment','supplier_credit','expense','expense_reimbursement','sale','sale_refund','accounting_adjustment','cash_reconciliation','receivable_collection'));

CREATE POLICY hosted_accounting_journals_receivable_collection_station_read ON hosted_accounting_journals FOR SELECT TO pos_api_runtime USING (
  source_type='receivable_collection'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_receivable_collections c WHERE c.tenant_id=hosted_accounting_journals.tenant_id
    AND c.branch_id=hosted_accounting_journals.branch_id AND c.id=hosted_accounting_journals.source_id
    AND c.recorded_by_membership_id=hosted_accounting_journals.actor_membership_id
    AND c.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
);
CREATE POLICY hosted_receivable_collections_station_read ON hosted_receivable_collections FOR SELECT TO pos_api_runtime USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
  AND recorded_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND confirmation_state='cash_recorded'
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=recorded_by_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_receivable_collections.tenant_id AND m.status='active' AND g.role_key='cashier'
      AND g.station_id=hosted_receivable_collections.station_id
      AND (g.branch_id IS NULL OR g.branch_id=hosted_receivable_collections.branch_id))
);
CREATE POLICY hosted_receivable_collections_station_insert ON hosted_receivable_collections FOR INSERT TO pos_api_runtime WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
  AND recorded_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND payment_method='cash' AND confirmation_state='cash_recorded'
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=recorded_by_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_receivable_collections.tenant_id AND m.status='active' AND g.role_key='cashier'
      AND g.station_id=hosted_receivable_collections.station_id
      AND (g.branch_id IS NULL OR g.branch_id=hosted_receivable_collections.branch_id))
);
CREATE POLICY hosted_receivable_allocations_station_read ON hosted_receivable_allocations FOR SELECT TO pos_api_runtime USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND collection_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_receivable_collections c WHERE c.tenant_id=hosted_receivable_allocations.tenant_id
    AND c.branch_id=hosted_receivable_allocations.collection_branch_id AND c.id=hosted_receivable_allocations.collection_id
    AND c.recorded_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND c.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
);
CREATE POLICY hosted_receivable_allocations_station_insert ON hosted_receivable_allocations FOR INSERT TO pos_api_runtime WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND collection_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_receivable_collections c WHERE c.tenant_id=hosted_receivable_allocations.tenant_id
    AND c.branch_id=hosted_receivable_allocations.collection_branch_id AND c.id=hosted_receivable_allocations.collection_id
    AND c.recorded_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND c.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
    AND c.customer_id=hosted_receivable_allocations.customer_id AND c.currency_code=hosted_receivable_allocations.currency_code)
);

CREATE POLICY hosted_accounting_journals_receivable_collection_insert ON hosted_accounting_journals FOR INSERT TO pos_api_runtime WITH CHECK (
  source_type='receivable_collection'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_receivable_collections c WHERE c.tenant_id=hosted_accounting_journals.tenant_id
    AND c.branch_id=hosted_accounting_journals.branch_id AND c.id=hosted_accounting_journals.source_id
    AND c.recorded_by_membership_id=hosted_accounting_journals.actor_membership_id
    AND c.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
);
CREATE POLICY hosted_accounting_journal_lines_receivable_collection_insert ON hosted_accounting_journal_lines FOR INSERT TO pos_api_runtime WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM hosted_accounting_journals j
    JOIN memberships m ON m.id=j.actor_membership_id AND m.tenant_id=j.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
      AND j.id=hosted_accounting_journal_lines.journal_id AND j.source_type='receivable_collection'
      AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=j.branch_id)
      AND g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
);

GRANT SELECT (id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,external_reference,recorded_by_membership_id,idempotency_key,request_hash,received_at,created_at,station_id,drawer_shift_id)
  ON hosted_receivable_collections TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,external_reference,recorded_by_membership_id,idempotency_key,request_hash,station_id,drawer_shift_id)
  ON hosted_receivable_collections TO pos_api_runtime;
GRANT SELECT (id,tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor,created_at)
  ON hosted_receivable_allocations TO pos_api_runtime;
GRANT INSERT (tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor)
  ON hosted_receivable_allocations TO pos_api_runtime;

GRANT SELECT (source_receivable_collection_id) ON hosted_cash_location_events TO pos_receivables_worker;
CREATE FUNCTION check_hosted_receivable_collection_cash_links() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  movement hosted_drawer_movements%ROWTYPE;
  movement_count INTEGER;
  journal_count INTEGER;
BEGIN
  IF NEW.station_id IS NULL THEN RETURN NULL; END IF;
  SELECT count(*) INTO movement_count FROM hosted_drawer_movements d
   WHERE d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id
     AND d.source_receivable_collection_id=NEW.id;
  IF movement_count<>1 THEN RAISE EXCEPTION 'station cash collection requires one drawer inflow' USING ERRCODE='23514'; END IF;
  SELECT * INTO movement FROM hosted_drawer_movements d
   WHERE d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id
     AND d.source_receivable_collection_id=NEW.id;
  IF movement.station_id<>NEW.station_id OR movement.shift_id<>NEW.drawer_shift_id
     OR movement.actor_membership_id<>NEW.recorded_by_membership_id
     OR movement.movement_type<>'paid_in' OR movement.amount_minor<>NEW.amount_minor THEN
    RAISE EXCEPTION 'drawer inflow does not match its customer collection' USING ERRCODE='23514';
  END IF;
  SELECT count(*) INTO journal_count FROM hosted_accounting_journals j
   WHERE j.tenant_id=NEW.tenant_id AND j.branch_id=NEW.branch_id
     AND j.source_type='receivable_collection' AND j.source_id=NEW.id;
  IF journal_count<>1 THEN RAISE EXCEPTION 'cash collection requires one accounting journal' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_receivable_collection_cash_links_complete
  AFTER INSERT ON hosted_receivable_collections DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_receivable_collection_cash_links();

GRANT SELECT (station_id,shift_id,source_receivable_collection_id,amount_minor,movement_type,actor_membership_id)
  ON hosted_drawer_movements TO pos_receivables_worker;
GRANT SELECT (tenant_id,branch_id,source_type,source_id)
  ON hosted_accounting_journals TO pos_receivables_worker;
