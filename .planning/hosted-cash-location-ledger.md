# Hosted cash-location ledger

Status: hosted locations, transfers, manager safe/petty-cash expense outflows, safe-funded supplier settlement, drawer-funded expense/supplier linkage, expense reimbursement, expense cost-center reclassification, station-register setup/cutover, cash sale/refund linkage, generic register movement linkage, balanced accounting journals for new hosted sales/refunds, eligible historical sale/refund journal backfill, permission-scoped branch accounting-coverage review, and append-only correction of legacy miscellaneous operating expenses are implemented and verified. Full financial statements and cash-to-GL reconciliation remain open.

## Phase checklist

- [x] Ground the existing desktop cash-location model and hosted expense/supplier/drawer/accounting paths.
- [x] Sketch two distinct hosted ledger shapes and cross-review the tradeoffs.
- [x] Select a data model, source-linking contract, authorization boundary, and migration sequence.
- [x] Implement the first hosted location-ledger slice and client workflow for branch safes, petty cash, openings, transfers and history.
- [x] Link manager safe/petty-cash expenses to one immutable source outflow, a balanced journal and audit event in a single transaction.
- [x] Link supplier payments funded from a safe/petty-cash location to one immutable source outflow, bill allocation, balanced journal and audit event in a single transaction.
- [x] Record expense returns, cap them at the remaining expense balance, and link cash inflows or reported external payments to balanced reversal journals and immutable audit history.
- [x] Verify migrations, forced RLS/grants, transfer atomicity, retries, balances, web build, and OpenAPI.
- [x] Update the consolidated implementation tracker and remaining category checklist.
- [x] Add manager setup for one station-bound register location and require its physical cutover count while the drawer is closed; expose unconfigured stations in the hosted web UI.
- [x] Link cash sale/refund tenders to typed immutable cash-location events and verify source, amount, actor, register cutover and restricted role access under PostgreSQL RLS.
- [x] Link generic register paid-in, paid-out and cash-drop movements to typed immutable events, enforce event-to-movement amount/actor/station matching, and verify cashier-only access under PostgreSQL RLS.
- [x] Link drawer-funded expenses and supplier payments to exactly one matching register cash event in the source transaction; verify restricted RLS policies, retries and the integrated register balance.
- [x] Post balanced journals atomically for new hosted sales/refunds, snapshot unit costs, reverse returns/tax and resalable inventory correctly, mark incomplete tax/cost coverage partial, and require exactly one journal per new sale/refund.
- [x] Backfill eligible historical sales/refunds with partial journals; prove repeated execution creates neither duplicate headers nor duplicate lines and never infers historical COGS.
- [x] Expose branch accounting coverage for missing journals, line/tender mismatches and partial historical journals; restrict route and UI to `reports:read` and cap exception detail at 50 records.
- [x] Move hosted “Other” operating expense postings to account 6000. Migration 0046 appends balanced corrections for existing expense and reimbursement journals that used COGS account 5000; source entries remain immutable.
- [x] Apply all 46 PostgreSQL migrations twice and pass both restricted tenant and operator HTTP integration suites, including sale/refund journal balance, cost reversal, historical backfill, account reclassification, coverage counts and cashier denial, RLS and source-event checks.

## Scope boundary

The hosted branch has cash tender-clearing account 1000 and signed register drawer outflows. Migrations 0036–0046 add hosted safe/petty-cash/other locations, openings, transfers, source-linked manager and drawer-funded cash expenses/supplier payments, expense reimbursements, append-only expense cost-center corrections, station register cutover, cash sale/refund plus drawer movement events, balanced journals for new hosted sales/refunds, partial journals for eligible historical sales/refunds, and append-only account reclassification for legacy miscellaneous expenses. The subledger tracks physical cash without double-posting to the GL. Full financial statements/reconciliation to account 1000 and immutable historical drawer close snapshots remain required.

## Existing evidence

- The desktop ledger already models register, safe, petty-cash, and other locations with append-only events and derived balances; see `apps/desktop/src-tauri/src/cash_locations.rs` and the local migration 0028.
- Desktop cash openings reconcile physical locations to aggregate GL cash without creating a second opening journal. Till movements remain attached to the register shift; safe and petty-cash events do not affect X/Z totals.
- Hosted cash supplier payments and cash expenses are immutable source records with balanced journals. Drawer-paid sources link to exactly one signed, open-shift `paid_out` movement; safe-funded expenses and supplier payments each link to one immutable negative location event.
- Hosted safe/petty-cash expense payments and supplier payments are implemented and use source-linked events; expense reimbursement is implemented in migration 0039; hosted expense reclassification is implemented in migration 0040 and changes reporting attribution without rewriting source payment, cash or journal events. The current transfer API deliberately excludes register locations so drawer activity stays bound to signed station operations.

## Design candidates

### Candidate A — immutable event subledger (selected)

Persist location identity and immutable signed events; derive current balances from events. Business sources remain authoritative and append their linked location event in the same transaction. Transfers have one immutable parent plus exactly two equal-and-opposite event legs. This is the closest fit to the existing desktop ledger and avoids a separately writable balance.

### Candidate B — mutable balance with append-only movement history (rejected)

Persist a current balance on each location and append a movement row for every opening, transfer, or source. Each transaction updates the balance and inserts its history row together; a periodic verifier recomputes balances from the history and repairs or flags drift. This makes reads cheap, but adds a second monetary truth and requires every write path to keep it synchronized.

### Cross-review

Candidate A was reviewed against Candidate B's performance advantage. The repository has no measured workload showing event aggregation is a bottleneck. The immutable event stream is therefore the smaller and safer initial contract; a rebuildable balance projection can be added later if measurements justify it, never as an independent source of truth. Hosted sales/refunds are not fully journaled, so a physical-location balance must not be described as a complete reconciliation to GL account 1000.

## Accepted design and implementation status

Use Candidate A. Scope every location to tenant, branch and currency; a register location is uniquely linked to its station. Keep location events append-only, balances derived, and source links typed or enforced by source-specific foreign keys (not an unchecked polymorphic ID). Opening and transfer writers acquire transaction-scoped advisory locks keyed by location UUID in deterministic ID order, then validate active/initialized/same-branch/same-currency and sufficient funds before appending events. This serializes balance-changing writes without granting row-update access to the immutable location master. Source record, location event, journal, drawer movement and audit write must share one database transaction where applicable. Register activity requires its exact open shift and current signed cashier proof and must create only one X/Z-counted drawer movement.

Cash locations are a physical-cash subledger, not another GL: openings and internal transfers do not post duplicate GL entries. Cutover counts establish explicit starting points; do not infer legacy cash locations. Until sale/refund journal coverage and cutover coverage are complete, expose tracked physical balances with an explicit incomplete-reconciliation state. Preserve historic drawer close snapshots.

Migration order: add location/event/transfer tables and RLS/grants; expose branch setup, openings, safe/petty transfers and event history; extend expense sources for explicit non-drawer funding (migration 0037); add reimbursement source events (0039) and append-only cost-center history (0040); wire station register cutover and sale/refund sources (0041), generic register drawer movements (0042), drawer-funded expense/supplier events (0043), sale/refund journals and immutable cost snapshots (0044), eligible historical sale/refund journal backfill (0045), then append-only correction of legacy “Other” expense classification (0046). A reports-scoped coverage route now surfaces unjournaled sources and partial postings. Next, reconcile cash against account 1000 and complete financial statements. Complete GL reconciliation claims only after every relevant source participates.

Implemented across migrations 0036–0046: branch list/create, one-time opening count, safe/petty transfer, event-history routes, manager safe-funded expenses and supplier settlements, drawer-funded expenses and supplier settlements, expense reimbursements, append-only expense cost-center corrections, station-bound register setup/cutover, typed cash sale/refund links, generic drawer-movement links, balanced journals for new hosted sales/refunds with unit-cost snapshots and partial-coverage status, eligible historical sale/refund journal backfill, and additive correction of legacy “Other” operating expenses previously posted as COGS. The hosted branch report now includes a reports-permission-scoped accounting coverage endpoint with missing-journal counts, line/tender mismatch reasons, partial historical postings and a 50-row exception cap. Forced RLS and append-only least-column grants cover the location ledger and source links. Restricted PostgreSQL HTTP verification passed register creation/opening, cashier-only cash sale, cash expense with the owner grant removed, elevated supplier settlement, generic movement linkage, owner-authorized refund linkage, exact retries without duplicate events, exactly one refund drawer outflow, balanced sale/refund journals with COGS/inventory reversal, idempotent historical backfill without invented COGS, and idempotent balanced correction of the legacy expense misclassification; the coverage route returned expected counts for management and denied a cashier. All 46 migrations applied twice and both restricted tenant/operator suites passed. A narrowly scoped RLS read policy lets a cashier inspect only their own open-station cash expense journal while its lines are inserted. The integrated scenario reconciles the physical register balance to the drawer, including source-linked expenses and supplier payments. Manager/owner balance-changing operations use transaction-scoped locks; changed create requests conflict, overdraw is rejected, and transfers create two equal-and-opposite legs. Remaining work is full financial statements, reconciliation of all relevant cash sources to account 1000, and immutable historical drawer close snapshots. Cash balances remain explicitly marked incomplete until that work is done. The product-wide remaining-work list stays in `docs/IMPLEMENTATION_TRACKER.md`.
