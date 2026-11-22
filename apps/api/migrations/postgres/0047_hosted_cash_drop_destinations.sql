-- Link a till cash drop to its receiving branch safe/location.
ALTER TABLE hosted_drawer_movements
  ADD COLUMN cash_destination_location_id UUID,
  ADD CONSTRAINT hosted_drawer_movement_cash_destination_fk
    FOREIGN KEY (tenant_id, branch_id, cash_destination_location_id)
    REFERENCES hosted_cash_locations(tenant_id, branch_id, id) ON DELETE RESTRICT,
  ADD CONSTRAINT hosted_drawer_movement_cash_destination_type_check
    CHECK (cash_destination_location_id IS NULL OR movement_type='cash_drop');

GRANT SELECT (cash_destination_location_id) ON hosted_drawer_movements TO pos_api_runtime;
GRANT INSERT (cash_destination_location_id) ON hosted_drawer_movements TO pos_api_runtime;

ALTER TABLE hosted_cash_location_events
  DROP CONSTRAINT hosted_cash_location_event_type_check,
  DROP CONSTRAINT hosted_cash_location_event_shape_check;
ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow','expense_reimbursement_inflow','sale_tender_inflow','sale_refund_outflow','register_movement_inflow','register_movement_outflow','cash_drop_inflow')),
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
    OR (event_type='cash_drop_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND source_drawer_movement_id IS NOT NULL AND signed_amount_minor>0)
  );

DROP INDEX hosted_cash_event_drawer_movement_once;
CREATE UNIQUE INDEX hosted_cash_event_drawer_movement_type_once
  ON hosted_cash_location_events(tenant_id,source_drawer_movement_id,event_type)
  WHERE source_drawer_movement_id IS NOT NULL;

-- Cashiers may see only eligible, initialized safe destinations while working
-- at their assigned station. The branch cash-location workspace remains gated.
CREATE POLICY hosted_cash_locations_cashier_drop_destination_read ON hosted_cash_locations FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND location_type<>'register' AND status='active'
  AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_cash_locations.tenant_id AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_locations.branch_id)
      AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
);

CREATE POLICY hosted_cash_events_cashier_safe_opening_read ON hosted_cash_location_events FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND event_type='opening'
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type<>'register' AND l.status='active'
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
);

CREATE POLICY hosted_cash_events_cashier_drop_inflow_insert ON hosted_cash_location_events FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type='cash_drop_inflow' AND source_drawer_movement_id IS NOT NULL
  AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL
  AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    JOIN hosted_drawer_movements d ON d.tenant_id=hosted_cash_location_events.tenant_id
      AND d.branch_id=hosted_cash_location_events.branch_id AND d.id=hosted_cash_location_events.source_drawer_movement_id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type<>'register' AND l.status='active'
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
      AND d.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND d.actor_membership_id=hosted_cash_location_events.actor_membership_id
      AND d.movement_type='cash_drop' AND d.cash_destination_location_id=l.id
      AND d.amount_minor=hosted_cash_location_events.signed_amount_minor)
);

CREATE POLICY hosted_cash_events_cashier_drop_inflow_read ON hosted_cash_location_events FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND event_type='cash_drop_inflow'
  AND EXISTS (SELECT 1 FROM hosted_drawer_movements d
    JOIN memberships m ON m.id=d.actor_membership_id AND m.tenant_id=d.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE d.tenant_id=hosted_cash_location_events.tenant_id
      AND d.branch_id=hosted_cash_location_events.branch_id
      AND d.id=hosted_cash_location_events.source_drawer_movement_id
      AND d.cash_destination_location_id=hosted_cash_location_events.location_id
      AND d.movement_type='cash_drop'
      AND d.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND d.actor_membership_id=hosted_cash_location_events.actor_membership_id
      AND d.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=d.branch_id)
      AND (g.station_id IS NULL OR g.station_id=d.station_id))
);

CREATE OR REPLACE FUNCTION check_hosted_drawer_movement_cash_location_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  register_id UUID;
  initialized BOOLEAN;
  expected_event_type TEXT;
  expected_amount BIGINT;
  expected_expense_id UUID;
  expected_payment_id UUID;
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
  ELSIF NEW.movement_type='paid_in' THEN
    expected_event_type:='register_movement_inflow'; expected_amount:=NEW.amount_minor;
  ELSE
    expected_event_type:='register_movement_outflow'; expected_amount:=-NEW.amount_minor;
  END IF;

  SELECT count(*) INTO matching_count FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id
      AND event_type=expected_event_type
      AND source_expense_id IS NOT DISTINCT FROM expected_expense_id
      AND source_supplier_payment_id IS NOT DISTINCT FROM expected_payment_id;
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
