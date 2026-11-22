-- Allow manager-recorded cash expenses to originate from an initialized safe or petty-cash location.
ALTER TABLE hosted_expenses
  ADD COLUMN cash_location_id UUID;
ALTER TABLE hosted_expenses
  ADD CONSTRAINT hosted_expense_cash_location_fk
  FOREIGN KEY (tenant_id,branch_id,cash_location_id)
  REFERENCES hosted_cash_locations(tenant_id,branch_id,id) ON DELETE RESTRICT;
ALTER TABLE hosted_expenses
  DROP CONSTRAINT hosted_expense_cash_requires_drawer;
ALTER TABLE hosted_expenses
  ADD CONSTRAINT hosted_expense_cash_has_one_source
  CHECK (
    (payment_method='cash' AND ((drawer_shift_id IS NOT NULL) <> (cash_location_id IS NOT NULL)))
    OR (payment_method<>'cash' AND drawer_shift_id IS NULL AND cash_location_id IS NULL)
  );

ALTER TABLE hosted_cash_location_events
  ADD COLUMN source_expense_id UUID;
ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_event_expense_fk
  FOREIGN KEY (tenant_id,branch_id,source_expense_id)
  REFERENCES hosted_expenses(tenant_id,branch_id,id) ON DELETE RESTRICT;
ALTER TABLE hosted_cash_location_events
  DROP CONSTRAINT hosted_cash_location_events_event_type_check,
  DROP CONSTRAINT hosted_cash_location_events_check;
ALTER TABLE hosted_cash_location_events
  ADD CONSTRAINT hosted_cash_location_event_type_check
    CHECK (event_type IN ('opening','transfer_out','transfer_in','expense_outflow')),
  ADD CONSTRAINT hosted_cash_location_event_shape_check
    CHECK (
      (event_type='opening' AND source_transfer_id IS NULL AND source_expense_id IS NULL AND signed_amount_minor>=0)
      OR (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND signed_amount_minor<0)
      OR (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND source_expense_id IS NULL AND signed_amount_minor>0)
      OR (event_type='expense_outflow' AND source_transfer_id IS NULL AND source_expense_id IS NOT NULL AND signed_amount_minor<0)
    );
CREATE UNIQUE INDEX hosted_cash_event_expense_once
  ON hosted_cash_location_events(tenant_id,source_expense_id)
  WHERE source_expense_id IS NOT NULL;

CREATE FUNCTION check_hosted_cash_expense_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  event_row hosted_cash_location_events%ROWTYPE;
  event_count INTEGER;
BEGIN
  IF NEW.payment_method<>'cash' OR NEW.cash_location_id IS NULL THEN RETURN NULL; END IF;
  SELECT count(*) INTO event_count FROM hosted_cash_location_events
   WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_expense_id=NEW.id
     AND event_type='expense_outflow';
  IF event_count<>1 THEN RAISE EXCEPTION 'cash expense requires one location outflow' USING ERRCODE='23514'; END IF;
  SELECT * INTO event_row FROM hosted_cash_location_events
   WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND source_expense_id=NEW.id
     AND event_type='expense_outflow';
  IF event_row.location_id<>NEW.cash_location_id OR event_row.signed_amount_minor<>-NEW.amount_minor
     OR event_row.actor_membership_id<>NEW.actor_membership_id
  THEN RAISE EXCEPTION 'cash expense outflow does not match its source' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_cash_expense_event_complete
  AFTER INSERT ON hosted_expenses DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_cash_expense_event();

GRANT SELECT (cash_location_id) ON hosted_expenses TO pos_api_runtime;
GRANT INSERT (cash_location_id) ON hosted_expenses TO pos_api_runtime;
GRANT SELECT (source_expense_id) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (source_expense_id) ON hosted_cash_location_events TO pos_api_runtime;
