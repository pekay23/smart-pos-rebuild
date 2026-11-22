# Candidate A: separate purchase-order aggregate

## Recommendation

Represent an order as an operational commitment separate from a received purchase. Goods receipt remains the only event that changes stock or posts inventory/GRNI. This matches the current domain boundary: `commands::receive_purchase` in `apps/desktop/src-tauri/src/commands.rs` is the atomic economic path, while `purchase_reports::query_purchases` reports actual receipts and supplier bills are matched against their receipt IDs.

## Schema

Add migration `0038_purchase_orders.sql` after migration 0037. Create:

- `purchase_orders`: `id`, `store_id`, per-store `order_number`, nullable `supplier_id`, `status` (`draft`, `placed`, `partially_received`, `received`, `cancelled`), optional supplier reference/notes/expected date, `revision`, created/updated/placed/cancelled timestamps, creating/updating user IDs. Unique `(store_id, order_number)` and `(id, store_id)`.
- `purchase_order_lines`: `id`, `purchase_order_id`, `store_id`, `product_id`, SKU/name snapshots, `ordered_quantity`, `received_quantity` defaulting to zero, estimated `unit_cost_minor`, timestamps. Check positive ordered quantity, nonnegative received quantity/cost, and `received_quantity <= ordered_quantity`. Unique `(purchase_order_id, product_id)` and `(id, purchase_order_id, store_id)`.
- Add nullable `purchase_order_id` to `purchases` and nullable `purchase_order_line_id` to `purchase_items`; link a receipt to at most one order and each item to its matching order line. Preserve existing rows as unlinked receipts.

Use composite store-aware keys or triggers so order lines cannot point across stores to a product, supplier, or order. Match established migration conventions, and avoid changing the receipt-centric report schema semantics.

## Lifecycle and boundaries

`draft -> placed -> partially_received -> received`; draft may be cancelled; placed or partially received may be cancelled while retaining prior receipts; received/cancelled are terminal. Only `purchases:write` may mutate and `purchases:read` may list/detail, following `receive_purchase` permissions. Every query and mutation must bind the active `session.store_id`.

Provide `purchase_orders.rs` commands for list/detail, save draft (create/update with expected revision), mark placed, cancel, and order-line-aware receipt. Register commands in both debug and release invoke handlers in `lib.rs`. Return typed header/detail/line models rather than overloading `PurchaseSummary`, which currently represents a posted receipt.

Extend `PurchaseInput` with an optional order ID and `PurchaseLineInput` with optional order-line ID using serde defaults, so legacy standalone receiving remains source and wire compatible. For linked receipts require all line IDs to belong to that order, each product to match its order line, supplier to match the order supplier, and received quantities to be positive and no greater than the remaining order commitment. Validate all lines before writing. In one existing `TransactionBehavior::Immediate`, insert the receipt and receipt items, update product stock/cost, record stock movement/audit, post accounting, increment order received quantities, and advance status. Any failure rolls all of it back.

Actual delivered unit cost is what goes into `purchase_items` and GRNI; never overwrite the order's estimated cost. This preserves expected-vs-actual variance for UI and later supplier-bill reconciliation. Order create/edit/place/cancel alone must not touch stock, purchase receipts, supplier balance, cash, or GL. Placement is only a local status confirmation; it does not imply an email or supplier transmission.

## Reorder planning and UI

Expose order detail in a separate section/panel in `PurchasesPage.tsx`, apart from receipt history. Support draft creation/edit, explicit “Mark placed”, cancellation, and receiving selected remaining quantities with actual unit costs and a supplier delivery reference. Receiving calls the existing `receive_purchase` economic path with order IDs and then refreshes both order status and receipt history. Keep purchase reports and bill matching receipt-only.

Change reorder suggestion quantity to `max(0, target_stock_quantity - on_hand - outstanding_placed_quantity)`. Count only placed/partially received remaining quantities; drafts do not represent committed supply. If suppliers differ across open orders, aggregate by product for the queue but retain per-order quantities in order details.

## Integrity risks and mitigations

- **Duplicate receiving / concurrent edits:** serialize with immediate SQLite transactions; increment revision on every transition/line update; reject stale revisions and over-receiving.
- **Cross-store ID substitution:** scope every query by active store and enforce same-store line/product/supplier links at the DB layer.
- **False accounting or stock:** make non-receipt lifecycle commands operational only; keep receipt and PO quantity/status updates in one transaction.
- **Order-price vs delivered-price confusion:** keep estimate and actual receipt cost in separate columns and snapshots.
- **Cancel after partial receipt:** preserve immutable receipt records and received quantity; disallow only future receiving.
- **Existing data loss / report drift:** nullable links keep legacy receipts intact; do not include open orders in historical purchase totals, supplier bill matching, or GL.
- **Reorder over-ordering:** subtract only open placed commitments, not drafts or cancelled/received orders.

## Acceptance checks

1. Fresh and legacy migrations apply; existing receipt, stock, and bill matching data are unchanged.
2. Draft/save/place/cancel have no inventory, movement, receipt, supplier-balance, or journal effects.
3. Wrong-store order/line/product/supplier references fail without mutations.
4. Partial linked receipt changes only received quantity, creates exactly one receipt, stock movement and accounting posting, and keeps the order open.
5. Completing via multiple receipts reaches `received`; excess, stale, replayed, cancelled, or already-complete receipt attempts fail atomically.
6. Actual cost variance is visible and does not rewrite estimated order cost.
7. Placed outstanding quantity reduces reorder suggestions; draft quantity does not.
8. Existing standalone receipt workflow remains supported; unreceived orders are absent from receipt history/reports and invoice matching.
9. Frontend supports draft/place/cancel/partial receive and clearly says placement is recorded locally.
