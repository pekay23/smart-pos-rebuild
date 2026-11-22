-- Tie drawer-funded expenses and supplier payments to the register cash ledger.
-- Branch safe/petty-cash events remain source linked without a drawer movement.
CREATE POLICY hosted_cash_events_cashier_source_outflow_insert ON hosted_cash_location_events FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type IN ('expense_outflow','supplier_payment_outflow')
  AND source_drawer_movement_id IS NOT NULL
  AND EXISTS (
    SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    JOIN hosted_drawer_movements d ON d.tenant_id=hosted_cash_location_events.tenant_id
      AND d.branch_id=hosted_cash_location_events.branch_id AND d.id=source_drawer_movement_id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type='register' AND l.status='active'
      AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND (g.station_id IS NULL OR g.station_id=l.station_id)
      AND d.station_id=l.station_id AND d.shift_id IS NOT NULL AND d.movement_type='paid_out'
      AND d.actor_membership_id=actor_membership_id AND d.amount_minor=-signed_amount_minor
      AND ((event_type='expense_outflow' AND source_expense_id=d.source_expense_id
            AND source_supplier_payment_id IS NULL AND EXISTS (
              SELECT 1 FROM hosted_expenses e WHERE e.tenant_id=d.tenant_id AND e.branch_id=d.branch_id
                AND e.id=d.source_expense_id AND e.drawer_shift_id=d.shift_id
                AND e.cash_location_id IS NULL AND e.payment_method='cash'
                AND e.actor_membership_id=d.actor_membership_id AND e.amount_minor=d.amount_minor))
        OR (event_type='supplier_payment_outflow' AND source_supplier_payment_id=d.source_payment_id
            AND source_expense_id IS NULL AND EXISTS (
              SELECT 1 FROM hosted_supplier_payments p WHERE p.tenant_id=d.tenant_id AND p.branch_id=d.branch_id
                AND p.id=d.source_payment_id AND p.cash_location_id IS NULL AND p.payment_method='cash'
                AND p.actor_membership_id=d.actor_membership_id AND p.amount_minor=d.amount_minor)))
  )
);

CREATE POLICY hosted_cash_events_cashier_source_outflow_read ON hosted_cash_location_events FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type IN ('expense_outflow','supplier_payment_outflow')
  AND source_drawer_movement_id IS NOT NULL
  AND EXISTS (
    SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id
      AND l.id=hosted_cash_location_events.location_id AND l.location_type='register'
      AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
      AND (g.station_id IS NULL OR g.station_id=l.station_id))
);

-- The journal-line insert policy checks its parent journal under forced RLS.
-- Give a cashier visibility only to their own current station cash-expense journal.
CREATE POLICY hosted_accounting_journals_station_cash_expense_read ON hosted_accounting_journals FOR SELECT USING (
  source_type='expense'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM hosted_expenses e
    JOIN hosted_drawer_shifts s ON s.tenant_id=e.tenant_id AND s.branch_id=e.branch_id AND s.id=e.drawer_shift_id
    JOIN memberships m ON m.tenant_id=e.tenant_id AND m.id=e.actor_membership_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE e.tenant_id=hosted_accounting_journals.tenant_id AND e.branch_id=hosted_accounting_journals.branch_id
      AND e.id=hosted_accounting_journals.source_id AND e.payment_method='cash'
      AND e.cash_location_id IS NULL AND e.actor_membership_id=hosted_accounting_journals.actor_membership_id
      AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid AND s.status='open'
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND g.station_id=s.station_id
  )
);

CREATE OR REPLACE FUNCTION check_hosted_drawer_movement_cash_location_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  location_id UUID;
  initialized BOOLEAN;
  expected_event_type TEXT;
  signed_amount BIGINT;
  expected_expense_id UUID;
  expected_payment_id UUID;
  event_count INTEGER;
  event_row hosted_cash_location_events%ROWTYPE;
BEGIN
  SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
    INTO location_id,initialized FROM hosted_cash_locations l
    WHERE l.tenant_id=NEW.tenant_id AND l.branch_id=NEW.branch_id AND l.station_id=NEW.station_id AND l.location_type='register' AND l.status='active';
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF NOT initialized THEN RAISE EXCEPTION 'register cash location requires a cutover count' USING ERRCODE='23514'; END IF;

  IF NEW.source_expense_id IS NOT NULL THEN
    expected_event_type:='expense_outflow';
    signed_amount:=-NEW.amount_minor;
    expected_expense_id:=NEW.source_expense_id;
  ELSIF NEW.source_payment_id IS NOT NULL THEN
    expected_event_type:='supplier_payment_outflow';
    signed_amount:=-NEW.amount_minor;
    expected_payment_id:=NEW.source_payment_id;
  ELSIF NEW.source_refund_id IS NOT NULL THEN
    -- Refunds have their own source-tender location event in migration 0041.
    RETURN NULL;
  ELSIF NEW.movement_type='paid_in' THEN
    expected_event_type:='register_movement_inflow';
    signed_amount:=NEW.amount_minor;
  ELSE
    expected_event_type:='register_movement_outflow';
    signed_amount:=-NEW.amount_minor;
  END IF;

  SELECT count(*) INTO event_count FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id
      AND source_drawer_movement_id=NEW.id AND event_type=expected_event_type
      AND source_expense_id IS NOT DISTINCT FROM expected_expense_id
      AND source_supplier_payment_id IS NOT DISTINCT FROM expected_payment_id;
  IF event_count<>1 THEN RAISE EXCEPTION 'drawer movement requires one matching register cash event' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events
    WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_drawer_movement_id=NEW.id;
  IF event_row.location_id<>location_id OR event_row.signed_amount_minor<>signed_amount
     OR event_row.actor_membership_id<>NEW.actor_membership_id THEN
    RAISE EXCEPTION 'drawer movement cash event does not match its source' USING ERRCODE='23514';
  END IF;
  RETURN NULL;
END $$;
