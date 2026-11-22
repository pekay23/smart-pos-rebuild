# Hosted drawer-funded expenses

Status: implemented and verified in the hosted API and register (1 November 2026).

## Scope

Allow an active cashier to record a small cash expense paid from the currently open station drawer. Keep the existing branch expense workspace for owner, admin and manager non-cash expenses. A cashier must not gain general branch expense creation or expense-list access as a side effect.

## Chosen flow

1. Add a station-scoped expense route that requires the dedicated `expenses:cash:write` permission, an active station lease, a one-use signed financial challenge, and the exact open drawer shift.
2. Accept only `paymentMethod: cash` on this route. The existing branch route remains non-cash and manager-scoped.
3. In one database transaction, insert the immutable expense, post a balanced expense journal (debit the category account, credit cash account 1000), record audit, and add one `paid_out` movement linked by `source_expense_id` to that expense and exact shift.
4. Bind proof to operation `expense.payment`, the shift ID and the exact request body. An exact retry with a fresh challenge returns the original expense without adding another movement; changed content with the same idempotency key conflicts.
5. Show a compact cash-out form in the register only when the active cashier has the permission, lease and open drawer. Refresh the drawer balance after success. Do not add a cashier expense-management workspace.

## Database and authorization requirements

- Add an additive migration. Allow `cash` in hosted expenses only when a drawer shift is linked; require a drawer shift for cash rows and no shift for non-cash rows.
- Add `source_expense_id` to drawer movements with tenant/branch-safe foreign keys, uniqueness, and a constraint preventing a movement from linking to both a supplier payment and an expense.
- Add forced-RLS insert/select policies limited to the active cashier, matching station, branch, tenant, open shift and dedicated permission. Add narrow runtime grants. Add expense-journal policies for this narrowly scoped source; do not grant generic journal access to cashiers.
- Preserve manager/owner/admin branch expense reads and non-cash create behavior. Cashier access must not permit editing, deleting, reclassifying or listing branch expenses.
- Ensure the drawer outflow, journal, audit and expense all commit or roll back together.

## Acceptance checks

- Restricted-role PostgreSQL migration/RLS/grant verifier applies the migration twice.
- Authorized cashier can record a cash expense only against their current open station shift; drawer expected cash decreases by the exact amount.
- Expense and drawer movement have one-to-one source linkage; journal is balanced and credits account 1000; audit event is present.
- Exact retry with a fresh signed challenge returns the same expense and creates no duplicate journal, audit or movement. Changed request conflicts.
- Missing/invalid proof, expired/revoked lease, wrong station/branch/shift, closed shift, non-cash method on the station route, cash on the branch route, and manager/cashier permission mismatches are denied without partial writes.
- Register UI exposes the form only to authorized cashiers and refreshes the drawer report after success.
- Update OpenAPI, API permission tests, migration verifier, web client type/API and tracker before marking complete.

## Explicitly out of scope

This does not implement safe/petty-cash locations, cash reimbursements, non-drawer branch settlements, expense correction/reclassification, or a complete hosted cash-location ledger. Those remain separate ledger work.

## Verification completed

- Restricted PostgreSQL verifier applied all 35 migrations twice and passed forced-RLS/least-column grant checks.
- The restricted-role HTTP integration opened a drawer, recorded a cashier cash expense, confirmed its linked `paid_out`, confirmed a balanced two-line journal crediting account 1000, and confirmed the expected drawer cash decreased by the exact amount.
- A fresh signed challenge exact retry returned the same expense and one movement; a changed payload under the same key conflicted.
- The same verifier exercised the broader hosted API and operator flows. `cargo test --locked -p pos-api --lib` passed 69 tests with the two database-only tests ignored in the normal suite; both database-only tests passed within the restricted PostgreSQL verifier.
- `apps/web` TypeScript and production build passed; OpenAPI YAML parsed with 67 paths and 135 schemas.
