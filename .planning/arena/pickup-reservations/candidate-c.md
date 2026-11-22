# Candidate C: shared stock allocations with sale-bound pickup completion

## Caller flow

A cashier at Accra finds two units at Kumasi, selects `Hold for pickup`, records the customer's name and phone with consent, and chooses a short expiry. The API atomically allocates those units at Kumasi and returns a reference, expiry, and destination contact details. No stock movement, tender, receipt, tax, or journal is created.

Kumasi staff see the reservation in their pickup queue and mark it ready after locating the units. When the customer arrives, a cashier opens the reservation in the register. The existing signed checkout request includes the reservation ID and expected revision. Successful checkout consumes its allocation, records the ordinary sale and stock movement once, and marks the reservation collected in the same transaction. A manager who has no cashier grant can prepare or cancel a hold but cannot complete the sale.

Cancellation releases stock immediately. Expiration releases stock at the stored server deadline even if no background job has run. A reservation cannot become collected merely because a worker clicks a generic status button.

```ts
const availability = await inventory.availability(productId)
const hold = await pickups.create({
  pickupBranchId, productId, quantity: 2,
  customer: { name, phone, contactConsent: true },
  durationMinutes: 60, idempotencyKey,
})
await pickups.transition(hold.id, { action: 'ready', expectedRevision: hold.revision, idempotencyKey })
await sales.commit({
  items, tenders, customerId,
  pickupReservation: { id: hold.id, expectedRevision: ready.revision },
  // Existing lease, station proof and cashier authorization still apply.
})
```

## Grounded constraints

- Hosted products already have canonical tenant IDs and branch assortment rows in migration 0008. Stations share their branch's quantity.
- `sales::commit` currently locks branch item rows in product-ID order and checks only `stock_quantity`. It would sell reserved units unless changed.
- `stock_transfers::transition` currently allows the destination to accept a request. Acceptance does not check or allocate source stock. Dispatch decrements source on-hand only. Source authorization and capacity must be established before acceptance becomes a guaranteed stock commitment.
- Cross-branch availability uses a transaction-local read scope. Reusing that scope to expose customer contact rows would broaden access unnecessarily.
- Ordinary owner/admin/manager roles lack `sales:checkout`; keep that distinction in pickup completion.

## Chosen structure

Add a small common allocation table for quantities promised to pickup customers and accepted outbound transfers. Keep customer details and workflow history in separate pickup tables. This gives every stock consumer one quantity calculation without exposing customer names and phone numbers to availability queries.

```rust
struct InventoryKey { tenant_id: Uuid, branch_id: Uuid, product_id: Uuid }
struct Availability {
    on_hand: i64,
    pickup_reserved: i64,
    outbound_committed: i64,
    available_to_promise: i64,
    observed_at: Timestamp,
}
enum PickupStatus { Held, Ready, Collected, Cancelled, Expired }
struct CreatePickup {
    pickup_branch_id: Uuid,
    product_id: Uuid,
    quantity: i64,
    customer: PickupContact,
    duration_minutes: u16,
}
enum PickupAction { Ready, Cancel { reason: String } }
struct PickupTransition { action: PickupAction, expected_revision: i64 }
struct SalePickup { id: Uuid, expected_revision: i64 }

// These operate inside the caller's transaction. They do not open their own.
async fn lock_inventory(tx, sorted_keys: &[InventoryKey]) -> Result<()>;
async fn availability(tx, key: &InventoryKey, at: Timestamp) -> Result<Availability>;
async fn allocate(tx, source: AllocationSource, key: InventoryKey, quantity: i64, expiry: Option<Timestamp>) -> Result<()>;
async fn release(tx, source: AllocationSource, cause: ReleaseCause) -> Result<()>;
async fn consume_for_sale(tx, pickup: SalePickup, sale_id: Uuid, quantities: &BTreeMap<Uuid, i64>) -> Result<()>;

async fn create_pickup(pool, authorized_station, key, raw_body, input) -> Result<PickupMutation>;
async fn list_pickups(pool, authorized_scope, filter, cursor) -> Result<PickupPage>;
async fn transition_pickup(pool, authorized_scope, id, key, raw_body, input) -> Result<PickupMutation>;
```

`inventory_allocations.rs` owns locking, capacity and allocation consumption. `pickup_reservations.rs` owns customer intent, visibility, transitions and events. `sales.rs` stays the sole owner of checkout, receipts and accounting. `stock_transfers.rs` owns logistics and consumes its allocation when dispatch succeeds.

## Schema and invariants

`pickup_reservations` contains tenant, requesting branch/station/member, pickup branch, one canonical product, quantity, product name/SKU snapshots, optional existing customer ID, consented contact snapshot, expiry, revision, persisted state, timestamps, optional sale ID, and creation idempotency key/hash. Start with one product per hold because the caller originates in product availability. Multi-product holds can add lines later without changing allocation ownership. Quantities are bounded and positive. Expiry is server-calculated with a 15-minute minimum, 24-hour maximum and 60-minute default. No silent extension on retries.

`pickup_reservation_events` is append-only and records held, ready, cancelled, collected and materialized expiry events. Each event has the actor or an explicit system expiry actor, effective timestamp, recorded timestamp, revision, operation key/hash and a bounded reason. Do not copy phone/name into general audit metadata. Unique reservation/revision and actor/key constraints prevent duplicate transitions. A deferred consistency constraint requires the matching event for every persisted state revision.

`inventory_allocations` contains tenant, branch, product, quantity, source kind, exactly one pickup/transfer source ID, optional expiry, released/consumed timestamp and release cause. Composite foreign keys reject cross-tenant links. Unique source/product prevents duplicate allocation. It contains no contact data. Pickup expiry equals its parent's deadline; transfer allocations have no automatic expiry. Source linkage constraints ensure a held/ready pickup has one matching allocation and a collected pickup has a matching same-branch sale and consumed allocation. Terminal allocation records remain for reconciliation.

At server time `t`, an active allocation is unreleased, unconsumed, and either has no expiry or has `expires_at > t`. Use one captured database timestamp per operation for all capacity checks. Availability is:

```text
pickupReserved = sum(active pickup allocations)
outboundCommitted = sum(active transfer allocations)
availableToPromise = max(0, onHand - pickupReserved - outboundCommitted)
```

Incoming transit stock is excluded until receipt. Safety stock remains a distinct future branch policy; do not subtract reorder point as a substitute. If safety stock is added now, give it its own field and include it in every allocation/checkout rule with an explicit staff override policy.

Persisted held/ready rows whose deadline has passed project as `expired` immediately. Allocation arithmetic uses the deadline directly. A periodic expiry task and authorized access can materialize an expiry event idempotently, with `effective_at = expires_at`. Expiry-job failure cannot extend the promise. A held event already records the immutable deadline, so the release remains auditable before materialization. Never allow a client to provide a system-expiry actor or bypass the deadline.

## Authorization and database boundaries

| Action | Origin cashier | Pickup cashier | Owner/admin/manager |
|---|---|---|---|
| Create from assigned station | Yes | Yes, including local pickup | Yes at an assigned station |
| Read originating hold | Own requests | No unrelated origin holds | Assigned origin branch |
| Read pickup queue/contact | Only if also assigned pickup station | Assigned pickup branch through station route | Assigned pickup branch |
| Mark ready | No unless pickup staff | Yes | Assigned pickup branch |
| Cancel | Own uncollected request | Manager escalation for another worker's hold | Either assigned participating branch, reason required |
| Collect/consume | Only at pickup branch through checkout | Existing cashier checkout | Only after acquiring a cashier role/lease |

Use `inventory:pickup:request`, `inventory:pickup:read`, `inventory:pickup:prepare` and `inventory:pickup:manage`. Origin cancellation also requires ownership, not just the request permission. Staff at a third branch cannot read contact details by guessing an ID. General availability readers receive aggregate allocation quantities only.

All tables use forced RLS. Reservation policies prove active tenant, identity, membership, participating branch and exact station scope. Do not copy the existing transfer shortcut that accepts any station-granted cashier without comparing the station ID. Allocation SELECT permits the inventory quantity projection under the existing authorized availability context or the current branch's stock mutation scope. Separate INSERT/UPDATE policies bind each allocation to a visible, authorized parent operation. Grant only required columns. Contact tables never get the availability policy. Events have SELECT/INSERT only; runtime cannot delete or alter history.

Cross-branch creation must not obtain permission to update the remote branch's stock row merely to acquire a row lock. Use the shared inventory advisory lock described below, which has no stock mutation privilege. Keep RLS authorization in every parent workflow and allocation write.

## Atomic transaction rules

Every stock-decreasing operation and allocation change must use the same lock namespace keyed by tenant/branch/product. Acquire sorted inventory keys before row locks, then read capacity. Existing sales, transfer dispatch/acceptance, manual adjustments and future sync ingestion must use this boundary. Add database guards on allocation writes and stock decreases so an omitted application check cannot consume active promises; guards use the same lock and quantity-only visibility. A reservation-specific advisory lock that checkout never takes provides no protection.

Create: validate input and permissions, start transaction and context, serialize the scoped idempotency key, replay an exact prior mutation or reject a changed payload, acquire the inventory key, verify active branch/canonical assortment and fresh capacity, insert reservation/allocation/event/audit, commit. Insufficient capacity returns 409 with current quantity and observed time, without contact data about competing holds.

Ready/cancel: lock the operation key, replay if applicable, discover the immutable inventory key, acquire inventory lock, lock the reservation, recheck revision and effective expiry, then update reservation and allocation together with event/audit. Ready retains the allocation. Cancel releases it. Terminal transitions cannot recreate a hold. A fresh hold requires a fresh request.

Checkout: authenticate the existing station proof/lease, acquire inventory locks for all products in sorted order, lock the specified reservation, verify pickup branch and effective status/revision, require the reserved product and full reserved quantity in the sale, and check unreserved portions of the cart against ATP. The reservation's own allocation is available only to this sale. Insert the sale, consume the allocation, decrement stock once, append collection event/audit and ordinary sale accounting, and commit together. Failed tender validation or any later failure rolls everything back. Exact checkout replay never consumes twice. Extra nonreserved products are allowed under ordinary ATP checks. Partial pickup is a later explicit workflow; do not silently release a remainder.

Transfer acceptance: authorize the source branch, lock all inventory keys in sorted order, check capacity and allocate accepted quantities before setting accepted state. Dispatch consumes that transfer's allocation while decrementing stock and posting transfer-out movement atomically. If dispatch ships less than accepted, release the unshipped remainder explicitly with a reason. Receipt changes destination on-hand normally. Existing destination acceptance behavior must change in API, UI, OpenAPI and verification together.

Existing accepted transfers need migration treatment. Preserve them and record their quantities as legacy outbound commitments. If legacy commitments exceed on-hand, expose the shortfall and block new promises until a source manager resolves it. Do not shrink historical acceptance amounts or manufacture available inventory. Add the missing source-manager cancellation/release path.

Negative stock adjustments need a truthful shortage workflow. A manager may atomically cancel or reduce affected promises with reasons while recording the real count under the same inventory locks. A two-request cancel-then-adjust flow creates a race for new holds. Ordinary adjustments that would breach active commitments return a conflict listing the affected internal hold references for authorized managers. Do not let a generic adjustment silently invalidate customer promises.

## API and browser controls

Use station-scoped create/list/read routes and a branch queue route for managers. Add a station pickup action route for prepare/own-cancel and a branch action route for management cancellation. Reuse one transition body and module implementation after scope authorization. All POST mutations require idempotency keys; stale revisions return 409. List routes are bounded and cursor-paged, with status filters and server time. Expiry is displayed from the server deadline.

The existing availability card gains `Hold for pickup` beside `Request transfer`; show each as a different customer outcome. Its sheet asks for quantity, customer name/phone, consent and expiry, then displays a reference and contact details on success. Keep the same mutation key across an uncertain network failure. A retry does not renew expiry. A failed or stale refresh disables create; cached branch contact information remains readable with a checked-at label.

The pickup branch gets a compact `Pickups` queue available from the register and branch workspace. Staff can search reference/consented phone, mark ready, inspect expiry and open in checkout. Opening into a nonempty cart first offers to hold the existing cart. The checkout shows the linked pickup and keeps its allocated quantity fixed until collection or explicit cancellation. The customer pays at the pickup branch using the existing tender UI. Reference alone is an operational lookup key, not an authorization secret.

## Verification that decides completion

Use independent PostgreSQL connections for races; the existing one-connection identity suite cannot prove concurrency safety. With one unit remaining, race two holds and verify exactly one succeeds. Race hold against ordinary checkout, transfer acceptance, dispatch and negative adjustment. Assert final on-hand, active allocations and stock movements, not only HTTP status.

Verify expiry at the exact boundary without a worker, ready-after-expiry rejection, idempotent expiry materialization, cancellation release, changed-payload conflict, stale revisions, and timeout/retry after commit. Verify that a ready action does not renew the deadline. Replay an old create after expiry and prove it does not recreate capacity; return the current effective reservation with the original operation result metadata.

Run denied third-branch, cross-tenant, wrong-station, removed-membership and pooled-context-reset cases under the restricted runtime role. Prove availability exposes quantities without contact fields. Prove manager-only identity cannot collect, signed pickup checkout consumes once, failure leaves the hold intact, and a sale at the wrong branch cannot consume it. Verify the normal sale/tender/journal/stock invariants still hold.

Migration verification must include previously accepted transfers with insufficient stock, source-approval correction, paired allocation/source constraints, and grant checks. Browser acceptance covers cashier origin, pickup cashier and manager, narrow/mobile layouts, lost connection, stale stock, expiry while payment is open and an existing held cart.

## Alternatives and decision

I chose a common quantity allocation table because pickup holds and accepted transfers compete with ordinary checkout for the same branch stock. The two workflows keep separate parents, permissions and events; only their inventory promises share a calculation.

Calculating reservations from a pickup table and transfer commitments from transfer headers avoids an extra table, but repeats lifecycle predicates in every stock consumer and makes the availability RLS policy depend on customer/transfer records. It can work if a single inventory module owns every query and the privacy projection is solved. It is the leaner alternative if the team cannot maintain source/allocation consistency constraints.

A standalone reservation CRUD module with a `collect` status action is rejected. Releasing its hold before sale would let another station take the units. Decrementing stock at collection and again at checkout would count the same pickup twice. Linking collection to signed checkout resolves both cases.

The smallest complete release is one-product expiring holds, staff preparation/cancellation, sale-bound full pickup, shared allocation checks in every stock consumer, source-authorized transfer commitment, shortage handling, aggregate availability and the two cashier/branch UIs. It is a cross-workflow change. Shipping only create/list/cancel may be a useful intermediate implementation, but it must remain feature-disabled and must not be marked a complete guaranteed pickup feature.
