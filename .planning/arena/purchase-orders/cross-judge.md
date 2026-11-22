# Purchase-order lifecycle cross-judge

## Ruling

Adopt **Candidate A: a separate, store-scoped `purchase_orders` aggregate**, linked to the existing receipt records. Keep `purchases` and `purchase_items` receipt-only. This is the cleanest long-term boundary for multi-branch operation and future cloud clients: an order is a mutable operational commitment, while a receipt is an actual stock and financial event.

The current code gives receipt rows economic meaning in several independent paths:

- `commands::receive_purchase` inserts a receipt and its items, updates stock and product cost, records stock movements and audit, posts accounting, then commits all of them in one `Immediate` transaction.
- `accounting::post_purchase` reads the receipt timestamp, reference, total, and item lines, validates their reconciliation, and posts inventory/GRNI against the receipt ID.
- `purchase_reports::run_query` snapshots and paginates `purchases` rows as received-delivery history, aggregating receipt totals and item summaries by store.
- Supplier-bill workspace and matching allocate against `purchases` IDs and their remaining unbilled receipt totals. Those IDs are economic source records, not order commitments.

Candidate B would place mutable order rows into this receipt-shaped table. It would require every report, pagination snapshot, accounting entry point, bill-matching query, and future consumer to remember a kind filter. Any missed filter could count an unreceived order as spend, receipt, or billable delivery. Candidate A preserves existing semantics and avoids weakening the current `received_at NOT NULL` receipt schema.

## Required design adjustments

1. Treat order estimates and receipt actuals as separate values. A receipt's actual quantity, unit cost, reference, and timestamp are the source for stock and accounting; never mutate them when an order changes.
2. Do not hard-code `received_quantity <= ordered_quantity` as an absolute database invariant unless the product explicitly forbids over-delivery. Prefer an explicit receiving exception/overage path with authorization and audit, or a documented tolerance; never silently clip quantities.
3. Avoid `UNIQUE(purchase_order_id, product_id)` if the domain must allow the same product on multiple lines (e.g. separate prices, lots, or delivery terms). Either combine lines by policy or use stable line IDs and validate their individual remaining quantities.
4. Cancellation after partial receipt cancels only the unreceived commitment. Preserve received quantities and all receipt, stock, bill, and journal history.
5. An order marked placed is only recorded locally unless a supplier transmission/integration is implemented and confirmed. Do not imply that placing sends it externally.

## Critical invariants

- **Receipt-only economics:** only receiving creates `purchases`/`purchase_items`, stock movements, on-hand/cost changes, and inventory/GRNI journals. Create, edit, place, or cancel orders have none of these effects.
- **Atomic receiving:** in one immediate transaction, validate the active store, order and line ownership, product and supplier consistency, status/revision, and remaining quantities; then write receipt/items, stock/cost, movements, audit, accounting, received quantities, and order status. Any failure rolls back all of it.
- **Tenant and branch isolation:** scope every command/query by the active business and store as applicable. Enforce same-store composite references for order-to-line/product/supplier and receipt-to-order/line links at the database boundary; reject cross-store IDs even if submitted directly.
- **Idempotency/concurrency:** serialize receipt transitions, reject stale revisions and duplicate/replayed submissions, and ensure retries cannot create a second receipt or apply stock twice.
- **Billing and reporting remain receipt-based:** supplier bills allocate to receipt IDs only. Purchase history, totals, exports, accounting reports, and snapshot pagination include receipts only; open orders and estimates never appear as received cost.
- **Commitment-aware replenishment:** subtract only placed/partially received outstanding quantities from reorder suggestions. Draft, cancelled, and fully received quantities do not reduce the suggested requirement.
- **Migration preservation:** existing purchases remain valid unlinked receipts with identical IDs, totals, timestamps, item snapshots, stock, journals, and bill allocations. New nullable links must not require backfilling historical orders or change old report results.
- **Permission and audit:** follow `purchases:read` / `purchases:write` at command boundaries, audit all order transitions and receiving exceptions, and return typed order models distinct from `PurchaseSummary`.

## Conclusion

Candidate A is the recommended foundation. Candidate B's discriminated-record variant is technically possible, but it creates avoidable cross-cutting risk around existing accounting and supplier-billing source records. Keep the purchase-order lifecycle separate and join it to immutable receipt events through explicit IDs.
