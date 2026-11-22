# Cross-station held-ticket coordination

## Worklist

- [x] Ground current hosted register draft, authorization, lease and PostgreSQL migration flow.
- [x] Sketch a shared-ticket domain contract and compare structurally distinct designs.
- [x] Select and record the design with security, recovery and concurrency invariants.
- [x] Implement the database/API contract and cashier-facing sync behavior.
- [x] Verify API compilation and production web build.
- [x] Verify migration/RLS/idempotency/concurrency with the restricted PostgreSQL HTTP verifier. All 64 migrations applied twice; shared-ticket create, exact create retry, branch listing, competing claims, save, exact checkout-attempt retry, known-rejected attempt clearing, release, atomic sale completion, and exact retry from another station passed. The test confirmed one hosted sale row linked to the ticket and a completed ticket state.
- [ ] Review cashier and manager register flows in an authenticated browser, including hold/share, claim/resume, checkout recovery, and permission boundaries.
- [x] Update the implementation tracker with completed scope and remaining rollout checks.

## Grounding notes

- `apps/web/src/HostedRegisterPanel.tsx` stores active and held tickets in localStorage keyed by tenant, branch, station and staff subject. Checkout recovery persists an exact body/key locally; held tickets do not leave that browser.
- Checkout is authorized in `apps/api/src/lib.rs` by `authorize_catalog_scope(..., "sales:checkout")`, then requires a station lease ID/token, an idempotency key, and signed station proof before `sales::commit`.
- Role permissions live in `apps/api/src/authorization.rs`; owner/manager can read sales but only cashier role receives checkout permission.
- `station_actor_leases` stores hashed lease tokens, actor membership/identity, station, status and expiry; `station_actor_lease_locks` selects the active lease per station.
- PostgreSQL migrations are ordered under `apps/api/migrations/postgres`; migration 0064 adds shared held tickets. RLS is forced and the API runtime role receives explicit grants.
- Cross-station sync must not turn a draft into a sale, allow owners/managers to transact, permit two cashiers to resume/checkout the same held sale, expose one tenant's tickets to another, or lose exact retry data after an uncertain checkout.

## Design direction to evaluate

Shared ticket records should be tenant- and branch-scoped, versioned, and separate from completed sales. Only branch-authorized staff can list them. A cashier must hold a valid lease for the current station to claim or mutate a ticket. Claims must be exclusive or revision checked, expire with their cashier lease, and allow safe handoff after release/expiry. Checkout remains the existing atomic, station-signed sale flow. An unresolved idempotent checkout is immutable and cannot be discarded.

## Additional checkout invariant discovered during design

- Existing hosted sale idempotency is scoped to tenant + station + cashier + key (`hosted_sales` uniqueness and lookup in `apps/api/src/sales.rs::commit`). Replaying a pending ticket from a different station would not match that row and could create a duplicate sale.
- Shared ticket checkout therefore needs a ticket-level global unique constraint and must lock/inspect/complete the claimed ticket inside the same database transaction that inserts or replays the sale. The exact canonical request body and attempt key remain frozen on the ticket; the next active cashier may sign the retry from their station, while the held-ticket ID guarantees one sale across stations.
- This changes `sales::commit`/hosted_sales schema as well as ticket CRUD. Treat exactly-once cross-station commit as a release gate; a non-atomic "mark ticket complete after checkout" alone is insufficient.

## Selected design

Use one authoritative `branch_held_tickets` row per shared draft, with a revision, status, frozen checkout attempt, claim lease ID and optional sale ID. Claim/save/release are atomic revision-checked transitions bound to the active cashier lease. Keep local offline tickets in the existing browser store and make cloud sharing explicit; only server-acknowledged tickets appear in the branch queue. A lightweight actor/station audit trail may be written with each mutation, but it is not a second source of truth.

Shared checkout adds a nullable `held_ticket_id` to `hosted_sales` with tenant/branch-scoped uniqueness. The normal hosted sale transaction must lock and validate the shared ticket, require its exact prepared body/key/fingerprint and current active claim, return the already completed sale for an identical retry, and atomically mark the ticket completed when it inserts the sale. The normal station/cashier idempotency key remains unchanged for non-shared checkouts. This is required because the existing idempotency index is station- and cashier-scoped.

Design artifacts and comparison: `.planning/arena/held-ticket-sync/row-authority.md`, `.planning/arena/held-ticket-sync/event-log.md`, and `.planning/arena/held-ticket-sync/judgment.md`. The single-row design is selected for its smaller transactional surface; event-sourcing and automatic local-to-cloud outbox replay are deferred because local tickets already work offline and neither is needed for branch sharing.

## Implementation checkpoint (6 November 2026)

- Added branch-scoped ticket create/list/claim/save/release/discard routes, lease checks, optimistic revisions and idempotent retries for create, claim, and unchanged autosaves.
- Added a frozen checkout attempt and an explicit clear operation for a known-rejected attempt. A completed sale links to its ticket atomically, with a unique ticket link preventing a second sale.
- Added explicit local-ticket sharing, branch queue refresh/polling, claim/resume, shared draft autosave, hold/release, and checkout preparation in the hosted register. Local-only browser tickets remain available.
- `cargo check -p pos-api` and `npm run build` in `apps/web` pass. The desktop Rust check and desktop frontend build also pass.
- The restricted PostgreSQL verifier applied all 64 migrations twice and passed both HTTP scenarios. Shared-ticket scenarios passed for competing claims, known-failed checkout recovery, atomic sale completion, and exact retry from a second station. The retry returned the original sale ID with `replayed=true`; the test confirmed exactly one linked sale row and a completed ticket.
- A completed ticket must be readable for exact replay without requiring row-update access. `sales::commit` now serializes shared-ticket commits with a ticket-key advisory lock, reads completed tickets without `FOR UPDATE`, and only takes a row-update lock for a pending ticket. This preserves the existing RLS rule that completed tickets are not mutable.
- Cashier/manager browser review remains open. No full authenticated UI workflow is claimed.
