# Pickup reservation cross-judge

Reviewed all three candidates against the current hosted inventory and checkout constraints described in their grounding. This is a design decision. It does not prove that reservations are implemented or safe to enable.

## Scores

Scores are 1 to 5. Higher is better.

| Criterion | A | B | C |
| --- | ---: | ---: | ---: |
| Separate pickup/transfer workflows and complete checkout lifecycle | 5 | 4 | 5 |
| Races, competing stock consumers and commitment shortfalls | 4 | 4 | 5 |
| Tenant, branch, station, RLS and contact privacy | 5 | 5 | 5 |
| Expiry, replay, audit and recovery | 4 | 5 | 5 |
| Feasible API and client implementation | 4 | 4 | 4 |
| **Total** | **22** | **22** | **24** |

## Decision

Use C as the base, with the grafts below. All candidates converge on a shared table containing only inventory allocation data, separate customer reservation records, one lock protocol for competing stock writers, source-authorized transfer acceptance and collection inside signed checkout. That agreement is stronger than their differences in naming or initial item count.

C gives the clearest treatment of stock shortages, exact station checks and independent-connection race tests. It also makes the release boundary explicit: a hold feature cannot be enabled while ordinary checkout, dispatch or adjustments can consume promised units.

A has the most complete caller and multi-line schema sketch. Its initial safety-stock setting and broader API add work, and its expiry-worker lock order is less precise than B's. B is the most explicit about database time after lock waits and expiry-worker ordering. Its route sketch omits a station action route despite granting pickup cashiers ready/cancel behavior, so an implementation following that sketch literally would strand station-only grants behind a branch-scoped action route.

## Grafts

- Take A's reservation header plus immutable line table and `items[]` create contract. The availability card may begin with one selected product, but a reservation can contain multiple products without replacing its identity or lifecycle later. Full collection consumes all reserved lines in one sale; partial collection remains a separate explicit workflow.
- Take B's rule to capture `clock_timestamp()` after acquiring inventory locks, then use that captured value for all deadline and capacity decisions in the operation. PostgreSQL transaction-start `now()` can be old after a lock wait.
- Take B's expiry-worker order. Discover due IDs without holding reservation row locks, acquire sorted inventory locks, then lock and recheck each reservation. Expiry is effective by deadline even if the worker is stopped. The worker only materializes status and one audit event.
- Take A's checked numeric aggregation. Sum allocation quantities as PostgreSQL `NUMERIC`, then reject overflow before conversion to the bounded API integer representation.
- Keep A's rule that a pickup reference is a lookup aid. Authentication and branch/station authorization still control contact visibility and checkout.

## Required refinements

1. Put the shared lock and allocation calculation in one private inventory module. Every sale, pickup mutation, transfer allocation/dispatch, negative adjustment and future sync stock write must call it. Acquire sorted inventory keys before product and reservation/transfer row locks. Keep source and allocation consistency constraints in the database.
2. Implement C's database guards with an explicit privilege design. They must use the same lock order and quantity-only reads. A caller-controlled GUC such as an unchecked "ignore allocation" flag cannot bypass the guard. Own-allocation consumption must be tied to a validated same-branch sale or source transfer operation.
3. Make transfer accept/reject source-authorized in the API, client, RLS and OpenAPI together. Acceptance allocates real source capacity. Add source-manager cancellation before dispatch. For a one-shot partial dispatch, record a required reason and release the unshipped remainder atomically.
4. Preserve existing accepted transfers as legacy commitments. Report any shortfall and block new promises until a source manager resolves it. Do not reduce historical acceptance quantities or manufacture stock during migration.
5. Add an atomic shortage-resolution operation. A manager can record a real physical count and cancel/reduce affected commitments with reasons under the same locks. Separate cancel and adjustment requests allow a new hold to race between them. Ordinary adjustments that breach live commitments return a conflict.
6. Expose both station and branch action routes. Station cashiers can prepare incoming pickup holds and cancel their own outgoing holds. Management can cancel participating-branch holds with a reason. General availability reads only allocation totals. Contact records never receive the cross-branch availability policy.
7. Complete pickup only through the existing cashier-only signed checkout transaction. Bind reservation IDs and revisions into the signed payload, validate full reserved quantities, consume allocations, decrement physical stock once, link the sale and append collection/audit records atomically. Failed checkout preserves the hold.
8. Keep safety stock distinct from reorder alerts. Its setting and override policy are an existing separate product requirement. If shipped with reservations, give it its own field and enforce it in every allocation and consumption check. Do not subtract `reorder_point` or quietly exempt one stock writer.

## Rejections

- A standalone `collect` status mutation creates a release-before-sale race or a second stock decrement. Collection belongs in checkout.
- A reservation-only lock, display-only ATP calculation or new hold table without changing existing stock consumers cannot enforce a stock promise.
- A mutable reserved-count column cannot release expired stock reliably when its worker stops. Use unreleased allocations with server deadlines for capacity arithmetic.
- Broad availability access to customer-bearing reservation rows weakens privacy. Read quantity allocations separately.
- Automatic extension on retry, client-defined expiry and silent cancellation during adjustment change the promise without an explicit authorized action.

## Implementation and completion gates

Build the allocation schema/module and migrate every stock consumer first. Then add pickup persistence/actions, signed checkout consumption, expiry materialization and the cashier/manager client workflow. Keep hold creation disabled until those parts are integrated.

Verification must use independent PostgreSQL connections for hold/hold, hold/sale, hold/transfer and hold/adjustment races. The existing one-connection identity suite cannot prove concurrency safety. Assert physical stock, active allocations, sale links, movements, events and audit totals. Also prove deadline behavior without a worker, exact/changed replay, failed-checkout rollback, legacy-shortfall handling, wrong tenant/branch/station denial, contact omission from availability, and cashier-only collection. Run migrations twice and complete authenticated browser review for origin cashier, pickup cashier and manager on desktop/tablet/mobile. Build or unit-test success alone does not complete this feature.
