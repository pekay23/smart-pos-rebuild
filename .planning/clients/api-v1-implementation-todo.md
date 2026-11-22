# API v1 implementation slice

- [x] Ground the chosen API-first boundary and current repository structure.
- [x] Sketch two materially different first API vertical slices and compare them against the platform plan.
- [x] Select a bounded design and record tradeoffs before implementation.
- [x] Implement an independently runnable Rust HTTP service and versioned contract for the first meaningful endpoint.
- [x] Build and exercise the service locally; document what the slice does not yet provide.
- [x] Update the main implementation tracker with verified scope and the next dependency.

## Grounding

The approved multi-client decision is API-first with native clients per platform. Local desktop workflows still use Tauri commands, SQLite, and pure `pos-core` sale calculations; the workspace now also contains the Rust `apps/api` service, PostgreSQL schema, OpenAPI contract, and an early browser identity client. Local SQLite accounts/passkeys remain installation scoped and cannot be treated as cloud identities. Hosted APIs use separate DTOs and verified tenant identity rather than exposing desktop commands or local database rows.

## Acceptance rubric

1. Independently runnable HTTP service within this workspace.
2. Explicit `/v1` wire contract and stable versioning, without leaking local database/domain structs.
3. No unauthenticated access to tenant/business data; health metadata contains no sensitive configuration.
4. A consumer can run and call the service from a browser or CLI without Tauri.
5. Startup/configuration and current security/product limitations are documented truthfully.

## Completed slice: stateless sale preview

The first hosted slice was `GET /healthz` and stateless `POST /v1/sale-calculations`, which delegates to checked `pos-core` functions. The service retains a loopback-only default bind, opt-in exact-origin CORS, bounded JSON bodies, line/safe-integer limits, stable JSON errors, graceful shutdown, OpenAPI 3.1 and an operating README. Sale-calculation inputs remain untrusted and this endpoint does not commit a sale. The service has since added authenticated tenant access, station licensing/leases and a hosted catalog/inventory API. Candidate and cross-judge evidence for the first slice is in `api-v1-candidates/`.

Verification: `cargo check -p pos-api`, `cargo test -p pos-api` (7 tests pass), `cargo fmt --all -- --check`, `cargo check -p smart-pos-desktop`, `cargo test -p pos-core` (2 tests), OpenAPI YAML parse, and live PowerShell calls to health, calculation, configured CORS and a missing tenant route. Calculation returned subtotal 3000, tax 435 and total 3335 minor units. This does not verify cloud auth, tenant isolation, PostgreSQL, sales persistence, production CORS/TLS, or web-client integration.

## Identity and tenant foundation decision

The three-review auth council recommends ZITADEL OIDC with PostgreSQL behind the Rust API. The decision is recorded in `.planning/security/CLOUD_IDENTITY_DECISION.md`; independent security, cashier-workflow and platform reviews are in `.planning/security/auth-council/`. Added an explicit SQLx checksummed migration runner and the initial PostgreSQL 14+ identity/organization migration for tenants, `(issuer, subject)` person identities, memberships, branches, stations, scoped role grants, and tenant audit events, with composite foreign keys preventing cross-tenant branch/station assignments. Run it with `POS_API_MIGRATION_DATABASE_URL=... cargo run -p pos-api -- migrate`; it is not invoked during API startup.

The service has since moved beyond schema groundwork. Authenticated runtime routes use the restricted PostgreSQL role, verified OIDC identity, active tenant membership, scoped permissions, transaction-local RLS context, and cross-tenant denial. Current tenant routes expose branch/station access, owner-managed station activation, device-bound cashier leases, branch/register catalog reads, authorized product creation, manager-authorized idempotent stock adjustments and station-authorized cross-branch product availability. Migrations 0008-0010 add tenant-wide product identity, branch price/tax/stock, stock movements, branch currency, auditable balance changes and narrow RLS for cross-branch stock lookup. The restricted-role PostgreSQL verifier applied all ten migrations twice and passed its HTTP/RLS integration scenario. This remains a partial hosted business API: hosted sale/checkout, tenders, drawer, product editing/archive, branch transfers and durable offline sync are not yet implemented; the browser now provides manager branch-catalog management and station catalog/availability lookup; hosted checkout remains unimplemented. Provider selection remains conditional on username-only staff provisioning/recovery, custom authentication domain and native PKCE/passkey proofs, issuer/JWKS/revocation behavior, and hosting/data-location procurement.

Verification history: the initial identity migration was first verified against disposable PostgreSQL 16. The current verifier applies all twelve migrations twice, then runs the restricted-role HTTP integration covering tenant isolation, station capacity and proof-bound leases, catalog authorization, case-insensitive duplicate SKU rejection, atomic product/opening stock, adjustment balance/movement/audit effects, exact retry replay, payload mismatch conflict, negative-stock prevention, second-cashier reads and cashier write denial, and cross-branch availability without cost leakage. Current API unit suite: 33 passed, 1 PostgreSQL-only test ignored in the ordinary suite. The Windows desktop check is tracked separately; these hosted API checks do not establish production provider configuration, deployment, or full checkout behavior.

Verifier council outcome: ZITADEL introspection with `private_key_jwt` is the first implemented path because the provider defaults to opaque access tokens and introspection checks revocation. The endpoint validates issuer, API audience, allowed client, bearer token type, expiry and not-before. Verified `(issuer, subject)` values map to a pre-provisioned internal identity through the separate runtime PostgreSQL credential; email is never used for auto-linking. Restricted-role integration covers identity mapping and tenant routes. The service/repository design candidates and cross-judge record are `.planning/clients/auth-context-candidate-a.md`, `.planning/clients/auth-context-candidate-b.md`, and `.planning/clients/auth-context-cross-judge.md`; the chosen minimal boundary and gates are in `.planning/clients/auth-context-design-todo.md`. Load-test introspection before exposing high-volume tenant routes; consider local `jsonwebtoken`/JWKS only after proving the actual access-token and ID-token profiles. Full token contract and gates: `.planning/security/OIDC_RESOURCE_SERVER_DECISION.md`.

