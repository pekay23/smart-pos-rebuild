# Cash source and drawer reconciliation

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This file retains cash-source and drawer reconciliation decisions.

## Work sequence

- [x] Ground: trace expense, supplier settlement, register shift, drawer movement, and Z close paths.
- [x] Sketch: compare explicit cash-source designs with the finance council.
- [x] Agree: select the domain shape and transaction invariants.
- [x] Implement: migrate and wire expense and supplier-payment flows end to end.
- [x] Verify: exercise migration constraints, drawer totals, native suite, frontend production build, lint, and formatting.

## Grounded flow

- `ExpensesPage` sends an `ExpenseInput` with a stable retry key and explicit cash origin to Rust `save_expense`; the command inserts the source row, source-linked drawer movement when paid from the till, audit event, and journal in one immediate SQLite transaction.
- `supplier_billing::record_supplier_payment` inserts the payment and allocations, source-linked drawer movement when applicable, AP settlement journal, and audit event atomically. Its existing retry key compares the funding origin and named outside-cash source as part of the payload.
- Hosted supplier cash settlement follows the equivalent PostgreSQL boundary: the station route verifies settlement authorization and signed active lease proof, then records payment/allocation, balanced journal, audit event, challenge consumption and source-linked `paid_out` in one transaction. Restricted-login integration verified the expected-cash reduction and exact retry without duplicate movement across migrations 0024–0025.
- Cash drawer commands resolve the device's station and require an open same-store shift for drawer-funded expense/payment writes. `report_for_shift` derives expected cash from opening float, cash sales, and movement types; it separately breaks down source-linked expense and supplier-payment outflows while applying the movement exactly once.
- `close_drawer_z` snapshots the expected amount and counted cash in one transaction. Editing the close report after posting would violate its audit meaning; any correction must be a compensating movement.

## Design decision

The accounting architect and finance operations council independently reviewed two designs; the cashier operations council cross-judged and selected the hybrid. Expense and supplier-payment sources retain funding origin (`drawer`, `other_cash`, `legacy_unknown`, `not_applicable`). A source-linked `paid_out` cash movement owns the actual register shift. This distinguishes cash intentionally paid outside the till from historical cash whose origin is unknown while keeping one authoritative deduction path for Z reports.

The off-register source is currently a required human-readable label, not a configured cash-account ledger. The UI states it is excluded from till close; account-level balances, opening floats, and transfers remain a separate cash-location subledger milestone. Hosted cash supplier settlement from non-drawer sources remains unsupported pending that ledger. No prior record is assigned to a current drawer.

## Implemented contract

- Migration 0026 adds source origin and notes to expenses and supplier payments, expense idempotency keys, and source identity to cash movements. Partial unique indexes prevent duplicate source movements. Triggers validate movement/source/store/amount/tender agreement and make source-linked movements immutable.
- Expense and supplier-payment commands run source insertion, linked drawer movement when selected, accounting journal, and audit event under one immediate transaction. Drawer identity comes from the authenticated installation's station. The same idempotency key and payload replays the original result before resolving today's open shift; key reuse with changed data is rejected.
- Drawer-paid entries require a current open shift. Named outside-cash entries do not change till cash. Non-cash entries never create a till movement. Historical cash is marked `legacy_unknown` and does not change an existing or future Z report.
- Z/X totals use the `cash_movements` path once. Expense and supplier-payment till outflows receive separate subtotals; no second accounting journal is created for the movement. Saved closed Z reports remain snapshots.

## Verification

- Rust migration test confirms legacy origins, duplicate source rejection, wrong-amount rejection, cross-store shift rejection, and immutability of source-linked drawer rows.
- Drawer report test confirms linked expense and supplier-payment outflows reduce expected cash once and remain separately visible.
- Full Rust library suite passes (61 tests); frontend production build and `cargo fmt --check` pass. Lint exits 0 with the existing React-effect warnings in Products, People, and Register pages.
- Authenticated visual review is still unavailable under the current local UI session; actual cash-entry UI flow and physical till close remain to be exercised with an authenticated test workspace.

