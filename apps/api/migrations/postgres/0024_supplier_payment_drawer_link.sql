-- Tie cash supplier settlements to the signed, source-linked station drawer outflow.
ALTER TABLE hosted_drawer_movements
  ADD COLUMN source_payment_id UUID;

ALTER TABLE hosted_drawer_movements
  ADD CONSTRAINT hosted_drawer_movement_supplier_payment_fk
  FOREIGN KEY (tenant_id,branch_id,source_payment_id)
  REFERENCES hosted_supplier_payments(tenant_id,branch_id,id)
  ON DELETE RESTRICT;

ALTER TABLE hosted_drawer_movements
  ADD CONSTRAINT hosted_drawer_supplier_payment_is_paid_out
  CHECK (source_payment_id IS NULL OR movement_type='paid_out');

CREATE UNIQUE INDEX hosted_drawer_supplier_payment_once
  ON hosted_drawer_movements(tenant_id,source_payment_id)
  WHERE source_payment_id IS NOT NULL;
