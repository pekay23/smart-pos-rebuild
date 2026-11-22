# Legacy expense-reversal milestone (superseded)

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This historical milestone is superseded by `expense-reimbursements.md`.

The full-return-only non-cash workflow described here is superseded by [expense-reimbursements.md](expense-reimbursements.md). Migration 0029 remains as the historical schema version; migration 0030 copies its immutable rows and is now the source of truth for partial returns, actual receiving tender, cash destination, report display, and X/Z register effects. Use the newer plan for remaining work and acceptance criteria.

## Progress checklist

- [x] Ground the append-only expense, accounting, reporting, permission, and migration paths.
- [x] Compare data-model alternatives and choose an append-only source event.
- [x] Add schema and transactional Rust command for confirmed non-cash reimbursements.
- [x] Add signed reimbursement rows to expense browsing, pagination, export, and period P&L.
- [x] Add the manager/administrator reimbursement UI with reason and returned-payment reference.
- [x] Add migration/schema constraint, report isolation/snapshot, and contra-journal coverage checks.
- [ ] Add direct command-level permission and idempotent-retry tests.
- [x] Run format, frontend build/lint, and the full Rust suite.
- [ ] Produce a fresh standalone Windows desktop executable.
- [ ] Review the reimbursement workflow in the live desktop UI.

## Implementation evidence

- Migration 0029 appends a store-scoped, immutable, full-amount reversal only for a matching non-cash expense; unique store/expense, idempotency, and provider-reference constraints prevent double recording.
- The reverse_expense command checks expenses:write, replays matching idempotent retries, and atomically appends the reversal, linked balanced contra journal, and audit row.
- Expense reports include signed correction rows and the period profit report nets the return on its actual returned date. CSV includes row type, reason, and external reference.
- Paged report snapshots capture separate expense and reversal row-ID high-water marks. A shared numeric ceiling was rejected because each SQLite table has its own row-ID sequence and a later row in the lower sequence could otherwise enter an earlier page snapshot.
- Verification: cargo fmt; all 66 Rust library tests pass; npm run build passes; npm run lint passes with the three existing effect warnings in RegisterPage, ProductsPage, and PeoplePage. The frontend build retains the existing 500 KB bundle-size warning. Desktop cargo check passes; a fresh standalone build was not completed because the isolated target began recompiling the vendored OpenSSL dependency, while the existing review executable remained in use. Live UI review, command-level permission/retry tests, and physical payment-provider confirmation are also open.

## Caller workflow

From an expense row paid by mobile money, bank transfer, or another recorded non-cash method, an authorized manager or administrator records a reimbursement after verifying that the money was actually returned. They provide a reason and the return transaction reference. The system appends one immutable reversal, links it to the original expense, posts the opposite journal on the reimbursement date, includes the negative amount in expense reports and CSV exports for that date, and writes an audit event. It does not edit the source expense or claim that an external processor verified the refund.

Cash expenses are intentionally excluded from this first slice because reversing physical cash must append an inflow to the exact safe/till location and, when applicable, the open drawer's X/Z movement stream. Add that path only when all physical cash consequences can be committed atomically.

## Candidates and synthesis

**A. Update the original expense and journal.** Simple to query, but conflicts with existing immutable source/journal triggers, rewrites historical reports, and cannot explain who corrected the source or when funds were returned. Rejected.

**B. Append an expense reversal event and linked contra journal.** Keeps the original source and journal immutable; uses the actual returned-payment date; supports idempotency and an audit reason; allows report totals to reflect the period when funds came back. Chosen, consistent with the finance/cash council's source-aware correction decision already captured in `cash-locations-opening-balances.md`.

## Type and module sketch

```rust
pub struct ExpenseReversalInput {
    pub expense_id: String,
    pub reason: String,
    pub return_reference: String,
    pub idempotency_key: String,
}

pub struct ExpenseReversal {
    pub id: String,
    pub expense_id: String,
    pub amount_minor: i64,
    pub payment_method: String,
    pub return_reference: String,
    pub reason: String,
    pub returned_at: String,
    pub recorded_by: String,
}

pub fn reverse_expense(input: ExpenseReversalInput, state: State<AppState>)
    -> Result<ExpenseReversal, String>;
```

`expense_corrections.rs` owns validation and one immediate SQLite transaction: verify `expenses:write`, same-store source, non-cash tender, no prior reversal, unique idempotency/reference; append reversal; post a journal with `source_type='reversal'` and `reversal_of` set to the original expense journal; append audit event; commit. `expense_reports.rs` returns signed `ExpenseReportRow` values and computes totals from the same immutable page snapshot. `sales.rs` nets reimbursement amounts in period P&L using returned-payment time. `ExpensesPage.tsx` presents a clearly labeled negative reimbursement row and offers the action only for non-cash, unreversed sources to users with `expenses:write`.

Acceptance requires same-store authorization, immutability, exact full-amount reconciliation, same-tender manual evidence, single-use references/keys, no cash side effects, period-correct net totals, stable page/export snapshots, and balanced linked contra journals.

