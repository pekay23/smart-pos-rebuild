# Candidate A: inventory commitments and pickup through checkout

## Problem and current evidence

The hosted catalog already uses tenant-wide product IDs and branch-owned stock. A cashier can read active branches through `catalog::product_branch_availability`. A customer hold must now protect those units against other cashiers and outbound transfers while the customer travels to the pickup branch.

`sales::commit` currently locks each `branch_catalog_items` row and compares the cart quantity with physical `stock_quantity`. Transfer dispatch also checks physical stock. Transfer acceptance records accepted quantities without reserving source stock, and the current accept endpoint runs at the destination branch. Consequently, adding a reservation table and subtracting it in the lookup response would leave checkout and dispatch able to consume the held units. Inventory consumption must adopt the same commitment rule in this implementation.

The existing branch-stock cross-judge already requires distinct pickup and transfer lifecycles, immediate short-lived holds, canonical product IDs, source-owned stock, and purchase at the branch the customer visits. This candidate applies those decisions to the current hosted API.

## Usage, written from the caller's view

A cashier selects the desired item, opens branch availability and sees physical units, customer holds, accepted outbound transfers, available units and the observation time. The customer chooses a branch with sufficient available units. The cashier enters a customer name and phone, quantity and a duration, then chooses `Hold for pickup`.

```ts
const hold = await createPickupReservation({
  tenantId, branchId: currentBranchId, stationId,
  pickupBranchId, items: [{ productId, quantity: 2 }],
  customer: { name: 'Ama', phone: '+233244123456' },
  durationMinutes: 60, idempotencyKey,
});
// { reservationId, pickupCode, status: 'reserved', expiresAt, revision: 1, ... }
```

The successful response is the stock promise. A stale search result can produce an availability conflict; the cashier sees the current available quantity and can refresh, reduce quantity or call the branch. The confirmation shows the pickup branch address, contact details, branch-local deadline and pickup code. No charge occurs at the requesting branch.

The pickup branch's staff sees the reservation in its queue. A manager or cashier with pickup fulfillment permission marks it ready after setting the goods aside. The customer arrives and the serving cashier chooses `Sell held items`. This restores the held quantities into the existing register and binds the reservation ID and revision to the signed checkout payload.

```ts
const saleBody = {
  items, tenders,
  pickupReservation: { reservationId: hold.reservationId, expectedRevision: hold.revision },
};
const proof = await prepareAndSignSale(saleBody);
const receipt = await commitSale(saleBody, proof);
// The same transaction consumes the hold, reduces physical stock and records collection.
```

Managers can cancel a hold with a reason. The requesting cashier can cancel their own uncollected hold. Expiry releases its quantity even if no worker runs at that exact instant. Collection is the result of a completed sale at the pickup branch; there is no independent action that can mark goods collected while leaving physical stock unchanged.

## Shape and ownership

Use an `inventory_commitments` table for stock promises and a separate `pickup_reservations` aggregate for customer intent and contact data. Accepted transfers create commitments in the same table. Availability reads one commitment total per branch/product without exposing customer names, phones or transfer notes.

The public inventory API hides locking order, expiry, transfer commitments and allocation arithmetic behind three operations. Pickup owns customer details and lifecycle. Checkout owns the financial transaction and calls pickup consumption inside its existing database transaction.

```rust
// Private inventory module. All functions participate in the caller's transaction.
struct InventoryKey { tenant_id: Uuid, branch_id: Uuid, product_id: Uuid }
struct InventoryAvailability {
    on_hand: i64,
    customer_reserved: i64,
    outbound_committed: i64,
    safety_stock: i64,
    available_to_promise: i64,
    overcommitted: i64,
    observed_at: DateTime<Utc>,
}
async fn lock_inventory(tx: &mut Transaction<'_, Postgres>, keys: &[InventoryKey]);
async fn availability(tx: &mut Transaction<'_, Postgres>, key: InventoryKey) -> InventoryAvailability;
async fn require_consumable(
    tx: &mut Transaction<'_, Postgres>, key: InventoryKey, quantity: i64,
    consuming_commitment_id: Option<Uuid>,
) -> Result<(), InventoryError>;

// Pickup domain. Boundary DTO parsing produces validated input types.
struct CreatePickup {
    pickup_branch_id: Uuid,
    items: Vec<PickupQuantity>,
    customer: PickupContact,
    duration_minutes: u16,
}
enum PickupAction { Ready, Cancel { reason: String } }
async fn create(ctx: AuthorizedScope, key: IdempotencyKey, input: CreatePickup) -> PickupReservation;
async fn list(ctx: AuthorizedScope, query: PickupQueueQuery) -> PickupPage;
async fn transition(ctx: AuthorizedScope, id: Uuid, expected_revision: i64,
                    key: IdempotencyKey, action: PickupAction) -> PickupReservation;
async fn consume_for_sale(tx: &mut Transaction<'_, Postgres>, cashier: CashierScope,
                         reservation: PickupReference, sale_id: Uuid,
                         sold_quantities: &BTreeMap<Uuid, i64>) -> Result<(), PickupError>;
async fn expire_due(tx: &mut Transaction<'_, Postgres>, scope: ExpiryScope) -> usize;
```

`consume_for_sale` is private to checkout. The client never coordinates a separate release call with sale completion. This keeps the transaction boundary inside the service, per boundary discipline and domain modeling.

### Schema

`pickup_reservations` stores `id`, `tenant_id`, `requesting_branch_id`, `requesting_station_id`, `pickup_branch_id`, `requested_by_membership_id`, `customer_id` when linked, name/phone snapshots, pickup code, `status`, `expires_at`, `revision`, create key/hash, created/ready/cancelled/collected timestamps and actors, cancellation reason, and `collected_sale_id`. Status values are `reserved`, `ready`, `collected`, `cancelled`, `expired`.

`pickup_reservation_lines` stores tenant/reservation/product IDs, SKU/name snapshots and positive requested quantity. It has one row per canonical product, composite foreign keys, and a unique tenant/reservation/product key. Support 1–100 distinct products so later ticket-level holds use the same API as the current one-product branch lookup.

`inventory_commitments` stores `id`, tenant/branch/product IDs, positive `quantity`, exactly one of `pickup_reservation_id` and `stock_transfer_id`, nullable `expires_at`, nullable `released_at`, release reason and actor. Pickup commitments require the reservation expiry; transfer commitments have no deadline until the transfer leaves accepted status. Quantity and source identity are immutable. A unique source/product key prevents duplicate allocation. Source association constraints verify that pickup commitments belong to the pickup branch and transfer commitments belong to the source branch. Deferred constraints require exactly one commitment for each held line and each accepted transfer line.

`pickup_reservation_events` is append-only. It records tenant/reservation IDs, actor or system expiry, acting branch/station, event type, source revision, timestamp, idempotency key/hash, and bounded non-sensitive metadata. A unique tenant/reservation/revision key and tenant/reservation/actor/idempotency key protect the transition history. Create and every transition append an `audit_events` row in the same transaction. Audit metadata contains IDs and quantities; customer contact data remains in the reservation header.

Add `safety_stock_quantity` to `branch_catalog_items` with default zero and nonnegative validation. Do not reuse `reorder_point`, which is a purchasing alert rather than an allocation policy.

Use a cryptographically random human-readable pickup code with tenant uniqueness. The code helps staff search; authenticated branch authorization remains required. It is not a bearer credential and does not authorize a sale by itself.

### Availability rule

For one branch/product at database time `t`:

```text
customerReserved = sum(open pickup commitment quantity where expires_at > t)
outboundCommitted = sum(open accepted-transfer commitment quantity)
rawAvailable = onHand - customerReserved - outboundCommitted - safetyStock
availableToPromise = max(0, rawAvailable)
overcommitted = max(0, -rawAvailable)
```

Incoming transit stock remains informational until receipt changes physical stock. Every station in a branch uses the same branch/product key. API numbers are bounded to JavaScript-safe integers, and aggregate SQL uses NUMERIC before checked conversion to prevent overflow.

The existing availability response keeps `stockQuantity` as physical on hand and adds `reservedQuantity`, `outboundCommittedQuantity`, `safetyStockQuantity`, `availableToPromiseQuantity`, and `overcommittedQuantity`. Label each quantity explicitly in the web client. New promises use available-to-promise quantity.

### Authorization and RLS

| Capability | Origin cashier | Pickup cashier | Branch manager/admin/owner |
| --- | --- | --- | --- |
| Discover quantities | Assigned station scope | Assigned station scope | Assigned branch/station scope |
| Create a hold | Assigned station, self as requester | Assigned station, self as requester | Assigned scope |
| Read customer contact | Own originated holds | Holds fulfilled at assigned pickup branch | Holds touching an assigned branch |
| Mark ready | Only if also assigned at pickup branch | Assigned station at pickup branch | Assigned pickup branch |
| Cancel | Own active originated holds | Pickup fulfillment holds with a reason | Assigned origin or pickup branch with a reason |
| Collect | Existing signed cashier sale at pickup branch | Existing signed cashier sale at pickup branch | Requires an active cashier profile and the normal sale proof |

Add baseline permissions `inventory:pickup:create`, `inventory:pickup:read`, `inventory:pickup:fulfill`, and `inventory:pickup:manage`. The first three apply to appropriate cashier station grants and elevated assigned scopes. `manage` applies to owner/admin/manager branch grants. The service derives requester and actor IDs from the authenticated membership. It ignores body-supplied actor, tenant and branch ownership claims.

Force RLS on all new tables. Header and event read policies require current active identity/membership/tenant and the matrix above. Origin station cashiers only read their own outbound requests. Pickup station cashiers can serve the pickup queue. No membership in another tenant can read or mutate the reservation even when it knows its UUID or code.

Commitment read policy permits valid inventory-availability scope across the same tenant, exposing only stock allocation IDs, quantities and source IDs. Customer contact remains in separately restricted headers. Insert/update policies require the source reservation or transfer to be visible to the actor and verify its canonical branch/product relationship. Grant runtime SELECT and the exact INSERT/UPDATE columns only. Grant no DELETE, and no UPDATE on events, immutable line data, request hashes or contact snapshots. Use transaction-local GUCs only after API authorization, following the existing catalog pattern.

Creating a hold at another branch needs a stock lock without giving the requester permission to update that branch's catalog row. Use the same advisory transaction lock key for all stock consumers. The requester only reads remote stock through the existing availability scope and inserts its authorized reservation/commitment rows.

### Routes and request types

```text
GET  /v1/tenants/{tenant}/branches/{branch}/stations/{station}/pickup-reservations
POST /v1/tenants/{tenant}/branches/{branch}/stations/{station}/pickup-reservations
POST /v1/tenants/{tenant}/branches/{branch}/stations/{station}/pickup-reservations/{id}/events

GET  /v1/tenants/{tenant}/branches/{branch}/pickup-reservations
POST /v1/tenants/{tenant}/branches/{branch}/pickup-reservations/{id}/events
```

Station routes enforce station permissions; branch routes enforce manager permissions. Reuse the same domain operations after authorization. List supports bounded cursor paging, active/history status and pickup-code search. Events accept `ready` or `cancel`, expected revision and cancellation reason. Every write requires the existing visible-ASCII `Idempotency-Key` header.

```ts
type CreatePickupRequest = {
  pickupBranchId: string;
  items: { productId: string; quantity: number }[];
  customer: { customerId?: string; name: string; phone: string };
  durationMinutes: number;
};
type PickupReference = { reservationId: string; expectedRevision: number };
type PickupTransitionRequest =
  | { action: 'ready'; expectedRevision: number }
  | { action: 'cancel'; expectedRevision: number; reason: string };
```

Default duration is 60 minutes; allow 15–240 minutes in this first implementation. Show the actual returned deadline before confirmation. A later tenant setting can supply duration bounds without changing the lifecycle. Contact fields receive length and control-character validation. Country-aware phone formatting belongs to the existing customer contact boundary, and the raw international dialing detail must remain usable. Record explicit customer permission to share contact with the pickup branch in the create form and request.

## Transaction algorithms

Create parses and validates once, establishes transaction-local identity context, and locks the create idempotency scope. It returns the saved original response for an identical retry; a changed body conflicts. It validates active origin station, active pickup branch, tenant product identity and active assortment at both branches. It sorts affected branch/product keys and takes the shared advisory inventory locks. It reads current stock and all effective commitments, rejects insufficient availability with current quantities, then inserts header, lines, commitments, event and audit. The commit is the moment the hold exists. A failed transaction leaves no allocated units.

Every operation changing physical stock or effective commitments uses those inventory locks in the same sorted order. This includes ordinary sales, reservation consumption, transfer acceptance and dispatch, negative stock adjustments and any administrative write-off. Existing product-row locks remain useful after the advisory lock. Product archival must reject active commitments or require explicit cancellation first.

Transfer source acceptance acquires the source product locks and creates accepted outbound commitments. The current destination-side accept rule must change to source approval to match the selected product design. Add source-manager cancellation before dispatch so a declined or legacy overcommitted transfer has an explicit audited release path. Dispatch consumes its own commitment while protecting all other commitments, debits physical stock, and releases any accepted quantity omitted from a one-shot partial dispatch. Destination receipt increases physical stock. Existing accepted transfer records must be allocated during migration and any shortage exposed for review; do not silently claim that historically accepted quantities were guaranteed. New commitments and ordinary consumption remain blocked where stock is already overcommitted.

For pickup checkout, validate the signed body including reservation ID/revision. Acquire inventory locks, lock the reservation snapshot and recheck branch, status, strict database expiry and revision. Require the cart to contain every held line at at least its full reserved quantity; extra units use ordinary available stock. For each product, the sale may consume `availableToPromise + thisReservationQuantity`. Insert the sale and its normal movements, release the reservation commitments as consumed, update the reservation to collected with sale ID, and append collection/audit events in the same transaction. Payment or proof failure rolls everything back and preserves the hold. A completed sale's exact retry returns the saved receipt and never consumes twice.

Cancellation takes the same inventory keys, locks the reservation and checks authorization/revision. It releases commitments, updates the header, appends event and audit, then commits. Ready checks the pickup scope, revision and deadline without changing quantity. A ready hold keeps the original expiry.

Expiry safety comes from the active-commitment predicate using database time. It does not depend on a timer. `expire_due` materializes expired status and one expiry event, releasing due rows under inventory locks. Run it lazily before queue operations and on a small periodic hosted worker using a dedicated restricted system role. The system event stores the original deadline as effective expiry and processing time separately. SKIP LOCKED processing prevents two workers from expiring a row twice. The minimum shippable version includes this expiry runner and restart behavior; it cannot rely on one open browser tab.

## Browser workflow

The cashier's availability row gets `Hold for pickup` next to branch contact details. It uses available quantity, shows the reservation duration, customer consent/contact fields and a clear success state with code/deadline. Keep the cart intact while creating a hold. Network failure retains the same idempotency key for Retry; successful completion clears the attempt.

A `Pickups` register tab shows incoming holds for that branch and the cashier's outgoing holds. Active holds show a countdown derived from server deadline and observation time. Expired items remain visible in history. Incoming actions are `Ready`, `Sell held items` and `Cancel`; outgoing requests offer contact details and permitted cancellation. Manager branch navigation adds the same queue with management actions and history. Owner/manager sessions can inspect it, but the sale button requests an active cashier profile through the existing role switch.

Disable hold creation, ready and checkout conversion while offline or while authentication is stale. Cached branch details remain available with `Last checked` and an explicit stale state. Refresh after any hold, cancellation, expiry or sale; the server remains the final quantity authority. No automatic customer message is claimed until a configured delivery integration exists.

## Failure and verification cases

Verify the whole path against the restricted runtime role, not a superuser-only test. Required evidence includes:

1. Two concurrent stations reserve the last units. Exactly one succeeds, and all reservation/commitment/event/audit totals reconcile.
2. An ordinary sale cannot consume another customer's held units. A signed pickup sale can consume its own hold plus any unreserved extra quantity.
3. Accepted outbound transfers reduce available quantity. Concurrent transfer acceptance and hold creation cannot overallocate, and dispatch preserves other holds.
4. Expired and cancelled holds release quantity. Expiry works after a service restart and without a browser. Duplicate expiry work produces one event.
5. Wrong tenant, wrong pickup branch, wrong station, another cashier's outbound request and owner-without-cashier checkout are denied. Contact fields do not leak through availability responses or unauthorised queues.
6. Identical create/transition/sale retries return original results; changed payload reuse and stale revision conflict. Failed tender/proof leaves reservation and physical stock unchanged.
7. Collection writes one sale, its existing balanced journals/movements, one collected reservation event and one immutable sale link. A refunded sale does not recreate its consumed hold.
8. Negative adjustments and archival respect active commitments. Legacy overcommitment is disclosed and blocks new promises until resolved.
9. OpenAPI response parsing, desktop/tablet/mobile layouts, keyboard focus, server-based countdown, stale/offline states and branch contact links receive browser acceptance review.

## Tradeoffs and alternatives

We accept a shared commitment table and changes to every stock-consuming path in exchange for an enforceable stock promise. Updating only the lookup is smaller, but it cannot fulfill the customer promise under concurrent sales.

We accept checkout integration in this milestone because collection must reduce physical stock and preserve the existing cashier-only financial controls. A standalone `collect` endpoint would require an additional inventory handoff/accounting workflow and a reconciliation process for later payment.

The main alternative is to calculate holds directly from reservation headers and transfer lines. That avoids the commitment table, but every caller then needs privileged aggregate access to several lifecycle tables, including customer-bearing headers, and must duplicate which statuses consume inventory. A commitment table gives availability one non-sensitive representation and leaves customer/transfer lifecycles private.

Another alternative is a mutable reserved-count column on every branch product. It makes reads cheap but requires every expiry, cancellation and crash recovery to maintain the counter exactly. Expiry is time-based, so a stale counter can retain released stock or leak promised stock. Effective commitments plus database-time filtering make expiry safe even while the materialization worker is unavailable.

## Recommended scope and next implementation step

Ship multi-line expiring pickup holds, create/list/ready/cancel, source transfer commitments, shared inventory locking, signed pickup checkout consumption, an expiry runner, and the cashier/manager queue together. Keep cross-branch payment and partial collection for later product work. Full collection at the pickup branch is a complete first workflow and preserves the earlier revenue-ownership decision.

Start with the shared inventory commitment module and its restricted PostgreSQL concurrency scenarios, then integrate every existing stock consumer before enabling `Hold for pickup` in the client.
