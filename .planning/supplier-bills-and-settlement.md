# Supplier bills and settlement

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This file retains bill and settlement domain decisions.

## Work phases

- [x] Ground: trace receipt, supplier, accounting-journal, permission, and purchase UI flows.
- [x] Council: compare bill-to-receipt matching and settlement boundaries.
- [x] Sketch: settle the bill/payment domain and transaction contracts below.
- [x] Local desktop schema/domain/UI: add store-scoped bills, receipt allocations, supplier payments and allocations, append-only journal posting, owner/admin settlement permissions, invoice matching and manual payment recording in the SQLite desktop client.
- [x] Local desktop verification: migration preservation and role-default checks, frontend build, Rust checks and focused suite passed as recorded in the dated tracker milestones. Cross-store command workflow and live app review remain open.
- [x] Hosted invoice matching: PostgreSQL migration 0022, forced RLS, least-column grants, manager matching API and web invoice allocation view are implemented and verified; see the 31 October milestone in [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md).
- [x] Hosted payment schema/API/UI: migration 0023 adds immutable payments/allocations, open/paid balances, idempotent settlement, owner/admin `suppliers:settle` enforcement, audit and the permission-gated web payment form.
- [x] Hosted accounting journals: invoice match and supplier payment records post balanced, append-only source journals in the same transaction, using built-in cash/tender-clearing account codes; a deferred PostgreSQL constraint rejects unbalanced entries.
- [x] Verify hosted payment allocations and journal posting under restricted PostgreSQL login: partial allocation, exact retry, changed-payload conflict, overpayment denial, owner success, manager/cashier denial, open balance and balanced journal. All 23 migrations applied twice; the API integration, unit suite, web production build and OpenAPI ref validation pass.
- [x] Hosted drawer-funded cash integration: station-scoped settlement requires an active signed cashier lease and open drawer shift, and atomically posts the payment, allocation, journal, audit event and source-linked `paid_out`; exact retry does not duplicate the drawer movement. Verified in restricted PostgreSQL HTTP integration against all 25 migrations.
- [ ] Hosted non-drawer cash locations and full accounting: cash settlement from other cash locations remains unsupported until their ledger is modeled. Post hosted receipt, sales and expense sources before representing the journals as a complete trial balance or financial statements.
- [~] Local desktop physical-goods return credit notes: migration 0040, immutable note/line records, returnable received-item candidates, stock reduction, open-balance validation, source journal, audit and idempotency are implemented. Migration preservation/constraint test, return quantity/value/overflow arithmetic coverage, the earlier 116-test desktop library suite, frontend production build and lint pass. Two focused command-transaction tests now pass, covering exact retry, changed-payload rejection, duplicate-write prevention and audit-failure rollback. Authenticated UI review remains open.
- [x] Hosted physical-goods return credit notes: migration 0065, owner/admin-only forced-RLS records, idempotent API, commitment-aware stock reduction, audit/movement history, balanced AP/Inventory journal and web return form/history are implemented. The restricted PostgreSQL verifier passed with all 65 migrations applied twice and both tenant/operator HTTP scenarios green; the tenant scenario confirmed exact replay, changed-payload conflict, manager denial, receipt/stock/payable caps, journal posting and transaction rollback. Paid-invoice refunds, freight/tax/price variance and supplier advances remain open until their linked payable, inventory and accounting treatment is specified and verified.

## Local desktop physical return credit notes (7 November 2026)

The first desktop correction slice accepts only physical goods returned from a received delivery already allocated to a supplier invoice. The credit is valued at the recorded receipt unit cost, cannot exceed the quantity still attributed to that delivery, current branch stock, remaining invoice allocation for that delivery or the invoice's unpaid balance. One immediate SQLite transaction stores the immutable credit note and item snapshots, decrements stock, appends a supplier-return-linked stock movement and audit event, and posts Dr Accounts Payable / Cr Inventory. Exact idempotency retries return the saved note; changed-payload reuse is rejected.

The workflow intentionally does not issue or track a cash refund or supplier credit balance when the bill is fully paid. It does not silently accept supplier invoice price differences, tax, freight, an advance, or a current catalog cost in place of receipt cost. These need distinct source records and reviewed posting rules. Focused local tests prove return quantity, stock, allocated-value and overflow bounds, while the migration test proves journal preservation, accepted `supplier_credit` source type, journal immutability and foreign-key integrity. The full desktop library suite passes (116 tests). Two command-transaction tests also pass: exact retry returns the existing note without duplicating stock, journal or audit writes; changed-payload reuse is rejected; and an injected audit failure rolls back the credit note, lines, stock movement and journal. Authenticated desktop visual review remains open.

## Problem

Stock receipts now post inventory against GRNI, but the existing `purchases` record represents a delivery, not a supplier bill or a payment. Its optional reference is labelled for either an invoice or a delivery number and it has no invoice due date, payment status, or settlement reference. Reusing sales `payments` or treating receipt as payment would misstate financial events.

## Usage and chosen shape

Users with invoice permission select one supplier and enter an invoice reference/date/amount, then allocate it across one or more of that supplier's received deliveries. A delivery may be billed in parts. Each allocation must fit the delivery's remaining GRNI, and all allocations must sum exactly to the entered invoice amount. A successful match posts Dr GRNI / Cr Accounts Payable and records the bill plus audit event atomically.

Owners and administrators who have settlement permission record a payment that has already occurred outside NimbleTill. They allocate it across one or more open bills; partial payments are allowed, but the allocations must sum exactly to the recorded payment and cannot exceed each bill's open balance. The journal posts Dr Accounts Payable / Cr the selected cash or tender-clearing account. This workflow records payment; it does not initiate or independently verify a bank/card transfer.

V1 has no editable drafts: each accepted bill or payment is complete and immutable. Invalid/unsupported invoice differences (tax, freight, price variance), currency changes, advances, and over-allocation are rejected with a clear reason. Corrections remain a source-scoped follow-up feature; no generic journal edit/reverse control is exposed.

## Domain records

- `supplier_bills`: business/store/supplier, normalized supplier reference, invoice and optional due date, currency, total, recorder, time, idempotency key.
- `supplier_bill_allocations`: bill, received purchase, allocated amount, business/store.
- `supplier_payments`: business/store/supplier, currency, method, optional external reference, amount, recorder, time, idempotency key.
- `supplier_payment_allocations`: payment, bill, allocated amount, business/store.
- Accounting source types `supplier_bill` and `supplier_payment`; system Accounts Payable account code 2000.

All ids and parent keys are store/business scoped; amounts are integer minor units and checked for overflow. Unique idempotency keys return the prior result only when the replayed payload and allocations match. Supplier invoice references are unique case-insensitively per business and supplier. Child rows and posted journals are append-only.

## Council synthesis

The accounting architect, finance operations, and migration reviewers independently recommended separate bill and payment records, with the receipt remaining the GRNI source. Their shared invariants are: do not infer settlement, do not reuse customer tender rows, match only same-store/supplier/currency receipts, never exceed open GRNI/AP, post source rows plus balanced journals and audit in one immediate transaction, and keep payments explicitly manual until a provider confirms them.

The chosen first slice supports exact receipt-value allocation only. The entered invoice or payment amount must equal its selected allocations. It does not silently route tax, freight, advances, or price differences to suspense. Invoice and settlement permissions are separated; built-in manager roles receive neither by default.

## Alternatives considered

- Mark a delivery itself as a supplier invoice: rejected because receipt references may be delivery numbers and receipts can predate invoices.
- Record a supplier payment as an expense or sales tender: rejected because it loses the payable link and can double count operating expenses or customer cash.
- Allow editable posted bills and payments: rejected because edits would invalidate immutable journal source values; later corrections need explicit bill credits, returns, or linked reversals.

## Boundary and next implementation step

The SQLite desktop workflow and hosted PostgreSQL workflow are separate implementations. Do not treat SQLite migration 0022's payment tables or desktop UI as hosted payment functionality. Hosted payment records, payable balances, source journal posting and station drawer-linked cash settlement are implemented. Local desktop physical return credits against unpaid bills are implemented, with focused command-transaction/retry/rollback coverage passing; authenticated visual acceptance remains pending. Hosted physical-return credits are implemented and passed restricted PostgreSQL migration/HTTP verification. Paid-invoice refund/credit treatment, price/tax/freight variances, supplier advances, non-drawer cash locations and broader source journal coverage remain open. Current verification and all remaining project work are tracked in [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md).

