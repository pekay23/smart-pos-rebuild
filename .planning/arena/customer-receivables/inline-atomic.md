# Candidate: extend the existing atomic sale commit

## Caller usage

The caller submits the existing signed `sale.commit` request with an optional explicit credit component. It never submits a credit limit, due date or captured-payment claim for that component.

```json
{
  "customerId": "customer-uuid",
  "items": [{ "productId": "product-uuid", "quantity": 1 }],
  "tenders": [{ "paymentOptionId": "option-uuid", "amountMinor": 4000, "reference": null }],
  "onAccount": { "amountMinor": 6000, "expectedTermRevision": 3 }
}
```

Omitting `onAccount` retains full-payment checkout. A full-credit sale permits an empty tender array. Positive tenders plus the explicit credit component must equal the authoritative priced total exactly. A successful response includes paid amount and a compact invoice snapshot containing ID, original amount and due timestamp; it never returns the customer's limit or other branches' invoice details. The response from an exact retry uses the saved invoice snapshot.

## Type and function sketch

```rust
struct OnAccountInput {
    amount_minor: i64,
    expected_term_revision: i64,
}

struct CreditSnapshot {
    revision: i64,
    limit_minor: i64,
    due_days: i32,
}

struct CommittedReceivable {
    id: String,
    original_amount_minor: i64,
    due_at: String,
}

// Extend the existing input and response; retain sales::commit as transaction owner.
struct CommitSaleInput { /* existing fields */ on_account: Option<OnAccountInput> }
struct CommittedSale { /* existing fields */ paid_minor: i64, receivable: Option<CommittedReceivable> }

async fn prepare_sale_credit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    customer_id: Uuid,
    currency: &str,
    requested: &OnAccountInput,
) -> Result<CreditSnapshot, SaleError> {
    // Acquire the customer/currency transaction lock; select authorized terms;
    // compare expected revision; calculate tenant-wide outstanding exposure;
    // reject disabled/missing terms, overflow or requested amount above capacity.
    // not implemented
}
```

Use distinct errors for missing/disabled terms, changed terms and exceeded credit. Only the exceeded-credit response may include an authorized selected-customer aggregate if the UI needs it; do not use a generic database error for these expected decisions. The helper hides the selected-customer exposure context and arithmetic; it cannot commit or open a nested transaction.

## Module map

- `apps/api/src/sales.rs`: input/response extension, credit preparation and invoice insertion within `commit`, invoice-aware exact replay.
- `apps/api/src/sale_accounting.rs`: debit `1100` Accounts Receivable for the saved on-account component; validate paid plus receivable equals total.
- Existing hosted accounting report account mapping: give `1100` its asset classification and readable name.
- `apps/api/src/lib.rs`: map explicit credit errors to stable conflict responses; keep the existing cashier-only authorization, lease and signed proof requirements.
- A new PostgreSQL migration: selected-customer exposure read policy, due-date snapshot constraint and required least-column grants. Do not alter already verified 0066/0067 migration bytes after deployment.
- Hosted OpenAPI contract: optional credit component, compact invoice response and error definitions.
- Existing restricted-role HTTP test harness: atomicity, concurrency, scope and retry coverage. No separate second checkout endpoint.

## Transaction and lock sequence

1. Parse positive safe-integer on-account amount and positive expected term revision. Reject credit without a customer and ambiguous zero-valued credit objects at the boundary. Permit zero tenders only when a credit object is present; still reject more than eight tenders and nonpositive tender entries.
2. Use the existing transaction, authenticated tenant/branch/station context, station advisory lock, active cashier lease and signature verification. Preserve the shared-ticket lock/fingerprint path.
3. Resolve completed-ticket and station idempotency replays before applying current credit rules. A sale already committed remains replayable after the manager changes or disables credit. Move the existing active-customer validation after the replay path so an archived customer does not turn a saved response into a new-sale decision.
4. Resolve authoritative branch currency. For a new credit sale, acquire one advisory transaction lock keyed by canonical tenant/customer/currency, then select the current terms and compute selected-customer tenant-wide exposure. The term revision must match the confirmation snapshot, limit must be positive, and `exposure + requested` must be within both the limit and the supported integer range.
5. Retain the established inventory locks and server pricing. Check `sum(tenders) + onAccount == total`. Existing payment-option/reference validation and drawer requirements apply only to actual tenders. A full-credit sale has no drawer cash inflow.
6. Insert `hosted_sales.on_account_minor`, its sale lines, captured/manual-recorded tenders and stock movements as one mutation. Insert the invoice using the source sale's exact `created_at`, customer, branch, currency, cashier and on-account amount; snapshot the locked revision, limit and due days. Derive due time on the server from issued time plus the configured number of 24-hour periods.
7. Post the journal, append the sale audit event including invoice ID/on-account amount, consume proof, and complete any shared ticket in the same transaction. Deferred sale/tender/invoice guards must pass at commit. Any failure rolls back all of these records, stock changes and proof consumption.

The lock order for new callers must be documented: station (where relevant), held ticket (where relevant), customer-credit key, inventory product keys. Term edits acquire only the customer-credit key. Collection and adjustment paths acquire station before customer-credit when they involve a drawer. Do not let future collection/refund paths acquire these in the reverse order.

## Exposure and RLS

Migration 0067 currently permits only branch manager reads and the issuing cashier's own-sale reads. Summing that result is incorrect for tenant-wide credit limits: the cashier would miss other branches and other cashiers' invoices.

Add a transaction-local selected-customer context, set only inside the authorized credit helper, and a narrowly scoped invoice SELECT policy for an active authorized cashier/manager in the current tenant and branch/station. It matches that exact customer and requested currency; it must not remove forced RLS or grant unrestricted tenant invoice reads. The helper returns only the aggregate and terms to its caller. Public cashier endpoints must never expose invoice rows across branches. Clear the extra context after the aggregate so later queries and deferred triggers continue under the ordinary branch context; transaction-local settings also reset with commit/rollback.

No collection or credit-adjustment tables exist yet. In this schema slice, outstanding exposure equals original invoice amounts because invoices cannot be settled. When collections/credits are added, the same single aggregate must subtract only immutable committed allocations and approved credit adjustments. Never subtract a client estimate or an unverified provider event.

Cashier `SELECT ... FOR UPDATE` on the terms row is not compatible with the current least-column grants and RLS: it requires UPDATE authority the cashier does not have. Use a tenant/customer/currency advisory transaction lock shared by every credit-affecting write, plus a normal authorized SELECT. This is a deliberate refinement of the plan's row-lock wording and avoids granting cashiers term mutation privileges. The alternative is a narrowly audited database function that performs only the lock/read; it is a larger security surface and is unnecessary if all runtime writes obey one lock contract.

Term changes also use the customer-credit lock before reading and updating the revision. A manager lowering the limit below existing exposure is allowed only as an explicit policy decision and blocks new credit; it never rewrites invoices. Customer merge/archive must either use the same credit lock and preserve invoice identity or reject while open exposure exists.

## Ledger behavior and corrections

`post_sale` reads the persisted on-account amount, checks nonnegative safe amounts and `tender_total + on_account == total`, and accepts no tender rows only when the receivable covers the total. It appends an AR debit for the unpaid amount alongside actual tender-clearing debits; revenue, tax, inventory and COGS continue using immutable existing snapshots. AR is an asset account. Existing cost/tax partial-coverage flags remain unchanged.

Credit is not a synthetic tender, provider capture or drawer inflow. Do not add an "on account" payment option.

Existing refunds cannot safely pay out an on-account sale's total while its invoice stays outstanding. Until source-linked receivable credit/refund allocation is implemented, fail closed for these sale refunds/voids with an explicit unsupported-correction response. This backend slice must stay out of general customer-facing rollout until collections, credit corrections, receipt disclosures and the full accounting path are complete, as required by the receivables plan.

## Idempotency and rollback

The existing raw signed body hash includes the optional credit object. An exact retry resolves the original sale and invoice before recalculating stock or capacity, including across shared-ticket station recovery. Changed credit amount/revision/customer with the same key conflicts. Invoice uniqueness `(tenant_id, sale_id)` prevents a second invoice even if a later caller regresses the retry path. The stored sale's on-account component and immutable invoice drive replay output and accounting, never the current term row.

If invoice insert, journal insert, audit insert, proof consumption, shared-ticket completion or deferred validation fails, rollback leaves the original ticket/stock/proof unchanged. Do not catch these failures and partially commit the sale.

## Verification rubric

1. Fully paid existing checkout retains behavior; partial and full-credit checkouts reconcile captured tenders, AR, revenue/tax and inventory entries.
2. Missing customer, missing/zero limit, wrong currency, stale revision, insufficient remaining credit and overflow fail with zero committed sale, invoice, stock, audit and journal changes.
3. Two stations/branches race for the same remaining credit: at most the allowed exposure commits. A simultaneous term reduction and checkout serializes under the shared credit lock.
4. Exact retries after terms change/disable and across shared-ticket recovery replay one sale/invoice/journal; changed payloads conflict. Customer archive cannot invalidate a saved response.
5. An injected audit/journal failure rolls back the sale, invoice, stock/tenders, ticket completion and proof consumption together.
6. Cashiers cannot write terms; a selected customer aggregate includes other branches in the same currency but exposes no other customer's or tenant's invoice detail. Missing or mismatched context is denied.
7. Full-credit sales require no cash drawer and create no drawer inflow; mixed cash credit sales require the existing open drawer and record only the cash amount.
8. Unsupported credit-sale refund/void attempts fail explicitly until correction allocation is implemented; no cash leaves and exposure stays unchanged.

## Alternatives and rationale

The chosen shape keeps `sales::commit` as the transaction owner because it already owns stock, tenders, proof, held-ticket completion, audit and accounting. A credit extension adds a balance component within that existing atomic boundary. One focused helper hides the complex lock/exposure rules; it does not split financial completion across modules or transactions.

A separate `receivables::checkout` orchestrator would require exposing sale internals or duplicating the large signed-station, shared-ticket and inventory sequence. That adds two sources of checkout truth and makes it easier for credit to be posted after sale commit. Reject that shape for this slice. A broader receivables module is appropriate when collections, aging and credit adjustments have enough behavior to own; it can then own the credit preparation helper without taking transaction ownership away from sales.

A SECURITY DEFINER aggregate/lock function could hide all cross-branch access behind a database return value. It would add privilege/search-path/function-owner maintenance and create a second place for business rules. Selected-customer RLS and a shared advisory lock fit the existing trusted transaction-context pattern with a smaller API surface; use the DB-function alternative only if a future threat model requires the runtime role itself to be unable to select invoice detail even within the selected-customer context.
