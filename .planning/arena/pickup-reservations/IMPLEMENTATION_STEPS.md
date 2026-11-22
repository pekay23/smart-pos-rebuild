# Pickup reservations implementation

This file tracks the design-to-shipping sequence for safe cross-branch pickup holds. It follows the existing cross-judge decision in `../branch-stock-routing/cross-judge.md`: customer pickup and internal stock transfer are separate workflows.

## Phases

- [x] **Ground:** trace hosted availability, RLS, stock transfer transitions, sales checkout, stock adjustments and register client paths. Confirmed checkout/dispatch only check physical on-hand; transfer acceptance does not allocate source stock.
- [x] **Sketch:** compare three independently written designs in `candidate-a.md`, `candidate-b.md` and `candidate-c.md` against allocation safety, privacy, RLS, idempotency, expiry and checkout integration.
- [x] **Agree:** three candidates and an independent cross-judge selected candidate C with A's multi-line reservation schema and B's post-lock database-time/expiry lock ordering. The synthesized verdict and rejected approaches are recorded in `cross-judge.md`.
- [x] **Implement allocation foundation:** migration 0050 adds the shared quantity-only allocation ledger and branch/product locks; availability reports on-hand, pickup-held, outbound-committed, available-to-promise and shortfall without customer details. Accepted legacy transfers are backfilled without stock fabrication.
- [x] **Protect existing stock writers:** signed checkout and negative stock adjustments preserve commitments; source-authorized transfer acceptance creates a commitment and dispatch releases it atomically with stock decrement. Database triggers guard commitment insertion and stock decrease. Restricted PostgreSQL verification passed on 2 November 2026 with all 50 migrations applied twice, including direct over-allocation and committed-stock decrease rejection.
- [x] **Implement initial pickup aggregate:** migration 0051, customer consent and contact snapshots, bounded server expiry, idempotent create, immutable held event, forced RLS, station/branch queue reads and quantity-only allocation links. Restricted PostgreSQL verification covers create/replay/conflict, origin queue visibility, sibling-station isolation and ATP effects.
- [x] **Implement and verify ready/cancel aggregate transitions:** migration 0052 adds status-aware RLS, optimistic revisions, idempotency, required cancellation reasons, atomic allocation release, immutable headers and a deferred state/event integrity check. The queue can mark incoming holds ready or cancel an eligible hold. On 3 November 2026 the restricted verifier applied all 53 migrations twice and passed ready/replay, branch-manager cancellation, allocation release, ATP restoration and the tenant/operator HTTP suites. Migration 0053 also extends allocation-release policy for collected holds. Persisted expiry materialization remains open.
- [ ] **Complete pickup aggregate lifecycle:** persist expiry events/status with a safe worker and database-time lock ordering; keep contact scope and role checks intact.
- [x] **Consume holds at checkout:** bind reservation IDs/revisions into the signed sale payload and atomically consume exact held quantities with sale, stock movement and sale link; failed checkout must preserve the hold. The restricted HTTP verifier proves failed tender preserves the ready reservation/allocation and successful checkout records the sale, collected event, revision 3, and allocation release atomically.
- [x] **Build cashier request surface:** request a 60-minute customer pickup hold from cross-branch availability, validate quantity against available-to-promise, collect contact consent and show the server-confirmed expiry/reference. Web TypeScript and production build pass.
- [x] **Build scoped queue views:** branch managers can review the branch queue; station staff can open a station-scoped queue from the register header. Cards show contact, products, status and expiry. Web TypeScript and production build pass; authenticated browser acceptance remains open.
- [x] **Build pickup sale UI:** retrieve a ready hold from the pickup queue and load its items into the cashier ticket; checkout validates held quantities and submits the signed reservation reference. The UI is implemented in `apps/web/src/HostedPickupQueuePanel.tsx` and `apps/web/src/HostedRegisterPanel.tsx`; web production build passed. Mobile layout, stale/offline behavior, and authenticated browser review remain open.
- [x] **Verify restricted lifecycle:** migrations apply twice; ready/replay/cancel, cancellation allocation release, ATP restoration, failed-checkout preservation, successful atomic collection, and existing commitment/transfer guards pass under the restricted PostgreSQL verifier. The full 53-migration run passed on 3 November 2026.
- [ ] **Verify remaining gates:** independent-connection hold/hold, hold/sale, hold/transfer and hold/adjustment races; OpenAPI contract/ref parse after the final route changes; authenticated browser acceptance across cashier and manager scopes on desktop/tablet/mobile.

## Safety constraints

- A lookup never reserves stock; only a committed reservation response promises a hold.
- Use server time for expiry. Expired holds stop reducing available-to-promise even if a cleanup worker is stopped.
- A completed pickup is a normal sale at the pickup branch. No separate collect call may release the hold before checkout.
- Every path that reduces physical stock or creates an outbound commitment must preserve all other live commitments.
- Keep customer contact details out of availability rows and general inventory commitment projections.
- If existing accepted transfers exceed current source stock, surface a shortfall for explicit resolution; do not classify them as guaranteed stock.

## Current progress

- The register availability lookup now labels snapshots older than two minutes as stale, uses “last seen” quantities, warns staff to refresh before directing customers, and provides a refresh control. Offline API failures do not display cached stock as live. `apps/web` production build and TypeScript check pass. Persistent offline cache and browser interaction review remain open.
- Allocation/outbound transfer foundations are implemented in migration 0050. Pickup create/read is implemented in migration 0051, ready/cancel transitions in migration 0052, and signed checkout consumption in migration 0053 plus the sale transaction. The 53-migration restricted verifier passes. Persisted expiry materialization, independent-connection race coverage, and browser acceptance remain open.
- The cashier hold request UI, scoped queue views, and pickup-sale handoff are implemented in `apps/web/src/HostedRegisterPanel.tsx` and `apps/web/src/HostedPickupQueuePanel.tsx`; availability uses available-to-promise, the form records consent, and the queue uses branch/station API scopes. TypeScript and production build passed. Authenticated browser acceptance and mobile/stale/offline review remain open.
