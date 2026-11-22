# Purchase order lifecycle implementation

## Phase checklist

- [x] Ground the existing receipt, stock, audit and GRNI posting flow.
- [x] Compare separate purchase-order aggregate designs and record a council decision.
- [x] Add a migration and store-scoped order read/write/transition commands.
- [x] Link actual receipts to placed orders and support partial receiving atomically.
- [x] Add local UI for draft, placement, cancellation, and partial receipt workflows.
- [x] Verify migration preservation and cross-store link guards; Rust suite; desktop binary compile; frontend build and lint.
- [x] Update the implementation tracker and keep supplier messaging and real printer/provider delivery explicit.
- [ ] Add command-level tests for atomic partial receipt/accounting and perform authenticated UI review of create, place, split receive, cancel, reorder suppression, and receipt history.

## Grounding notes

- `receive_purchase` is the economic event: it validates the active store and supplier, inserts `purchases` and `purchase_items`, increments product stock and current cost, adds stock movement, writes audit, and posts inventory/GRNI journal entries in one immediate SQLite transaction.
- Supplier bills are matched against received purchase receipts, not orders. Purchase orders must therefore remain non-accounting commitments until goods arrive.
- Existing purchase history queries are receipt-centric and date/snapshot paginated. They should remain unchanged for draft or placed orders.
- Existing reorder suggestions are planning-only and calculate target stock minus on-hand stock. Open placed-order quantities need to reduce these suggestions to avoid duplicate ordering.

## Design contract (reviewed)

- Store purchase orders separately from received purchases; statuses are `draft`, `placed`, `partially_received`, `received`, and `cancelled`.
- Draft save, placement, cancellation and receipts require `purchases:write`; all reads are store-scoped and require `purchases:read`.
- A draft does not touch inventory, supplier balances, cash, or the general ledger.
- Placement is an explicit operator confirmation that the order was sent externally. No supplier communication is implied.
- A receipt links each received line to its order line, allows quantities up to the remaining commitment, and atomically creates the existing receipt/stock movement/GRNI event and advances received quantities/status.
- Receipt unit cost is the actual delivery cost; variance from the ordered estimate is preserved and surfaced.
- Optimistic order revisions protect edits and transitions. Immediate SQLite transactions serialize receipt posting.
- Cancelled orders preserve already received quantities and cannot receive more. Supplier bill matching continues to target actual receipts.
- An open order's unreceived quantity reduces reorder suggestions; drafts do not count as ordered supply.
- Overage is rejected on linked lines and must be recorded as a separate delivery. Receipt request keys are persisted to make retries idempotent.

## Council decision

Two design candidates and a cross-judge were recorded under `.planning/arena/purchase-orders/`. The council chose a separate `purchase_orders` aggregate because the existing `purchases` tables are receipt ledgers used by accounting, receipt reports, and supplier-bill matching. Keeping only actual deliveries in those tables protects current financial semantics. The implementation adds store/business guard triggers for order suppliers/products and receipt/order-line links, plus optimistic revisions and receipt idempotency keys. Overage against an order is rejected with a prompt to record the unexpected goods as a separate delivery; quantities are never silently clipped.

## Implemented slice

- Migration 0038 adds store-scoped orders/lines and nullable receipt links, preserving legacy receipts unlinked.
- Store-scoped commands list orders, save/update drafts, place, and cancel with revision checks and audit events.
- Placed orders can be received in parts through `receive_purchase`; each receipt uses actual cost, appends stock movement and GRNI, advances received quantity/status in the same immediate transaction, and accepts an idempotency key.
- Purchases UI supports creating/editing drafts from reorder suggestions, local placement confirmation, cancellation, and partial receiving. Reorder suggestions subtract open placed quantities.
- Reorder exports include outstanding placed quantities; linked receiving shows estimated versus actual unit-cost variance. Database triggers reject cross-store suppliers/products and receipt items that do not match the linked order line.

## Verification recorded

- `cargo test --lib`: 86 passed, 0 failed after the lifecycle and store-guard implementation.
- Focused `purchase_order_migration_preserves_legacy_receipts_and_adds_scoped_links`: passed after asserting legacy preservation, same-store links, cross-store rejection, receipt-link matching, and idempotency uniqueness.
- `cargo check --bin smart-pos-desktop` and `cargo fmt --all -- --check`: pass.
- `npm run build` and `npm run lint`: pass. Lint reports existing `react(set-state-in-effect)` warnings in People, Products and Register; Vite reports the existing >500 kB main chunk.
- Authenticated live UI and printer/provider integrations are not verified by this code check.

## Open acceptance evidence

- Legacy databases upgrade with no new orders and all received purchase history intact.
- Wrong-store supplier, product, order, or order-line IDs cannot be read or mutated.
- Draft/place/cancel never modifies on-hand stock, stock movements, receipt history or journals.
- A partial receipt increases inventory and posts GRNI exactly once for only the received quantity; outstanding order quantity and status remain correct.
- Replayed or stale receipt commands cannot double receive; a unit-cost variance does not overwrite the expected order price.
- An order can be completed by multiple receipts; a fully received or cancelled order rejects further receipts.
- Open placed/partially received quantities suppress duplicate reorder suggestions without changing the reorder target.
