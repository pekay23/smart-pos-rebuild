# Candidate: actor scoped event log with serialized ticket claims

## Caller usage first

1. Register boot loads device-local tickets and pending sync operations immediately, then fetches the branch's shared ticket index if connected.
2. A staff member with branch `sales:read` can list shared ticket summaries and fetch a ticket's event-derived snapshot. This path never returns a lease token and never grants claim, edit, discard, or checkout authority.
3. A cashier at the active station acquires the existing station lease, then calls `claim(ticketId, expectedRevision, leaseId, leaseToken)`. Claim is an online compare-and-set. The cashier may then append edits while the claim remains valid.
4. Before checkout, the UI freezes the exact `CheckoutRequest` bytes/value and the idempotency key already generated for that request into a `checkout_prepared` event. The API accepts it once for the claimed revision and stores the canonical body and key. Retries use those stored values unchanged; the event cannot be replaced or removed.
5. Checkout calls the existing signed station checkout endpoint with that exact body/key and proof. A successful result appends `checkout_completed` with the resulting sale id. An uncertain result stays prepared and can only be retried unchanged. A completed ticket is no longer claimable.
6. If the station is offline, the register continues to use its local ticket and writes local-only events to an IndexedDB outbox with stable event UUIDs. On reconnect, it submits them in order with the same IDs. A local offline draft is not shown as a globally claimed ticket; acquiring or renewing a shared claim requires the server.

## Domain and API contract

All ids below are opaque UUIDs. Tenant and branch are explicit in route scope and repeated in stored rows as integrity keys.

```ts
type TicketSnapshot = {
  ticketId: string; tenantId: string; branchId: string; revision: number;
  createdAt: string; updatedAt: string; createdBy: string;
  label: string; lines: Array<{ productId: string; quantity: number }>;
  selectedCustomerId: string; selectedPaymentId: string; reference: string;
  pickupSale: PickupSale | null;
  pendingCheckout: { body: CheckoutRequest; idempotencyKey: string } | null;
  status: 'open' | 'checkout_pending' | 'completed' | 'discarded';
  claim: { membershipId: string; identityId: string; stationId: string;
    leaseId: string; expiresAt: string } | null;
};

type TicketEvent = { eventId: string; ticketId: string; tenantId: string;
  branchId: string; actorIdentityId: string; actorMembershipId: string;
  stationId: string | null; leaseId: string | null; baseRevision: number;
  kind: 'created' | 'edited' | 'claimed' | 'released' | 'checkout_prepared' |
    'checkout_completed' | 'discarded'; occurredAt: string; payload: unknown };

GET  /v1/tenants/{tenant}/branches/{branch}/held-tickets?cursor=...
GET  /v1/tenants/{tenant}/branches/{branch}/held-tickets/{ticket}/events?afterRevision=N
POST /v1/tenants/{tenant}/branches/{branch}/held-tickets/events
POST /v1/tenants/{tenant}/branches/{branch}/held-tickets/{ticket}/claim
POST /v1/tenants/{tenant}/branches/{branch}/held-tickets/{ticket}/release
```

`GET` requires branch `sales:read`; it returns summaries/snapshots without lease secrets. `created`, `edited`, `discarded`, claim/release and checkout-prepared writes require `sales:checkout` at the station plus a valid station lease; this deliberately keeps managers/owners with read permission out of transaction flow. Claim/release routes accept lease id and token in an authorization header/body field but never persist or echo the token. Every mutating request carries a client-generated event UUID and expected ticket revision; duplicate event UUID returns the original result, while stale revision returns `409` plus current revision. The claim itself uses the same lease proof validated against the active station lease lock and expiry, not merely a lease id supplied by the caller.

## Storage and serialization

- `held_ticket_streams`: immutable ticket identity and tenant/branch scope; unique `(tenant_id, branch_id, id)`.
- `held_ticket_events`: append-only, unique `(tenant_id, branch_id, event_id)` and `(tenant_id, branch_id, ticket_id, revision)`, with actor/membership/station/lease provenance and JSONB event payload. No UPDATE/DELETE grant to runtime role.
- `held_ticket_heads`: mutable projection with current revision/status and current claim tuple `(membership_id, identity_id, station_id, lease_id, expires_at)`. This is a rebuildable index, not the audit record.
- optional `held_ticket_sync_receipts`: idempotency result for event UUIDs if event uniqueness alone does not cover response replay.

All writes run in one transaction. Lock `held_ticket_heads` `FOR UPDATE`, validate tenant/branch and expected revision, validate the current active station lease (`station_actor_leases` plus `station_actor_lease_locks`, status and `expires_at > now()`), insert event, update projection, commit. Claim is serialized on that same head row: it succeeds only if no unexpired claim exists, or if the existing claim's lease is no longer active/has expired; in the latter case the same transaction appends a reclaiming `claimed` event and replaces the projection. Release is likewise a lease-checked event. Heartbeat/lease renewal updates the projected claim expiry from the authoritative lease row; a janitor is not required for correctness because claim checks treat expired/inactive leases as reclaimable. SQL constraints and a single transaction prevent two cashiers from both winning a claim.

`checkout_prepared` is a terminally immutable payload choice. Enforce one prepared event per ticket and reject subsequent edit/discard/checkout-prepared events. A claim may expire during an uncertain checkout; another cashier can reclaim but must see the frozen pending checkout and can only retry that same body/key through the standard signed checkout flow. Since sale idempotency is already scoped and enforced by checkout, the event log must never mint a replacement key or mutate the request after preparation.

## Module map and signatures

- `apps/api/src/held_tickets.rs`: `list(pool, identity, membership, tenant, branch, cursor) -> Result<Page<TicketSummary>, Error>`; `events(pool, scope, ticket, after_revision) -> Result<Vec<TicketEvent>, Error>`; `append(pool, actor, scope, station, lease_proof, event_id, ticket, expected_revision, kind, payload) -> Result<TicketSnapshot, Error>`; `claim(...)` and `release(...)` wrap the same transaction helper with constrained event kinds. Keep the lease verification helper reusable from the existing `station_lease` module or add a narrowly scoped `require_active_lease_tx` there so lock validation shares one source of truth.
- `apps/api/src/lib.rs`: route declarations and thin handlers. Read handlers call `authorize_catalog_scope(..., None, "sales:read")`; all writes require a station path and `authorize_catalog_scope(..., Some(station), "sales:checkout")`, then delegate lease-bound changes to the module. Existing sales checkout stays the sale-commit boundary.
- `apps/api/migrations/postgres/0064_held_ticket_events.sql`: schema, forced RLS, tenant+branch policies and indexes. Runtime role receives SELECT/INSERT on stream/events, SELECT/INSERT/UPDATE on heads and receipt rows; no DELETE on any table and no UPDATE/DELETE on event rows. If the migration owner role is distinct, only it owns tables/policies. Set identity/tenant/branch context transaction-locally as existing RLS helpers require; policies must compare all scope keys and reject null scope.
- `apps/web/src/heldTickets.ts`: typed API client, IndexedDB event outbox, replay by stable event UUID, revision conflict reconciliation, and no local persistence of lease tokens beyond the existing lease mechanism.
- `HostedRegisterPanel.tsx`: keep local tickets as the offline cache, merge shared list by ticket id/revision, show remote claims/expiry, and require successful online claim before importing a shared ticket into the active cart. Preserve current local pending-sale exact body/key restoration.

## Security and recovery invariants

- A read grant lists/reads ticket data only. Only cashier checkout permission at an assigned station plus an active signed lease allows claim or mutation. Never infer station permission from branch read scope.
- Every query and FK includes tenant and branch; RLS is forced and scoped to identity context. No cross-tenant fallback query, broad table grant, client-supplied actor identity, or lease token in an event.
- Claim is exclusive per ticket and transactionally serialized. A lease expiry/revocation/release makes it reclaimable immediately at the next claim attempt; an offline client cannot claim or keep an exclusive claim alive.
- Event UUID replay is idempotent. Revision mismatch cannot silently overwrite another station's edit. Event sequence and projection update commit together.
- A prepared checkout freezes the exact serialized request and idempotency key. Uncertain checkout cannot be edited, discarded, or re-keyed. Completion is recorded only after existing checkout reports a definitive result.
- Local offline work remains available on that device and syncs later. Explicitly display unsynced/locally held status; do not claim another device can see or reserve it until server acknowledgement.

## Alternatives and tradeoffs

1. **Mutable shared ticket row with revision CAS**: fewer tables and simplest reads/writes. However history/replay is weaker, claim transfer and checkout recovery fields tend to become a complicated state machine in one row, and audit reconstruction after projection corruption is unavailable. Suitable if audit/rebuild is explicitly out of scope.
2. **One event log with no head/projection**: strongest append-only purity, but listing open tickets and serializing claims require repeatedly folding each stream or maintaining a separate index. That index is effectively a head; omitting it pushes complexity and load into callers.
3. **Lease-owned claim rows separate from ticket head**: can model active claims cleanly, but acquiring a claim and checking ticket revision/status now needs a consistent lock order across two rows and expiry cleanup. Keep claim state in the locked head projection and history in events for a simpler single-ticket serialization point.
4. **Database advisory lock only**: serializes concurrent requests but not durable ownership; it releases at transaction end and cannot represent a claim between requests. Use the head row lock for each transition and a durable lease-linked claim for the between-request ownership interval.

The event-plus-head candidate is viable because current station leases already provide an authoritative durable lease and current PostgreSQL flows use explicit transaction locks. The price is event schema and projection maintenance; a rebuild operation should fold events and compare the projection during operational repair.
