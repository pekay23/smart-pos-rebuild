# Hosted drawer and register workflow

## State and phases

1. **Ground** — local drawer rules and hosted station proof, authorization, sales, migrations, and deployment grants inspected.
2. **Sketch** — documented below before implementation.
3. **Implement** — generalized signed financial challenges, server-owned drawer shift lifecycle, cash sale linkage, RLS/grants, API routes, and PostgreSQL integration verification.
4. **Client integration** — add shift open/count/movement/X/Z to the cashier web client and expose read-only drawer/reports to permitted manager roles.
5. **Audit** — verify concurrency, replay, tenant isolation, cash reconciliation and remaining unsupported cases.

## Grounded local rules

The desktop drawer has one open shift per store and station. A shift captures its opening cashier and opening float. Expected cash is `opening + cash sales + paid in - paid out - drops`. X is a non-closing snapshot; Z stores an immutable final snapshot with counted cash and variance. The hosted checkout already locks a station, derives prices/tax from server data, stores payment snapshots and requires a one-use device signature challenge. Hosted manual tenders are cashier-confirmed; provider authorization is not inferred.

## Chosen hosted model

The station is the licensed/physical checkout endpoint; multiple people can use it sequentially with their own cashier identity and lease. Only one live cashier lease and one open drawer shift may exist at a station. The shift is the physical drawer period and records who opened/closed it; each sale continues to store the actual cashier identity independently. A cashier rotation does not create another simultaneous cash balance for the same till. When a station closes its drawer, a later shift may open.

All financial mutations use one station-signed challenge contract. The challenge binds an allowlisted operation, canonical target path, cashier lease, active device activation, idempotency key and SHA-256 of the exact request body. Server transactions serialize on the station lock. Open/movement/close, cash sale and challenge consumption commit atomically. Reports are recomputed while open and a Z snapshot is immutable once closed.

## API sketch

- `POST .../checkout-challenge` accepts `operation`, optional `resourceId`, and exact `payloadHash`. Allowed targets: sale commit, shift open, movement create, shift close.
- `GET .../drawer-shift` returns the current station shift for permitted staff.
- `POST .../drawer-shifts` opens with `openingCashMinor`.
- `POST .../drawer-shifts/{shift_id}/movements` records paid-in, paid-out, or cash-drop with a required note.
- `GET .../drawer-shifts/{shift_id}/x-report` returns a live snapshot without closing.
- `POST .../drawer-shifts/{shift_id}/close` saves counted cash and an immutable Z report.
- A cash tender in `POST .../sales` requires the station's current open shift and snapshots its id on the sale. Non-cash manual tenders remain valid without a drawer.

## Database invariants

- At most one open shift per tenant/station.
- Cash sales reference an open shift from the same tenant, branch, station; sale and tender insert are in one transaction.
- A closed shift cannot receive new cash sales or movements.
- A Z report is unique per shift and immutable; retries with the same operation key/body return the saved result and a different body conflicts.
- Tenant/branch/station RLS and per-column runtime grants apply to shifts, movements, reports and sale linkage.
- Expected cash uses checked BIGINT arithmetic; no client-supplied totals are trusted.

## Review notes / limits

The hosted drawer first implements a station till and manual cash reconciliation. Bank integrations, safe/petty-cash transfer ledger, multi-currency drawer accounting, offline conflict resolution, web UI, receipt printing, and close-history UI remain follow-on work. No merchant terminal is represented as automatically approved by entering its reference.
