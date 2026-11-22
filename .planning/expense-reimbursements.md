# Expense reimbursements

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This file retains reimbursement transaction and acceptance detail.

## Council decision

Use a new immutable expense-reimbursement source table instead of extending the old expense-reversal table. A reimbursement is a new cash receipt with its own actual return tender, amount, reference/description, destination, and effective date. This prevents the original payment method from being mistaken for the return method and supports multiple partial returns. Preserve and migrate existing full non-cash reversal rows into the new table without changing their IDs or journals.

The finance and cashier-operations personas recommend partial or full returns bounded by the unreturned amount. The accounting persona recommends full-return-only. The majority recommendation is selected because actual provider returns can be partial, and requiring a fictitious full refund would make the register and reports inaccurate.

## Required transaction

One immediate store-scoped transaction validates the source and cumulative remainder, verifies the actual return method, validates cash provenance/destination when applicable, appends the reimbursement, creates the cash-location inflow, appends a drawer paid-in movement only for the destination open register, posts the actual-tender debit and expense credit, writes the audit event, and commits. Retries return the same saved event only when the complete request matches.

Cash must be directed to an explicitly selected, initialized safe/petty-cash location or to the signed-in device's assigned register with its exact current open shift. Never infer the receiving location from the expense's original tender or location, and never add a return to the expense's original closed shift. This destination rule also allows an older cash expense whose original location was not recorded to be reimbursed safely: the actual receiving location is still explicit and validated.

## Progress

- [x] Migration 0030 with migrated legacy reversals, partial reimbursements, source validation, and cash event/paid-in support. Verified legacy ID/reference preservation, cash-event review foreign keys, over-return rejection, safe inflow, and `PRAGMA foreign_key_check`.
- [x] Add the transactional command and actual-tender accounting journal. The command is role-gated, store-scoped, idempotent, and commits the linked ledger/audit effects atomically.
- [x] Integrate received-cash events with location balances and X/Z totals. Register returns use the exact current open shift; safes and petty cash do not alter drawer reports.
- [x] Update expense report snapshots, P&L, screen, and complete CSV export. The screen supports partial returns, actual tender, destination, reference/reason, remaining balance, and manager/admin/owner-only action visibility.
- [x] Test register station/shift validation, atomic rejection, retry replay after close, and later-shift reimbursement preserving a saved Z snapshot. Authorization-role coverage, exact-request idempotent replay/mismatch, cumulative cap, migration preservation, current-device station matching, closed-shift rejection without writes, and actual-tender journal balance/post-once behavior are covered.
- [x] Frontend build/lint, Rust format/full suite, and desktop check. Current full Rust library suite passes 77 tests; frontend production build/lint, Rust formatting, and desktop binary check pass. Manual live-window workflow review remains open. See [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md) for the consolidated continuation list.

## Council acceptance examples

- Return a bank-paid expense partly in cash to a safe: the safe and aggregate cash asset increase, expense is reduced on the return date, and no drawer shift changes.
- Return expense funds to the active register: the receiving location and that current shift's paid-in increase once; the original expense shift and any saved Z remain unchanged.
- Attempt to reimburse beyond the original amount, reuse a key with different details, send cash to a closed/wrong station, or use an uninitialized/cross-store location: reject atomically. A legacy cash expense with unknown original location remains eligible when the real receiving location is explicitly selected and validated.

