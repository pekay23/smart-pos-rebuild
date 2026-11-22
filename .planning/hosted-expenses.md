# Hosted branch expenses

## Domain choices

- An expense is an append-only branch source with a branch-currency amount, category, description, optional payee/reference, effective date and recording membership.
- Creating an expense, its balanced source journal and its audit event is one PostgreSQL transaction. Request keys are scoped by tenant, branch and actor; identical retries return the original record and changed payloads conflict.
- Managers, administrators and owners can read and record expenses within their granted branch scope. Cashiers cannot access the branch expense workspace. API authorization is backed by forced RLS and least-column runtime grants.
- The first hosted slice accepts manually reported non-cash methods only. It does not initiate or verify a transfer. Cash is rejected because hosted safe/petty-cash accounts do not yet exist and a drawer expense must create a signed, source-linked `paid_out` against its exact open shift.
- Category keys map to operating-expense accounts: `other` → 5000, `rent` → 5100, `utilities` → 5200, `wages` → 5300, `transport` → 5400, `supplies` → 5500, `maintenance` → 5600, `marketing` → 5700, `bank_fees` → 5800 and `taxes_fees` → 5900. Payment clearing uses the existing non-cash accounts 1010, 1020, 1030 and 1090.

## Implemented

- Migration 0033 adds immutable hosted expense sources and expands the journal source type with forced RLS, expense-specific policies and runtime grants.
- `GET` lists a bounded page; `POST` validates category/method/date/amount, hashes the request, serializes the idempotency key, writes the expense, balanced journal and audit event atomically.
- The hosted web workspace lists records and permits authorized staff to submit expenses. Cash is visibly identified as pending support.
- Restricted PostgreSQL integration covers owner/manager write and read, exact retry, payload conflict, cashier read/write denial, cash rejection, balanced journal lines and absence of failed source rows.

## Still required

- Authenticated visual review across owner, manager and cashier roles.
- Cash-location ledger, safe/petty-cash setup, cash-expense and signed drawer-expense flows.
- Reimbursements with actual return tender/destination, partial-return bounds and linked contra journals.
- Versioned cost-center attribution corrections and audit history.
- Expense receipts/attachments with private storage, retention and size/type controls.
- Date-filtered reports, CSV export, accountant review, corrections, and complete expense/sales/receipt journal coverage before full financial statements.
- Hosted offline outbox and reconciliation support.

The local desktop reimbursement and cost-center domain decisions are in `expense-reimbursements.md` and `expense-cost-center-reclassification.md`. This hosted API slice does not claim those local workflows are available in the cloud.
