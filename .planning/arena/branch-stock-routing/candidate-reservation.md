# Candidate B: canonical catalog, branch discovery and optional customer reservation

## Problem

Cashiers need trustworthy stock visibility across the company, then a way to help the customer buy the item without pretending a search result is a reservation. Today each branch is represented by a `store`, products belong to a store, and stations share the branch's stock balance. A store-local device cannot truthfully report other branches until there is a shared, current stock authority. The design should first give branches the same company product identity, then make availability freshness and reservation state explicit.

## Usage (caller's view)

At checkout, after the cashier scans or selects the item, the register asks the company inventory service for branch availability:

```ts
const view = await inventory.findAvailability({ productId, quantity: 2 });
// results identify branch, available-to-promise quantity, last sync time, and confidence
```

If the customer wants the item held for pickup:

```ts
const request = await inventory.requestPickup({
  productId, quantity: 2, sourceBranchId: view.bestBranchId,
  customer: { name, phone }, expiresInMinutes: 60,
});
```

If they prefer to visit another branch, the register can show its address, opening hours and contact details, with a freshness label; it does not imply a stock guarantee.

## Shape

Make the company catalog canonical and branch stock a separate aggregate keyed by `(business_id, product_id, branch_id)`. Existing `products` become branch-specific sellable records linked to a `catalog_product_id` (or are migrated into canonical products plus branch assortment rows). SKUs/barcodes belong to the canonical product where globally assigned; legacy duplicate identifiers across branches must be reconciled during migration instead of silently joined by display name. `register_stations` remain children of a branch and never get their own quantity unless the business explicitly adds stockroom/bin locations later.

```ts
type BranchAvailability = {
  branchId: string; branchName: string; address: string | null;
  onHand: number; reserved: number; outboundCommitted: number;
  availableToPromise: number; observedAt: string; freshness: "live" | "recent" | "stale";
};
type AvailabilityQuery = { productId: string; quantity: number; excludeBranchId?: string };
type PickupRequest = {
  id: string; businessId: string; productId: string; quantity: number;
  sourceBranchId: string; destinationBranchId: string; customerId: string | null;
  status: "requested" | "reserved" | "ready" | "collected" | "expired" | "cancelled";
  expiresAt: string; revision: number;
};

interface InventoryService {
  findAvailability(query: AvailabilityQuery): Promise<BranchAvailability[]>;
  requestPickup(input: CreatePickupRequest): Promise<PickupRequest>;
  transitionPickup(input: { id: string; revision: number; action: "accept" | "ready" | "collect" | "cancel" }): Promise<PickupRequest>;
}
```

The API calculates `availableToPromise = max(0, onHand - activeCustomerReservations - outboundTransferCommitments - safetyStock)`. Search results include on-hand and in-transit separately so the cashier cannot confuse them. A pickup request reserves source stock atomically with a revision check and expiry. Failed or stale requests return current availability. Request, reservation, expiry, collection and cancellation are auditable and idempotent. The source branch continues to own the sale and receipt; the destination branch owns the physical pickup handoff, with an explicit inter-branch settlement policy added before financial consolidation.

Module boundary: `inventory` owns canonical product matching and availability; `fulfillment` owns pickup reservations and their lifecycle; a later `transfers` module owns physical stock movement between branches. The desktop register calls one typed API adapter. The server validates the tenant, branch membership, role permission and current reservation state. Local clients show last sync and queue read-only availability refreshes while offline; they must disable reservation and label cached results stale.

## Synthesis decision

Candidate B was not selected as the base. Its canonical company product identity and separation between customer pickup reservations and physical branch-to-branch transfer were grafted into Candidate A; see `.planning/arena/branch-stock-routing/cross-judge.md`.

## Tradeoffs accepted

- We accept a catalog identity migration and duplicate-SKU reconciliation in exchange for precise cross-branch matching and a future web/API source of truth.
- We accept a server dependency for live availability and reservation in exchange for avoiding false stock promises and concurrent over-reservations.
- We accept two fulfillment lifecycles (customer pickup and company stock transfer) in exchange for making customer intent distinct from internal logistics.
- We accept displaying an explicit stale state offline in exchange for useful cached branch context without asserting current inventory.

## Alternatives considered

- Search every branch by normalized SKU/barcode without canonical product identity. This hides little complexity: callers must handle collisions, catalog drift and accidental matches. It is acceptable only as a temporary migration audit, not the long-term product contract.
- Add a `branch_id` column to existing products and keep all inventory in a single table. It eases the initial migration but exposes store/catalog identity coupling to every caller and makes branch assortment, price differences and canonical merchandising hard to evolve.
- Reuse a stock transfer to represent a customer request. It exposes internal shipping workflow to the register and moves inventory even when the customer simply wants to visit the other branch; rejected because the intent is different.

## Open questions and risks

- Should a pickup request reserve stock immediately, or wait for a branch worker to accept it? Default: reserve atomically immediately, with short expiry and clear release on decline/expiry.
- Which branch owns revenue, tax and returns if the customer pays at Branch A but collects at Branch B? This design avoids cross-branch payment until a settlement policy is approved.
- What freshness window is acceptable for cashiers to recommend a branch without a hold? Until product policy is set, show timestamp and never label a stale count as available.

## Next implementation step

Build the API-owned canonical product/branch-stock model and live availability endpoint, after the tenant API and shared synchronization foundation exists; the current single-device SQLite app can only implement a clearly labeled local lookup, not reliable live multi-branch inventory.
