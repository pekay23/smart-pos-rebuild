-- Keep miscellaneous operating costs out of cost of goods sold.
-- Existing ledgers are corrected with additive journals; original entries stay immutable.

ALTER TABLE hosted_accounting_journals
  DROP CONSTRAINT hosted_accounting_journals_source_type_check,
  ADD CONSTRAINT hosted_accounting_journals_source_type_check
    CHECK (source_type IN ('supplier_bill','supplier_payment','expense','expense_reimbursement','sale','sale_refund','accounting_adjustment'));

INSERT INTO hosted_accounting_journals
  (id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description,coverage_status)
SELECT gen_random_uuid(),e.tenant_id,e.branch_id,'accounting_adjustment',e.id,j.currency_code,j.effective_at,
       j.actor_membership_id,'Reclassify legacy Other expense from cost of goods sold','complete'
FROM hosted_expenses e
JOIN hosted_accounting_journals j
  ON j.tenant_id=e.tenant_id AND j.branch_id=e.branch_id AND j.source_type='expense' AND j.source_id=e.id
JOIN hosted_accounting_journal_lines l
  ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
 AND l.account_code='5000' AND l.debit_minor=e.amount_minor AND l.credit_minor=0
WHERE e.category_code='other'
ON CONFLICT (tenant_id,branch_id,source_type,source_id) DO NOTHING;

INSERT INTO hosted_accounting_journal_lines
  (tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
SELECT j.tenant_id,j.branch_id,j.id,1,'6000','Reclassified miscellaneous operating expense',e.amount_minor,0
FROM hosted_accounting_journals j
JOIN hosted_expenses e ON e.tenant_id=j.tenant_id AND e.branch_id=j.branch_id AND e.id=j.source_id
WHERE j.source_type='accounting_adjustment'
  AND j.description='Reclassify legacy Other expense from cost of goods sold'
  AND e.category_code='other'
  AND NOT EXISTS (SELECT 1 FROM hosted_accounting_journal_lines l WHERE l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id);

INSERT INTO hosted_accounting_journal_lines
  (tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
SELECT j.tenant_id,j.branch_id,j.id,2,'5000','Reverse legacy cost-of-goods-sold classification',0,e.amount_minor
FROM hosted_accounting_journals j
JOIN hosted_expenses e ON e.tenant_id=j.tenant_id AND e.branch_id=j.branch_id AND e.id=j.source_id
WHERE j.source_type='accounting_adjustment'
  AND j.description='Reclassify legacy Other expense from cost of goods sold'
  AND e.category_code='other'
  AND NOT EXISTS (SELECT 1 FROM hosted_accounting_journal_lines l WHERE l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id AND l.line_number=2);

INSERT INTO hosted_accounting_journals
  (id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description,coverage_status)
SELECT gen_random_uuid(),r.tenant_id,r.branch_id,'accounting_adjustment',r.id,r.currency_code,j.effective_at,
       j.actor_membership_id,'Reclassify legacy Other reimbursement from cost of goods sold','complete'
FROM hosted_expense_reimbursements r
JOIN hosted_expenses e ON e.tenant_id=r.tenant_id AND e.branch_id=r.branch_id AND e.id=r.expense_id
JOIN hosted_accounting_journals j
  ON j.tenant_id=r.tenant_id AND j.branch_id=r.branch_id AND j.source_type='expense_reimbursement' AND j.source_id=r.id
JOIN hosted_accounting_journal_lines l
  ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
 AND l.account_code='5000' AND l.credit_minor=r.amount_minor AND l.debit_minor=0
WHERE e.category_code='other'
ON CONFLICT (tenant_id,branch_id,source_type,source_id) DO NOTHING;

INSERT INTO hosted_accounting_journal_lines
  (tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
SELECT j.tenant_id,j.branch_id,j.id,1,'5000','Reverse legacy COGS reimbursement classification',r.amount_minor,0
FROM hosted_accounting_journals j
JOIN hosted_expense_reimbursements r ON r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id AND r.id=j.source_id
JOIN hosted_expenses e ON e.tenant_id=r.tenant_id AND e.branch_id=r.branch_id AND e.id=r.expense_id
WHERE j.source_type='accounting_adjustment'
  AND j.description='Reclassify legacy Other reimbursement from cost of goods sold'
  AND e.category_code='other'
  AND NOT EXISTS (SELECT 1 FROM hosted_accounting_journal_lines l WHERE l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id);

INSERT INTO hosted_accounting_journal_lines
  (tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
SELECT j.tenant_id,j.branch_id,j.id,2,'6000','Reclassify miscellaneous operating reimbursement',0,r.amount_minor
FROM hosted_accounting_journals j
JOIN hosted_expense_reimbursements r ON r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id AND r.id=j.source_id
JOIN hosted_expenses e ON e.tenant_id=r.tenant_id AND e.branch_id=r.branch_id AND e.id=r.expense_id
WHERE j.source_type='accounting_adjustment'
  AND j.description='Reclassify legacy Other reimbursement from cost of goods sold'
  AND e.category_code='other'
  AND NOT EXISTS (SELECT 1 FROM hosted_accounting_journal_lines l WHERE l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id AND l.line_number=2);
