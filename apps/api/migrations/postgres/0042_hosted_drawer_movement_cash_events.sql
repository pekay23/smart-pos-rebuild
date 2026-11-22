-- Link non-source cash movements to the register cash-location subledger.
-- Drawer movements remain authoritative for X/Z reports.
ALTER TABLE hosted_drawer_movements
  ADD CONSTRAINT hosted_drawer_movement_branch_id_unique UNIQUE (tenant_id,branch_id,id);

ALTER TABLE hosted_cash_location_events
  ADD COLUMN source_drawer_movement_id UUID,
  ADD CONSTRAINT hosted_cash_event_drawer_movement_fk
    FOREIGN KEY (tenant_id,branch_id,source_drawer_movement_id)
    REFERENCES hosted_drawer_movements(tenant_id,branch_id,id) ON DELETE RESTRICT,
  DROP CONSTRAINT hosted_cash_location_event_type_check,
  DROP CONSTRAINT hosted_cash_location_event_shape_check;

ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow','expense_reimbursement_inflow','sale_tender_inflow','sale_refund_outflow','register_movement_inflow','register_movement_outflow')),
  ADD CONSTRAINT hosted_cash_location_event_shape_check CHECK (
    (event_type='opening' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND signed_amount_minor>=0)
    OR (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND signed_amount_minor<0)
    OR (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND signed_amount_minor>0)
    OR (event_type='expense_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NOT NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor<0)
    OR (event_type='supplier_payment_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NOT NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor<0)
    OR (event_type='expense_reimbursement_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NOT NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND signed_amount_minor>0)
    OR (event_type='sale_tender_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NOT NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NULL AND signed_amount_minor>0)
    OR (event_type='sale_refund_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NOT NULL AND signed_amount_minor<0)
    OR (event_type='register_movement_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND signed_amount_minor>0)
    OR (event_type='register_movement_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND signed_amount_minor<0)
  );
CREATE UNIQUE INDEX hosted_cash_event_drawer_movement_once
  ON hosted_cash_location_events(tenant_id,source_drawer_movement_id)
  WHERE source_drawer_movement_id IS NOT NULL;

CREATE POLICY hosted_cash_events_cashier_drawer_movement_insert ON hosted_cash_location_events FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type IN ('register_movement_inflow','register_movement_outflow')
  AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL
  AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL
  AND EXISTS (
    SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    JOIN hosted_drawer_movements d ON d.tenant_id=hosted_cash_location_events.tenant_id
      AND d.branch_id=hosted_cash_location_events.branch_id AND d.id=hosted_cash_location_events.source_drawer_movement_id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type='register' AND l.status='active'
      AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND (g.station_id IS NULL OR g.station_id=l.station_id)
      AND d.station_id=l.station_id AND d.actor_membership_id=hosted_cash_location_events.actor_membership_id
      AND d.source_payment_id IS NULL AND d.source_expense_id IS NULL AND d.source_refund_id IS NULL
      AND ((event_type='register_movement_inflow' AND d.movement_type='paid_in' AND d.amount_minor=signed_amount_minor)
        OR (event_type='register_movement_outflow' AND d.movement_type IN ('paid_out','cash_drop') AND d.amount_minor=-signed_amount_minor))
  )
);
CREATE POLICY hosted_cash_events_cashier_drawer_movement_read ON hosted_cash_location_events FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type IN ('register_movement_inflow','register_movement_outflow')
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type='register'
      AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND (g.station_id IS NULL OR g.station_id=l.station_id))
);

CREATE FUNCTION check_hosted_drawer_movement_cash_location_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE location_id UUID; initialized BOOLEAN; expected_event_type TEXT; signed_amount BIGINT; event_count INTEGER; event_row hosted_cash_location_events%ROWTYPE;
BEGIN
  -- Source-specific paid-outs are linked by their expense/supplier/refund events.
  IF NEW.source_payment_id IS NOT NULL OR NEW.source_expense_id IS NOT NULL OR NEW.source_refund_id IS NOT NULL THEN RETURN NULL; END IF;
  SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
    INTO location_id,initialized FROM hosted_cash_locations l
    WHERE l.tenant_id=NEW.tenant_id AND l.branch_id=NEW.branch_id AND l.station_id=NEW.station_id AND l.location_type='register' AND l.status='active';
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF NOT initialized THEN RAISE EXCEPTION 'register cash location requires a cutover count' USING ERRCODE='23514'; END IF;
  IF NEW.movement_type='paid_in' THEN expected_event_type:='register_movement_inflow'; signed_amount:=NEW.amount_minor;
  ELSE expected_event_type:='register_movement_outflow'; signed_amount:=-NEW.amount_minor; END IF;
  SELECT count(*) INTO event_count FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id AND event_type=expected_event_type;
  IF event_count<>1 THEN RAISE EXCEPTION 'drawer movement requires one register cash event' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id;
  IF event_row.location_id<>location_id OR event_row.signed_amount_minor<>signed_amount OR event_row.actor_membership_id<>NEW.actor_membership_id THEN
    RAISE EXCEPTION 'drawer movement cash event does not match its source' USING ERRCODE='23514';
  END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_drawer_movement_cash_location_event_complete
  AFTER INSERT ON hosted_drawer_movements DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_drawer_movement_cash_location_event();

GRANT SELECT (source_drawer_movement_id) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (source_drawer_movement_id) ON hosted_cash_location_events TO pos_api_runtime;
