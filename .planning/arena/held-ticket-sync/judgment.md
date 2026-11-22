# Design judgment: cross-station held tickets

## Decision

Use **one authoritative branch ticket row with a revision CAS** as the base. It provides the clearest single serialization point for editable ticket state, the current lease-linked claim, and the checkout-pending state. The append-only event-log proposal has useful audit and recovery ideas, but its mutable `held_ticket_heads` projection is already the authority for claim serialization; the event layer adds another consistency surface without improving the exclusive-claim guarantee.

Neither proposal, as written, satisfies exactly-once checkout recovery. Add a ticket-level sale identity and enforce it inside the sale commit transaction before either design is considered safe.

## Scores

| Criterion | Row authority | Event log + head | Judgment |
|---|---:|---:|---|
| Role/RLS/lease security | 4/5 | 4/5 | Both preserve branch read versus cashier checkout, forced RLS, and active station lease checks. The event proposal states the boundaries more explicitly; both need careful runtime grants, proof/lease validation on every mutation, and tenant+branch scoped keys/FKs. |
| Exactly-once cross-station checkout recovery | 2/5 | 1/5 | Both freeze body/key and block edits while pending, but neither currently binds a sale to the shared ticket in the sale transaction. Event-log's claim that existing checkout idempotency prevents duplicates is contradicted by current scope. |
| Atomic exclusive claim and lease-expiry handoff | 4/5 | 4/5 | Both serialize transitions on one ticket/head row, compare revisions, and check the referenced lease's live state at claim time. The row design expresses the mechanism with fewer moving parts. Neither should rely on expiry cleanup; validate lease status/expiry in the claim transaction. |
| Simplicity/maintainability | 5/5 | 2/5 | One row is a direct fit for the modest shared draft state machine. The event design needs stream, immutable events, head projection, replay/rebuild rules, event receipt idempotency, and multiple API concepts. Choose it only if audit/rebuild is an actual requirement. |
| Offline local continuity/conflict behavior | 4/5 | 3/5 | Both retain local tickets and require an online claim before shared resume. Row design keeps the existing local storage model and handles shared edits with revision conflicts. Event outbox is useful only if local changes are actually synced as events; otherwise it duplicates the existing local draft/recovery store and complicates reconciliation. |

## Critical checkout finding

Current `sales::commit` looks up a replay by `tenant_id + station_id + cashier_membership_id + idempotency_key`, and the `hosted_sales` unique index uses those same dimensions. This is correct for same-station retries, but a pending shared ticket retried at another station does not match the original row. A second sale can be inserted even when the ticket preserves the same body and key.

Required graft for either design:

1. Add nullable `held_ticket_id` to `hosted_sales`, with a tenant/branch-scoped unique constraint so at most one sale can ever belong to a shared ticket. Keep the current station/cashier/key index for ordinary checkout compatibility.
2. Extend `sales::commit` to accept the shared ticket ID only for a ticket-backed attempt. In the same transaction that locks/validates the ticket, verifies the frozen request fingerprint and active claiming cashier lease, inserts the sale and all existing financial/inventory records, and marks the ticket completed with that sale ID.
3. On retry from any station, lock by ticket ID first. If completed, return the original sale only when the frozen fingerprint matches; otherwise reject as conflict. If pending, continue the one frozen attempt. Unique constraint is the final concurrency guard, not a substitute for the transactional state check.
4. Keep the station proof bound to the current station, lease, and exact body. The ticket ID must also be covered by the signed operation context (or included in the canonical signed body) so it cannot be swapped independently.
5. Do not release/reclaim a ticket while a checkout outcome is uncertain unless the next claimant can only perform this exact ticket-bound retry. The lease may expire, but the global ticket lock/identity prevents a second sale.

An out-of-transaction "mark completed after checkout" is unsafe: a crash between sale commit and ticket finalization recreates the cross-station duplicate window. The sale and ticket completion need one database transaction boundary.

## Concrete choices

- **Take from row authority:** one row as the shared source of truth; expected-revision CAS; lock-and-validate claim; claim expiry bounded by the authoritative lease; read-only shared listing for managers/owners; local-only offline drafts remain visible and distinct; immutable checkout-pending body/key/fingerprint; terminal retained rows for duplicate prevention and audit.
- **Take from event log:** preserve actor/station/lease provenance for transitions and an auditable event history if operational or compliance needs justify it. If retained, make events an audit trail written in the row transaction, not a competing source of truth or required projection rebuild system in the initial scope.
- **Reject from event log:** do not add a local IndexedDB event outbox for unsynced draft edits unless the product explicitly requires automatic local-to-cloud event replay. Local drafts are already available offline; only acknowledged server state should appear shared. Do not claim current station-scoped checkout idempotency gives cross-station exactly-once behavior.
- **Reject from row draft:** the fallback that finalizes the ticket after the existing checkout transaction. It is not safe for uncertain outcomes. Ticket binding and completion must move into the sale commit transaction.
- **For both:** every write rechecks tenant/branch authorization, cashier checkout permission, station assignment, active non-revoked lease and token hash; use forced RLS as defense in depth; no client-supplied actor/expiry/sale status is authoritative. Expiry handoff is lazy and transactional, so no janitor is needed for correctness.

## Release gate

Do not ship shared checkout until migration, RLS policy/grants, and the sale transaction enforce a globally unique `held_ticket_id` and tests demonstrate two distinct station/cashier retries produce one sale and the same returned sale ID. Also exercise crash/timeout recovery, frozen-body conflict rejection, simultaneous claims, and lease expiry/revocation handoff.
