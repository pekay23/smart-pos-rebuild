# Hosted sales and station licensing design

## Work phases

- [x] Ground: trace API identity, branch/station scopes, device activation, cashier leases, local checkout and drawer/accounting invariants.
- [x] Sketch: compare a broad workflow-first API with a bounded, server-authoritative sale transaction; review security and retail-finance constraints.
- [x] Agree: choose the bounded sale vertical slice and document prerequisites.
- [x] Implement: bind hosted cashier lease acquisition to the activated station key and persist the activation ID.
- [x] Implement first catalog slice: tenant products, branch pricing/tax/stock, opening movement, audit, and authorized branch/register reads.
- [x] Implement manager-authorized stock adjustment with balance lock/update, append-only movement, audit and exact idempotent replay.
- [x] Implement station-authorized cross-branch product availability so a cashier can check stock and selling price at other branches without seeing cost.
- [x] Implement manager-authorized branch-local product archive/restore; retain stock/history, isolate status by branch, and audit the transaction.
- [x] Implement revision-checked branch product settings updates for selling price, tax and stock alert levels; preserve stock quantity and audit the update.
- [x] Implement tenant-wide revision-checked edits for shared product SKU, barcode, name, description and category, restricted to a tenant-scoped grant and audited across branches.
- [x] Implement branch payment-option configuration: managers define manual tender labels, kind, provider/country reference, and reference requirements; cashiers can read options at their assigned station. Disabled rows are retained and edits require the expected revision.
- [x] Add the hosted API's server-owned `sales:checkout` capability for scoped cashier grants only; owner/admin/manager roles retain POS viewing and sales-read access without checkout by default. A separately scoped cashier grant is required for management staff to sell.
- [x] Add a short-lived checkout proof challenge bound to the active lease, station activation, checkout idempotency key, and request-body hash; return the exact canonical UTF-8 message for the station Ed25519 key to sign.
- [ ] Implement remaining catalog/inventory operations: branch assignment, inter-branch transfer/request/receipt and purchase receiving. Manager branch-catalog management and station-scoped catalog/availability lookup are implemented in the web client.
- [x] Implement atomic hosted checkout for configured manual tenders, including cash only when tied to an open station drawer shift; retain server-owned pricing/tax, inventory locks, station signature proof, immutable snapshots, stock movements, audit, and exact idempotent retry.
- [x] Implement hosted station drawer open/current/X-report/movement/Z-close endpoints with a single open shift per station, signed one-use challenges, forced RLS, restricted column grants, audit, reconciliation, exact retry replay, and changed-body conflict.
- [x] Add read-only station-scoped history for the latest 50 immutable closed-shift Z snapshots and surface it to POS viewers; verify the route through the restricted-role PostgreSQL HTTP integration.
- [x] Add a permission-gated branch sales report with UTC date filters, bounded pagination, exact aggregate minor-unit strings, receipt/tender summaries and current-page CSV export; verify aggregate totals, invalid-date rejection and cashier denial through the restricted-role PostgreSQL HTTP integration.
- [x] Wire the hosted drawer and checkout APIs into the web register: owner device linking, assigned cashier lease with heartbeat/release, station catalog scan/search, editable ticket, manual tender, signed server-authoritative checkout, cash drawer open/movements/X/Z-close, local ticket recovery, and browser-print receipt.
- [ ] Verify the connected register and branch report in a live authenticated browser and printer workflow; add emailed/mobile receipt delivery, customer selection and saved sale drafts. Closed-shift history and basic sales totals/receipt listing are implemented; advanced accounting/profitability, refund and provider-settlement reports remain open.
- [x] Verify device-bound lease prerequisite: 9 migrations applied twice and restricted-runtime HTTP integration passed for forged-proof denial, one-time challenges, cashier contention/handoff, heartbeat/release, revocation and tenant RLS.
- [x] Verify hosted catalog and stock adjustment slices with restricted PostgreSQL role: product creation/read permissions, generated SKU, duplicate rejection, opening and adjustment movement/audit atomicity, exact retry replay, key-payload conflict, nonnegative stock, second-cashier read access, cashier write denial and cross-tenant denial.
- [x] Verify cross-branch availability with restricted PostgreSQL role: the ten-migration verifier applied every migration twice and exercised the route over HTTP; source-station authorization, same-tenant branch visibility, selling price/stock, cost omission, and cross-tenant denial passed.
- [ ] Verify hosted-sales acceptance under concurrency and failure injection: competing sales for last stock, rollback at each persistence boundary, refunds and reporting. The restricted PostgreSQL HTTP test verifies cashless and cash sale persistence, drawer float, cash movement, X snapshot, Z reconciliation, exact movement/close retry and changed-close conflict. Competing last-stock checkout and rollback injection remain untested.
- [x] Verify branch-local archive/restore with the restricted PostgreSQL role: all eleven migrations applied twice; HTTP integration proved cashier denial, unchanged stock, unchanged other-branch status, hidden availability while archived, and manager restore.
- [x] Verify branch product settings with the restricted PostgreSQL role: manager updates passed; stale revision conflicted; invalid settings and cashier writes were denied; stock and other-branch values stayed unchanged.
- [x] Verify tenant product identity editing with the restricted PostgreSQL role: all twelve migrations applied twice; shared identity updated across two branches while stock stayed unchanged; stale edit and duplicate SKU conflicted; cashier/branch-only grants were denied.
- [x] Verify payment-option configuration with the restricted PostgreSQL role: all thirteen migrations applied twice; owner creation and revision-checked disable passed over HTTP; station cashier reads passed; cashier writes and stale revisions were denied.
- [x] Verify checkout proof-challenge preparation with the restricted PostgreSQL role: all fourteen migrations applied twice; HTTP challenge creation passed for a live cashier lease; an invalid lease token conflicted; the canonical signature message bound actor, station, activation, idempotency key and body hash; owner-only POS view access was denied checkout proof.

## Licensing and staffing decision

A billable station is one activated register device, not one staff account. Multiple cashiers, managers, or owners may have separate credentials and use the same station in succession. The station lease allows only one active cashier identity at a time, preventing two people from finalizing overlapping work on one register. A business can have multiple licensed stations at a branch and multiple branches under one tenant. Concurrent use at two physical registers requires two stations; additional user accounts do not.

The same owner or manager identity can retain its management role while entering an explicitly authorized temporary-cashier mode. The checkout actor is the staff identity holding the station lease, and the server checks the effective cashier permission for each mutation. Owner or manager access to view POS must not itself grant checkout rights.

## Grounded current state

- The hosted Rust API has OIDC-backed identity, tenant membership, branch/station permission snapshots, station activations, activation revocation, and single-actor station leases.
- Local desktop checkout is an immediate SQLite transaction. It computes money in integer minor units, verifies the saved ticket/revision and cashier authority, checks stock, snapshots sale/line/payment/station data, decrements stock, and posts stock, drawer, audit, and accounting effects atomically.
- The hosted API now has tenant and branch catalog/inventory, payment-option configuration, checkout proof challenges, server-authoritative sale commit, and hosted drawer open/current/X/movement/Z-close endpoints. The initial hosted web register is connected to these routes for cashier lease, manual tender checkout and drawer handling; the browser retains in-progress ticket items and prints a sale summary through the browser print dialog. Cash sales link to the current station shift and feed reconciliation; manual provider-labelled tenders do not imply processor approval or capture. Authenticated browser, direct printer and customer-message delivery verification remain open.
- Station lease acquisition now includes fresh proof from the activated station Ed25519 key and persists the activation ID on the lease. Restricted-role integration covers one-time challenge use, forged-proof denial, concurrent cashier contention, handoff, heartbeat/release, revocation and tenant RLS. This satisfies the device-binding prerequisite, not hosted financial acceptance.

## Design alternatives considered

1. **Build the whole business workflow before checkout.** Create catalog, stock, customers, drawer, payments, refunds, reports, and then expose sales. This leaves a long interval without an end-to-end sale and encourages disconnected partial APIs.
2. **Commit sales directly against caller-provided product and money data.** This is small but unsafe: the API has no authoritative prices, taxes, inventory, tender configuration, or drawer to validate against.
3. **Chosen: bounded server-authoritative sale vertical slice.** Bind cashier leases to the activated device, then add the smallest tenant/branch catalog, price/tax, stock, payment-option, sale, immutable line/tender snapshot, stock movement, audit and idempotency schema needed for one complete non-cash manual sale. Device binding and this cashless sale path are implemented. Hosted hosted cash drawer shifts and cash accounting now support cash tender; broader catalog management, purchase receiving, offline sync, processor callbacks, refunds, reporting and multi-branch transfers build on these records afterward.

This keeps the first deliverable end-to-end while ensuring the server—not the web/native client—is the financial authority.

## Caller contract sketch

```text
POST /v1/tenants/{tenant}/branches/{branch}/stations/{station}/sales
Authorization: Bearer <OIDC access token>
Idempotency-Key: <random stable key for this checkout attempt>
X-Station-Lease-Id: <active lease>
X-Station-Lease-Token: <lease secret>
X-Station-Proof: <fresh signature from activated station key>

{
  "items": [{ "productId": "...", "quantity": 2 }],
  "customerId": "...",
  "tenders": [{ "paymentOptionId": "...", "amountMinor": 2400,
                 "amountReceivedMinor": 2500, "reference": "..." }]
}
```

The request must not contain authoritative unit prices, costs, tax rates, discounts, totals, cashier identity, receipt number, stock result, or processor-approved status. Server-computed values are returned with a committed sale/receipt identifier and immutable receipt snapshot. For unsupported bank/mobile-money integrations, the tender is explicitly recorded as cashier-confirmed/manual; it is not represented as processor approval.

Before the sale request, the client calls `POST /v1/tenants/{tenant}/branches/{branch}/stations/{station}/checkout-challenge` with the same `Idempotency-Key`, lease headers, and `{ "payloadHash": "<sha256-of-exact-sale-body>" }`. The response contains a challenge ID, nonce, and `messageToSign`. The device signs that exact UTF-8 message; the sale endpoint recomputes the submitted body hash and verifies and consumes the challenge inside the sale transaction.

## Transaction and type sketch

```rust
struct SaleCommand {
    items: Vec<SaleItemCommand>, // product_id, positive quantity
    customer_id: Option<Uuid>,
    tenders: Vec<TenderCommand>, // configured option, applied amount, optional received amount/reference
}

struct SaleContext {
    verified_identity_id: Uuid,
    tenant_id: Uuid,
    membership_id: Uuid,
    branch_id: Uuid,
    station_id: Uuid,
    lease_id: Uuid,
    activation_id: Uuid,
}

async fn commit_sale(
    pool: &PgPool,
    context: SaleContext,
    idempotency_key: IdempotencyKey,
    station_proof: DeviceProof,
    command: SaleCommand,
) -> Result<CommittedSale, SaleError>;
```

`commit_sale` owns the transaction. It rechecks active identity/membership and effective cashier permission, branch/station ownership, current active device activation and proof, lease actor/token/expiry/lock, open drawer when cash is tendered, configured tender options, product status, current price/tax/discount permissions and stock. It locks stock rows in deterministic product-ID order, computes money server-side in integer minor units, then inserts sale and immutable snapshots, tenders, stock movements, cash movement where applicable, audit event, and idempotency result before commit. Any failure rolls the whole operation back.

Idempotency is unique by tenant + station + cashier + key and binds a canonical request hash. A matching retry returns the prior committed receipt; reuse with changed content returns conflict. Receipt numbering is generated by the server. Split tender is supported only when each applied amount sums exactly to the sale amount; only the cash portion changes expected drawer cash. Credit/partial payment is deferred.

## Device-bound lease prerequisite

Lease acquisition accepts fresh station proof and persists the active `activation_id` on the lease. The signed canonical message binds protocol version, tenant, branch, station, activation, lease challenge/nonce, cashier identity, and request purpose. The server verifies against the active activation public key and consumes the nonce in the same transaction that creates the lease. The sale endpoint recomputes the body hash, verifies the station signature, consumes the challenge, and rechecks activation and lease under transaction locks so revocation cannot race a sale commit.

Desktop key storage and signing must use the OS secure key facility where available. Web clients need a WebCrypto non-extractable key and must handle browser profile/device loss through owner-controlled reactivation. Local-only checkout must remain usable independently while the cloud pathway is being introduced.

### Implemented API slice

- `POST .../actor-lease-challenge` now creates a 2-minute, single-use nonce for the authenticated identity and assigned cashier membership.
- `POST .../actor-lease` now requires the challenge ID and an Ed25519 signature. The server checks the activation key, validates the canonical tenant/branch/station/membership/identity/activation/challenge/nonce message, consumes the challenge, and creates the lease in the same transaction.
- `POST .../checkout-challenge` requires a live cashier lease and `sales:checkout`; it stores a 60-second, single-use challenge bound to the activated station, lease, idempotency key and 32-byte body hash, and returns the exact UTF-8 message to sign. The restricted PostgreSQL integration verifies valid lease issuance, invalid lease rejection, canonical message binding, owner-only checkout denial and consumption through the sale transaction.
- `POST .../sales` accepts product IDs, quantities, configured tender IDs, applied tender amounts and optional references. Cash tender requires an open shift at the station and records that shift on the sale. It rechecks the current cashier lease and activation, validates the signed exact body, resolves current product prices/tax and enabled tender configuration, locks stock in deterministic product order, and atomically stores the sale, line/tender snapshots, updated stock, stock movement, audit and challenge consumption. Exact retry with a new proof returns the existing sale without a second stock change; a changed body under the same key conflicts. Cash requires the hosted drawer lifecycle; a cash sale cannot commit without the station open shift.
- Migration 0007 stores `activation_id` on leases and enforces that new leases refer to an active same-tenant activation. Existing retained lease history is backfilled against the station's activation history.
- Expiry reclaim, one-actor-per-station locking, and owner revocation use the existing station advisory lock. Activation reads do not use `FOR UPDATE` because that would incorrectly invoke the owner-only update policy for ordinary cashiers.
- The restricted-runtime integration exercises two separate cashier identities, forged proof rejection without challenge consumption, lease contention and handoff, heartbeat/release, owner activation revocation, hosted cashless and cash sales, drawer open/movement/X/Z reconciliation, exact operation retries, and changed-close conflict; it passes against PostgreSQL 16. Safe/petty-cash transfers, provider capture, refunds, concurrency/rollback stress, and the hosted web checkout client remain pending.

### Hosted catalog and opening inventory slice

- Migration 0008 models a tenant-wide product identity and a branch catalog item with branch-local price, cost, tax basis points, opening stock, reorder/target quantities and active state. Branch currency defaults to GHS for the initial Ghana rollout.
- Branch management can list all associated products and create a product with its first branch catalog entry. A missing SKU is generated server-side. Product identity, branch entry, opening stock movement, and audit event commit in one transaction. Tenant SKU and barcode uniqueness use case-insensitive indexes.
- Register product listing has a station-scoped route and returns active products only. Responses intentionally omit cost; branch management and cashier data are separated by permission checks. Products/branch inventory/stock movement tables use forced RLS and least-column runtime grants.
- The restricted PostgreSQL HTTP integration passes for manager-owned create/list, cashier register reads, cashier write denial, cross-tenant hiding, duplicate SKU rejection, and a database assertion that exactly one product, branch balance, opening movement, and audit event exist after the create.
- Product editing/archive, product images, barcode uniqueness collision handling beyond database uniqueness, product assignment to more branches, stock transfers, explicit revision/idempotency headers for other mutations, authenticated browser/printer verification of the hosted register, closed-shift history and financial reports remain pending.

## Module boundaries

- `station_activation`: activation lifecycle and public-key verification.
- `station_lease`: exclusive staff ownership of one station, bound to activation.
- `sale_authorization`: one transaction-scoped revalidation of actor, tenant/branch/station, activation, lease, permission, and drawer.
- `catalog` / `inventory`: tenant/branch scoped product snapshots, price/tax policy, stock locks and movements.
- `sales`: validation/calculation orchestration and immutable committed sale/receipt snapshot.
- `idempotency`: payload binding and replay result within the same database transaction.
- `tenders` / `drawer`: configured payment options; manually recorded external tender is distinct from provider authorization; cash effects reconcile to the active shift.

Avoid routes or services that accept a client total and forward it to persistence. SQL tenant scope, composite ownership constraints and forced RLS supplement application authorization; HTTP integration tests must use the restricted runtime DB role.

## Acceptance gates

- A second cashier can take over after release/expiry; concurrent active cashier acquisition at one station is rejected. Two activated stations in the same branch can hold distinct leases concurrently.
- Activation revocation/replacement and membership/role changes invalidate the lease and any sale attempt, including requests racing revocation.
- Manager/owner preview cannot commit a sale unless an explicit, current temporary-cashier grant is active.
- Forged client prices/tax/totals/discounts and cross-tenant IDs cannot change or expose financial state.
- Two concurrent sales for the last unit cannot both commit.
- No cash drawer, inactive/disabled tender, closed product, stale activation, invalid lease/proof or insufficient stock yields zero partial financial writes.
- Replayed identical idempotency request returns one receipt and creates no duplicate stock, drawer, tender or audit effects; changed request with same key conflicts.
- Manual terminal/mobile-money tender is clearly not processor-authorized; cash received/change and cash retained reconcile correctly.
- Restricted-runtime HTTP tests verify RLS, tenant boundaries, idempotency, rollback and race conditions; migrations run twice and on an empty database.

## Council synthesis

The hosted-sale architect recommended an online-only single-tender first slice and explicit deferral of offline conflict handling, refunds, processor callbacks, and full catalog UI. The security reviewer required device-bound leases, activation rechecks and restricted-runtime HTTP coverage. The retail-finance reviewer confirmed immutable sale/tender snapshots, integer-minor-unit arithmetic, cash-vs-external tender reconciliation, and append-only refunds. Synthesis adopts the server-authoritative atomic transaction, but permits split tender only in the initial contract because the finance review showed it is necessary for coherent multi-method checkout; partial/credit payment remains out of scope. No sale implementation should begin against client-authoritative prices or before device binding is repaired.
