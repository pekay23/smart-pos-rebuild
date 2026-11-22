-- Source-link hosted register cash into the physical cash-location ledger.
-- Drawer movements remain the sole source for X/Z cash movement totals.
ALTER TABLE hosted_sale_tenders
  ADD CONSTRAINT hosted_sale_tender_branch_id_unique UNIQUE (tenant_id,branch_id,id);
ALTER TABLE hosted_sale_refund_tenders
  ADD CONSTRAINT hosted_sale_refund_tender_branch_id_unique UNIQUE (tenant_id,branch_id,id);

ALTER TABLE hosted_cash_location_events
  ADD COLUMN source_sale_tender_id UUID,
  ADD COLUMN source_refund_tender_id UUID,
  ADD CONSTRAINT hosted_cash_event_sale_tender_fk
    FOREIGN KEY (tenant_id,branch_id,source_sale_tender_id)
    REFERENCES hosted_sale_tenders(tenant_id,branch_id,id) ON DELETE RESTRICT,
  ADD CONSTRAINT hosted_cash_event_refund_tender_fk
    FOREIGN KEY (tenant_id,branch_id,source_refund_tender_id)
    REFERENCES hosted_sale_refund_tenders(tenant_id,branch_id,id) ON DELETE RESTRICT,
  DROP CONSTRAINT hosted_cash_location_event_type_check,
  DROP CONSTRAINT hosted_cash_location_event_shape_check;

ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow','expense_reimbursement_inflow','sale_tender_inflow','sale_refund_outflow')),
  ADD CONSTRAINT hosted_cash_location_event_shape_check CHECK (
    (event_type='opening' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor>=0)
    OR (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor<0)
    OR (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor>0)
    OR (event_type='expense_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NOT NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor<0)
    OR (event_type='supplier_payment_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NOT NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor<0)
    OR (event_type='expense_reimbursement_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NOT NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NULL AND signed_amount_minor>0)
    OR (event_type='sale_tender_inflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NOT NULL AND source_refund_tender_id IS NULL AND signed_amount_minor>0)
    OR (event_type='sale_refund_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND source_expense_reimbursement_id IS NULL AND source_sale_tender_id IS NULL AND source_refund_tender_id IS NOT NULL AND signed_amount_minor<0)
  );
CREATE UNIQUE INDEX hosted_cash_event_sale_tender_once
  ON hosted_cash_location_events(tenant_id,source_sale_tender_id) WHERE source_sale_tender_id IS NOT NULL;
CREATE UNIQUE INDEX hosted_cash_event_refund_tender_once
  ON hosted_cash_location_events(tenant_id,source_refund_tender_id) WHERE source_refund_tender_id IS NOT NULL;

-- Once a register location is provisioned, it must be cut over before cash
-- is accepted, and every cash tender/refund must have one matching ledger event.
CREATE FUNCTION check_hosted_sale_tender_cash_location_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE sale_row hosted_sales%ROWTYPE; location_id UUID; initialized BOOLEAN; event_row hosted_cash_location_events%ROWTYPE; event_count INTEGER;
BEGIN
  IF NEW.tender_kind<>'cash' THEN RETURN NULL; END IF;
  SELECT * INTO sale_row FROM hosted_sales WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND id=NEW.sale_id;
  SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
    INTO location_id,initialized FROM hosted_cash_locations l
    WHERE l.tenant_id=NEW.tenant_id AND l.branch_id=NEW.branch_id AND l.station_id=sale_row.station_id AND l.location_type='register' AND l.status='active';
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF NOT initialized THEN RAISE EXCEPTION 'register cash location requires a cutover count' USING ERRCODE='23514'; END IF;
  SELECT count(*) INTO event_count FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_sale_tender_id=NEW.id AND event_type='sale_tender_inflow';
  IF event_count<>1 THEN RAISE EXCEPTION 'cash sale tender requires one location inflow' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_sale_tender_id=NEW.id AND event_type='sale_tender_inflow';
  IF event_row.location_id<>location_id OR event_row.signed_amount_minor<>NEW.amount_minor OR event_row.actor_membership_id<>sale_row.cashier_membership_id
  THEN RAISE EXCEPTION 'cash sale location inflow does not match its source' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_sale_tender_cash_location_event_complete
  AFTER INSERT ON hosted_sale_tenders DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_sale_tender_cash_location_event();

CREATE FUNCTION check_hosted_refund_tender_cash_location_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE refund_row hosted_sale_refunds%ROWTYPE; location_id UUID; initialized BOOLEAN; event_row hosted_cash_location_events%ROWTYPE; event_count INTEGER;
BEGIN
  IF NEW.method<>'cash' THEN RETURN NULL; END IF;
  SELECT * INTO refund_row FROM hosted_sale_refunds WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND id=NEW.refund_id;
  SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
    INTO location_id,initialized FROM hosted_cash_locations l
    WHERE l.tenant_id=NEW.tenant_id AND l.branch_id=NEW.branch_id AND l.station_id=refund_row.station_id AND l.location_type='register' AND l.status='active';
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF NOT initialized THEN RAISE EXCEPTION 'register cash location requires a cutover count' USING ERRCODE='23514'; END IF;
  SELECT count(*) INTO event_count FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_refund_tender_id=NEW.id AND event_type='sale_refund_outflow';
  IF event_count<>1 THEN RAISE EXCEPTION 'cash refund tender requires one location outflow' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_refund_tender_id=NEW.id AND event_type='sale_refund_outflow';
  IF event_row.location_id<>location_id OR event_row.signed_amount_minor<>-NEW.amount_minor OR event_row.actor_membership_id<>refund_row.actor_membership_id
  THEN RAISE EXCEPTION 'cash refund location outflow does not match its source' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_refund_tender_cash_location_event_complete
  AFTER INSERT ON hosted_sale_refund_tenders DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_refund_tender_cash_location_event();

-- Cashiers need the exact active register row to link sale/refund tenders
-- inside their transaction. Do not expose branch safes or other locations.
CREATE POLICY hosted_cash_locations_cashier_register_read ON hosted_cash_locations FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND location_type='register' AND status='active'
  AND station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
  AND EXISTS (SELECT 1 FROM memberships m
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_cash_locations.tenant_id AND m.status='active'
      AND g.role_key='cashier'
      AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_locations.branch_id)
      AND (g.station_id IS NULL OR g.station_id=hosted_cash_locations.station_id))
);

CREATE POLICY hosted_cash_events_cashier_sale_refund_insert ON hosted_cash_location_events FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type IN ('sale_tender_inflow','sale_refund_outflow')
  AND EXISTS (
    SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id AND l.id=hosted_cash_location_events.location_id
      AND l.location_type='register' AND l.status='active' AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id) AND (g.station_id IS NULL OR g.station_id=l.station_id)
      AND ((event_type='sale_tender_inflow' AND EXISTS (SELECT 1 FROM hosted_sale_tenders t JOIN hosted_sales s ON s.tenant_id=t.tenant_id AND s.branch_id=t.branch_id AND s.id=t.sale_id WHERE t.tenant_id=hosted_cash_location_events.tenant_id AND t.branch_id=hosted_cash_location_events.branch_id AND t.id=source_sale_tender_id AND t.tender_kind='cash' AND t.amount_minor=signed_amount_minor AND s.station_id=l.station_id AND s.cashier_membership_id=actor_membership_id))
        OR (event_type='sale_refund_outflow' AND EXISTS (SELECT 1 FROM hosted_sale_refund_tenders t JOIN hosted_sale_refunds r ON r.tenant_id=t.tenant_id AND r.branch_id=t.branch_id AND r.id=t.refund_id WHERE t.tenant_id=hosted_cash_location_events.tenant_id AND t.branch_id=hosted_cash_location_events.branch_id AND t.id=source_refund_tender_id AND t.method='cash' AND t.amount_minor=-signed_amount_minor AND r.station_id=l.station_id AND r.actor_membership_id=actor_membership_id)))
  )
);

CREATE POLICY hosted_cash_events_cashier_source_read ON hosted_cash_location_events FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND event_type IN ('sale_tender_inflow','sale_refund_outflow')
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=hosted_cash_location_events.actor_membership_id AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id AND l.id=hosted_cash_location_events.location_id
      AND l.location_type='register' AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id) AND (g.station_id IS NULL OR g.station_id=l.station_id))
);
CREATE POLICY hosted_cash_events_cashier_opening_read ON hosted_cash_location_events FOR SELECT USING (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND event_type='opening'
  AND EXISTS (SELECT 1 FROM hosted_cash_locations l
    JOIN memberships m ON m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.tenant_id=l.tenant_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE l.tenant_id=hosted_cash_location_events.tenant_id AND l.branch_id=hosted_cash_location_events.branch_id AND l.id=hosted_cash_location_events.location_id
      AND l.location_type='register' AND l.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
      AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id) AND (g.station_id IS NULL OR g.station_id=l.station_id))
);

GRANT SELECT (source_sale_tender_id,source_refund_tender_id) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (source_sale_tender_id,source_refund_tender_id) ON hosted_cash_location_events TO pos_api_runtime;
