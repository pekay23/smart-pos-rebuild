# Grounding: cross-branch product availability and fulfillment

## Existing ownership model
`businesses` own `stores`; each store is a branch-like inventory scope. `products` are store-scoped and their SKU is unique only per store. `register_stations` belong to a store and multiple registers at that store share the same `products.stock_quantity`; station identity is currently for sales, cash shifts, device assignment and reporting, not separate stock.

## Stock mutation model
`stock_movements` are append-only-ish source records linked to a store/product/user, with `sale`, `receiving`, `adjustment`, `refund`, and `void` reasons. Stock-affecting workflows use transactions; sale/receipt/accounting writes must remain consistent. Purchase orders are store-scoped and receiving updates stock via the existing receipt/accounting transaction.

## Permissions / runtime
The local Tauri app stores one local SQLite DB. Session identity includes business_id and store_id. Commands should scope queries by business_id; read access is permission-gated in the command, and writes require explicit inventory/transfer permission. UI role hiding is supplementary, not authorization. Existing stores represent branches; stations within one store do not have separate inventory balances.

## Important gaps
The current setup provisions one store. Product rows are separate per store, with no canonical company-wide product ID; SKU uniqueness is per store, so cross-store matching requires business_id plus normalized SKU/barcode and ambiguity handling. No cross-device/cloud synchronization or server authority is implemented, so a device's local database cannot promise live stock at another branch. A local transfer workflow is only valid when both branch inventory ledgers are in the same database and online/shared transactional authority exists.

## Required business semantics
Availability must distinguish on-hand from sellable/available-to-promise. Pending customer holds and outbound transfer commitments reduce available quantities; incoming transit stock is informational until physically received. A transfer decrements source stock only on dispatch, increments destination stock only on receipt, records both movements and handles partial/damaged/missing receipt with an auditable exception. Same-branch station selection is informational because inventory is shared at branch level. A cashier can either create a time-bounded customer stock request/hold for admin/branch fulfillment, or give the customer verified branch address/contact and live availability timestamp. No cashier should claim another branch stock is guaranteed without a reservation or sufficiently fresh live response.
