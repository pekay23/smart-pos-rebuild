# Council candidate: extend purchase records with pre-receipt states

## Proposal

Use `purchases` for both purchase commitments and received deliveries by adding a `kind`/`status` discriminator (`order` versus `receipt`, and `draft`/`placed`/`partially_received`/`received`/`cancelled`). Add nullable `ordered_at`/`received_at`, `order_id` (self-reference to the order row), order revision, and ordered/received quantity fields on `purchase_items`. Draft and placed rows would have no `received_at`; each delivered tranche would become a child `purchase` row linked to the order, with actual unit cost and receipt time. Only child receipt rows could alter inventory, post GRNI, appear in received-purchase reports, or be matched to supplier bills.

An alternative single-row variant would update each order line's `received_quantity` and create a separate receipt-event table, but that introduces a second economic event collection. It is structurally closer to keeping the existing `purchases` table receipt-only and adding a distinct order aggregate.

## Grounded implications

- `commands::receive_purchase` currently creates one `purchases` row per receipt, inserts its `purchase_items`, updates stock and stock movements, writes audit, and calls `accounting::post_purchase_for_session` inside one `Immediate` transaction (`apps/desktop/src-tauri/src/commands.rs`, `receive_purchase`).
- `accounting::post_purchase_for_session` posts a purchase by the receipt ID. A pre-receipt order has no inventory/GRNI event; state-aware posting guards would be required if order and receipt rows shared this table.
- `purchase_reports::MATCHED_PURCHASES` currently joins all `purchases` rows and aggregates `purchase_items` with no discriminator. Adding order rows requires changing every purchase report path to filter receipt rows, including snapshot counts/totals and date queries.
- `supplier_billing::get_supplier_billing_workspace` offers any purchase with unmatched total as an allocatable receipt, and `match_supplier_bill` validates a purchase ID and remaining amount. Both require strict `kind='receipt'` checks or a draft/placed order could be treated as delivered and billed.
- `supplier_bill_allocations.purchase_id` references purchases by `(id, store_id)`. Existing bill history and accounting should continue to reference immutable receipt children, not mutable order totals.
- `PurchaseSummary` and `PurchaseInput` currently model a received event; query pagination is keyed by `purchases.rowid` snapshots. Orders in this table would contaminate or complicate this stable receipt query and its tests.
- Existing schema is receipt-shaped: `purchases.received_at NOT NULL`, `total_minor NOT NULL`, and `purchase_items.quantity > 0`; changing nullability or adding pending-state sentinels would make existing invariants weaker.

## Required changes if chosen

1. Add a `kind` discriminator with default `receipt` to preserve all existing rows. Ensure all order rows are store-scoped and constrain order/receipt relationships to the same store and supplier.
2. Model order estimates separately from actual receipt line cost. Every receipt child stores actual delivered quantity/cost and the link to order and order line. Never rewrite a receipt child after accounting or bill allocation.
3. Migrate reports to exclude order rows and preserve receipt snapshot semantics. Add regression coverage for historical totals and pagination.
4. Constrain supplier bill allocation and accounting posting to receipt children. Add an explicit non-receipt rejection at command boundaries, not just in UI filters.
5. Prevent order rows from affecting `products.stock_quantity`, stock movement history, cost valuation, accounting journals, supplier balances, and receipt counts.
6. Add explicit guarded transitions, revision checks, partial receipt remaining-quantity validation and same-transaction receipt/stock/GRNI/status updates.
7. Redesign Purchases UI to visibly separate commitments from received receipts even though they share a table. Existing order history/report results must remain receipt-only.

## Risks and verdict

This could be made correct only by treating order rows and receipt rows as different record kinds and filtering every consumer. That widens the migration and review surface across accounting, supplier billing, purchase reports, existing model types, and future features. A one-row-per-order design also cannot represent immutable partial delivery cost snapshots without a separate receipt ledger.

**Recommendation: do not adopt this design.** Keep `purchases` as the append-only economic receipt ledger and introduce a separate store-scoped `purchase_orders` aggregate. Link immutable receipt rows to order and line IDs and update received quantities/status in the same transaction. This preserves existing report, bill-allocation, inventory and accounting meanings, with fewer chances that an unreceived commitment is treated as an economic event.
