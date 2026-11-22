# Candidate A: explicit transfer request lifecycle

## Problem

Cashiers need to answer “Can the customer buy this at another location?” with branch-level stock data, then either give a current branch recommendation or create a fulfillment request. In this repository, stores are branch-like stock owners, stations within a store share one inventory balance, and product rows are store-scoped. The standalone SQLite database cannot promise live data across independent devices, so authoritative availability and transfer writes ultimately require a shared tenant service or a single synchronized database authority.

## Usage (caller's view)

```ts
const result = await stockNetwork.availability({ productId, quantity: 2 });
// result.locations[] includes branch, optional station display context, onHand,
// reserved, availableToPromise, lastUpdatedAt and confidence.
```

```ts
const request = await stockNetwork.requestTransfer({
  productId,
  quantity: 2,
  fromStoreId: selectedSource,
  toStoreId: currentStoreId,
  customerId: customer?.id ?? null,
  customerName: customer?.name ?? null,
  customerPhone: customer?.phone ?? null,
  fulfillBy: "pickup_at_destination", // or customer_pickup_at_source / branch_delivery
  idempotencyKey,
});
```

```ts
await stockNetwork.dispatch({ transferId, revision, lines });
await stockNetwork.receive({ transferId, revision, receivedLines, discrepancyReason });
```

## Shape

### Core types

```rust
struct BranchAvailability {
    business_id: BusinessId,
    product: CanonicalProductRef,
    store_id: StoreId,
    store_name: String,
    on_hand: u32,
    reserved: u32,
    available_to_promise: u32, // derived: on_hand - reservations - safety_stock
    last_movement_at: Timestamp,
    observed_at: Timestamp,
    freshness: AvailabilityFreshness, // Live | Cached { age } | Unknown
}

enum TransferStatus { Requested, Accepted, Picking, InTransit, PartiallyReceived,
                      Received, Cancelled, Rejected, Exception }
enum Fulfillment { PickupAtDestination, CustomerPickupAtSource, BranchDelivery }

struct TransferRequest {
    id: TransferId,
    business_id: BusinessId,
    source_store_id: StoreId,
    destination_store_id: StoreId,
    status: TransferStatus,
    fulfillment: Fulfillment,
    customer_snapshot: Option<CustomerSnapshot>,
    revision: u64,
    expires_at: Option<Timestamp>,
    created_by: UserId,
    lines: Vec<TransferLine>,
}

struct TransferLine {
    product_id: CanonicalProductId,
    source_product_id: ProductId,
    destination_product_id: ProductId,
    sku_snapshot: String,
    name_snapshot: String,
    requested: u32,
    accepted: u32,
    dispatched: u32,
    received: u32,
}
```

### Public module boundary

```rust
fn availability_for_business(db, business_id, query: ProductMatch, requested: u32)
  -> Result<Vec<BranchAvailability>>;
fn request_transfer(db, actor, TransferDraft, idempotency_key) -> Result<TransferRequest>;
fn accept_transfer(db, actor, id, expected_revision, accepted_lines) -> Result<TransferRequest>;
fn dispatch_transfer(db, actor, id, expected_revision, dispatched_lines) -> Result<TransferRequest>;
fn receive_transfer(db, actor, id, expected_revision, received_lines, discrepancy) -> Result<TransferRequest>;
fn cancel_transfer(db, actor, id, expected_revision, reason) -> Result<TransferRequest>;
fn list_transfer_inbox(db, actor, filters) -> Result<Vec<TransferSummary>>;
```

Commands resolve actor permissions and business scope before calling domain operations. The domain layer owns validation, optimistic revisions, idempotency, stock changes and audit records, hiding SQL lifecycle details from UI and transports. A single SQLite `IMMEDIATE` transaction performs each transition; API implementation later uses a serializable transaction / row locks with the same transition rules.

### State and inventory rules

- Availability is per branch/store, never per register station. The UI may show stations as contact/collection points but must not imply separate station stock.
- A canonical company product ID links store-specific sellable rows. Introduce this before allowing transfers; never silently match by product name. During migration, seed canonical IDs and link branches by exact normalized SKU/barcode only when unambiguous, otherwise require an owner mapping screen.
- `available_to_promise = max(0, on_hand - active_customer_reservations - accepted_outbound_not_dispatched - safety_stock)`. Show the timestamp and “availability can change” until held.
- A request is a workflow record, not a stock mutation. Source branch acceptance allocates the accepted quantity and reduces availability. The physical dispatch transaction decrements source on-hand and writes a `transfer_out` movement; receiving increments destination stock and writes `transfer_in`. The in-transit quantity is not included in destination sellable stock.
- Receipt may be partial. Missing, damaged or excess quantities enter `Exception`, require a reason and authorized resolution; never erase/rewrite movements. `transfer_out`/`transfer_in` need a distinct movement reason and cross-store source references. Transfer inventory remains within one business and has zero P&L impact; inventory valuation follows existing cost policy, with inter-branch freight/cost adjustments deferred to a separately audited expense/journal flow.
- Every command checks both stores belong to the actor's business, are active, and actor has transfer permission for source/destination scope. Cashiers may search and submit a customer request; source acceptance and dispatch need inventory-transfer permissions. Admin/manager policy is configurable. Idempotency keys and expected revision prevent double dispatch/receipt and stale-screen writes.
- If live shared branch data is unavailable, results show `Unknown`/stale and direct the cashier to call; never display a stale positive quantity as confirmed or create an authoritative transfer in an unsynced device database.

### UI modules

- Register product search gets a “Check other branches” action when the selected item is low/out of stock. Results group by branch with available-to-promise, last updated, address/phone, and actions: “Tell customer where to buy”, “Request transfer”, or “Hold at branch” only when a server-backed reservation is supported.
- Cashier request captures customer contact with consent, requested quantity, preferred fulfillment, and an expiry. It creates no sale/payment. A request status panel exposes accepted, ready, dispatched, received, rejected, cancelled, expired and exception states.
- Inventory adds an “Inter-branch transfers” inbox with source/destination tabs, accept/reject, picking, dispatch and receive actions. Destination branch can receive against a transfer and resolve discrepancies.
- Admin settings add branch profile/contact, branch operating hours, transfer policy, default safety stock and eligible source branches. Stations remain independently configured under their branch and share branch inventory.

## Synthesis decision

Selected as the base by `.planning/arena/branch-stock-routing/cross-judge.md`. Graft Candidate B's independent expiring customer pickup reservation lifecycle and canonical business product mapping. Preserve the explicit transfer lifecycle because a simple “subtract here, add there” shortcut breaks inventory history and loses custody while goods are in transit.

## Tradeoffs accepted

- We accept introducing canonical product identity and an explicit mapping migration in exchange for safe matching of a product across branches.
- We accept a shared-service prerequisite for live availability and transfer guarantees in exchange for avoiding misleading local snapshots across disconnected tills.
- We accept two inventory movements and an in-transit state in exchange for accurate source/destination counts and auditability.
- We accept temporary reservation complexity in exchange for not overselling stock after the source branch confirms a customer request.

## Alternatives considered

- Search branches by SKU and show their `stock_quantity` only: simpler, but exposes stale-data and identity-ambiguity handling to the cashier, and has no durable way to request/track stock.
- Treat a transfer as a sale at one branch and receiving at another: reuses existing flows but contaminates sales, tax, cash and accounting reports; rejected.
- A company-wide shared stock pool: hides branch-specific ownership and cannot answer where the customer can physically purchase or collect an item; rejected.

## Open questions and risks

- Should a cashier request reserve stock immediately on source acceptance, or should the source branch explicitly accept a temporary customer hold with an expiry? The design supports an accepted allocation with expiry, defaulting to a short configurable window.
- Should destination branch sell and close the customer's sale, or should the original branch collect payment and create an inter-branch fulfillment order? The first release should avoid cross-branch paid orders until tax, refund and revenue-attribution policy is specified.
- Offline caches can show a useful hint but cannot guarantee availability. A cloud/API authority is required before presenting cross-device counts as live.

## Next implementation step

Establish canonical company product identity and shared, tenant-scoped branch availability in the API/local service before enabling transfer writes; meanwhile expose the correct stock ownership semantics in the product plan and tracker.
