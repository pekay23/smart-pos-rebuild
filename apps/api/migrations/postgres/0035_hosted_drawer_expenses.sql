-- Keep cashier cash expenses tied to the active station drawer and immutable source.
ALTER TABLE station_financial_proof_challenges
  DROP CONSTRAINT station_financial_proof_challenges_operation_check;
ALTER TABLE station_financial_proof_challenges
  ADD CONSTRAINT station_financial_proof_challenges_operation_check
  CHECK (operation IN ('sale.commit','sale.refund','drawer.open','drawer.movement','drawer.close','supplier.payment','expense.payment'));

ALTER TABLE hosted_drawer_shifts
  ADD CONSTRAINT hosted_drawer_shift_branch_id_unique UNIQUE (tenant_id,branch_id,id);
ALTER TABLE hosted_expenses
  DROP CONSTRAINT hosted_expenses_payment_method_check;
ALTER TABLE hosted_expenses
  ADD CONSTRAINT hosted_expenses_payment_method_check
  CHECK (payment_method IN ('cash','card','mobile_money','bank_transfer','other'));
ALTER TABLE hosted_expenses
  ADD COLUMN drawer_shift_id UUID;
ALTER TABLE hosted_expenses
  ADD CONSTRAINT hosted_expense_drawer_shift_fk
  FOREIGN KEY (tenant_id,branch_id,drawer_shift_id)
  REFERENCES hosted_drawer_shifts(tenant_id,branch_id,id) ON DELETE RESTRICT;
ALTER TABLE hosted_expenses
  ADD CONSTRAINT hosted_expense_cash_requires_drawer
  CHECK ((payment_method='cash')=(drawer_shift_id IS NOT NULL));
ALTER TABLE hosted_expenses
  ADD CONSTRAINT hosted_expense_drawer_source_unique
  UNIQUE (tenant_id,branch_id,drawer_shift_id,id);

ALTER TABLE hosted_drawer_movements
  ADD COLUMN source_expense_id UUID;
ALTER TABLE hosted_drawer_movements
  ADD CONSTRAINT hosted_drawer_movement_expense_shift_fk
  FOREIGN KEY (tenant_id,branch_id,shift_id,source_expense_id)
  REFERENCES hosted_expenses(tenant_id,branch_id,drawer_shift_id,id) ON DELETE RESTRICT;
ALTER TABLE hosted_drawer_movements
  ADD CONSTRAINT hosted_drawer_movement_one_source
  CHECK (NOT (source_payment_id IS NOT NULL AND source_expense_id IS NOT NULL));
ALTER TABLE hosted_drawer_movements
  ADD CONSTRAINT hosted_drawer_expense_is_paid_out
  CHECK (source_expense_id IS NULL OR movement_type='paid_out');
CREATE UNIQUE INDEX hosted_drawer_expense_once
  ON hosted_drawer_movements(tenant_id,source_expense_id)
  WHERE source_expense_id IS NOT NULL;

CREATE POLICY hosted_expenses_station_cash_read ON hosted_expenses FOR SELECT USING (
  payment_method='cash'
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM memberships m
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    JOIN hosted_drawer_shifts s ON s.tenant_id=hosted_expenses.tenant_id
      AND s.branch_id=hosted_expenses.branch_id AND s.id=hosted_expenses.drawer_shift_id
    WHERE m.id=hosted_expenses.actor_membership_id
      AND m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_expenses.tenant_id AND m.status='active'
      AND g.role_key='cashier'
      AND g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND s.station_id=g.station_id
  )
);
CREATE POLICY hosted_expenses_station_cash_insert ON hosted_expenses FOR INSERT WITH CHECK (
  payment_method='cash'
  AND drawer_shift_id IS NOT NULL
  AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM memberships m
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    JOIN hosted_drawer_shifts s ON s.tenant_id=hosted_expenses.tenant_id
      AND s.branch_id=hosted_expenses.branch_id AND s.id=hosted_expenses.drawer_shift_id
    WHERE m.id=actor_membership_id
      AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.tenant_id=hosted_expenses.tenant_id AND m.status='active'
      AND g.role_key='cashier'
      AND g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND s.station_id=g.station_id AND s.status='open'
  )
);

CREATE POLICY hosted_accounting_journals_station_cash_expense_insert ON hosted_accounting_journals FOR INSERT WITH CHECK (
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
      AND e.actor_membership_id=hosted_accounting_journals.actor_membership_id
      AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND s.status='open' AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.status='active' AND g.role_key='cashier' AND g.station_id=s.station_id
  )
);
CREATE POLICY hosted_accounting_journal_lines_station_cash_expense_insert ON hosted_accounting_journal_lines FOR INSERT WITH CHECK (
  tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
  AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
  AND EXISTS (
    SELECT 1 FROM hosted_accounting_journals j
    JOIN hosted_expenses e ON e.tenant_id=j.tenant_id AND e.branch_id=j.branch_id AND e.id=j.source_id
    JOIN hosted_drawer_shifts s ON s.tenant_id=e.tenant_id AND s.branch_id=e.branch_id AND s.id=e.drawer_shift_id
    JOIN memberships m ON m.tenant_id=j.tenant_id AND m.id=j.actor_membership_id
    JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE j.tenant_id=hosted_accounting_journal_lines.tenant_id AND j.branch_id=hosted_accounting_journal_lines.branch_id
      AND j.id=hosted_accounting_journal_lines.journal_id AND j.source_type='expense'
      AND j.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
      AND e.payment_method='cash' AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
      AND s.status='open' AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
      AND m.status='active' AND g.role_key='cashier' AND g.station_id=s.station_id
  )
  AND ((line_number=1 AND account_code IN ('5000','5100','5200','5300','5400','5500','5600','5700','5800','5900') AND debit_minor>0 AND credit_minor=0)
    OR (line_number=2 AND account_code='1000' AND debit_minor=0 AND credit_minor>0))
);

GRANT SELECT (drawer_shift_id) ON hosted_expenses TO pos_api_runtime;
GRANT INSERT (drawer_shift_id) ON hosted_expenses TO pos_api_runtime;
GRANT INSERT (source_expense_id) ON hosted_drawer_movements TO pos_api_runtime;
GRANT SELECT (source_expense_id) ON hosted_drawer_movements TO pos_api_runtime;
