-- Link branch-level cash supplier payments to an initialized safe or petty-cash location.
ALTER TABLE hosted_supplier_payments
  ADD COLUMN cash_location_id UUID,
  ADD CONSTRAINT hosted_supplier_payment_cash_location_fk
    FOREIGN KEY (tenant_id,branch_id,cash_location_id)
    REFERENCES hosted_cash_locations(tenant_id,branch_id,id) ON DELETE RESTRICT,
  ADD CONSTRAINT hosted_supplier_payment_cash_location_method
    CHECK (payment_method='cash' OR cash_location_id IS NULL);

ALTER TABLE hosted_cash_location_events
  ADD COLUMN source_supplier_payment_id UUID,
  ADD CONSTRAINT hosted_cash_event_supplier_payment_fk
    FOREIGN KEY (tenant_id,branch_id,source_supplier_payment_id)
    REFERENCES hosted_supplier_payments(tenant_id,branch_id,id) ON DELETE RESTRICT,
  DROP CONSTRAINT hosted_cash_location_event_type_check,
  DROP CONSTRAINT hosted_cash_location_event_shape_check;
ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow')),
  ADD CONSTRAINT hosted_cash_location_event_shape_check
    CHECK (
      (event_type='opening' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND signed_amount_minor>=0)
      OR (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND signed_amount_minor<0)
      OR (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NULL AND signed_amount_minor>0)
      OR (event_type='expense_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NOT NULL AND source_supplier_payment_id IS NULL AND signed_amount_minor<0)
      OR (event_type='supplier_payment_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND source_supplier_payment_id IS NOT NULL AND signed_amount_minor<0)
    );
CREATE UNIQUE INDEX hosted_cash_event_supplier_payment_once
  ON hosted_cash_location_events(tenant_id,source_supplier_payment_id)
  WHERE source_supplier_payment_id IS NOT NULL;

CREATE FUNCTION check_hosted_cash_supplier_payment_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  event_row hosted_cash_location_events%ROWTYPE;
  event_count INTEGER;
BEGIN
  IF NEW.payment_method<>'cash' OR NEW.cash_location_id IS NULL THEN RETURN NULL; END IF;
  SELECT count(*) INTO event_count FROM hosted_cash_location_events
   WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_supplier_payment_id=NEW.id
     AND event_type='supplier_payment_outflow';
  IF event_count<>1 THEN RAISE EXCEPTION 'cash supplier payment requires one location outflow' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events
   WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_supplier_payment_id=NEW.id
     AND event_type='supplier_payment_outflow';
  IF event_row.location_id<>NEW.cash_location_id OR event_row.signed_amount_minor<>-NEW.amount_minor
     OR event_row.actor_membership_id<>NEW.actor_membership_id
  THEN RAISE EXCEPTION 'cash supplier payment outflow does not match its source' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_cash_supplier_payment_event_complete
  AFTER INSERT ON hosted_supplier_payments DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_cash_supplier_payment_event();

GRANT SELECT (cash_location_id) ON hosted_supplier_payments TO pos_api_runtime;
GRANT INSERT (cash_location_id) ON hosted_supplier_payments TO pos_api_runtime;
GRANT SELECT (source_supplier_payment_id) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (source_supplier_payment_id) ON hosted_cash_location_events TO pos_api_runtime;
