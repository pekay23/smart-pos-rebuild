# Candidate B — independently deployed operator control plane

## Problem

Tenant payment requests are correctly branch-scoped, but their `submitted` status cannot be triaged by a developer. Adding a privileged route to the tenant API or broadening `pos_api_runtime` would make one application credential capable of crossing every tenant boundary. The operator product also needs its own identity, roles, audit and deployment lifecycle, so the inbox should begin as a separately deployed control plane.

## Usage (caller's view)

An operator opens the separate console, signs in through the operator-only identity-provider application, and sees only the queue allowed by their provisioned role:

```ts
const queue = await operatorApi.listPaymentRequests({ cursor })
await operatorApi.updatePaymentRequest(id, {
  expectedRevision: request.revision,
  status: 'under_review',
  customerVisibleUpdate: 'We are checking support for this terminal model.',
  internalNote: 'Need protocol documentation from the acquirer.',
})
```

The tenant manager continues to use the branch API:

```ts
const requests = await tenantApi.listPaymentIntegrationRequests({ tenantId, branchId })
```

That response contains customer-visible status and progress only. It never includes operator identity, internal notes, or another branch's requests.

## Shape

### Applications and modules

- `apps/operator-api` is a separate Rust service. It owns an operator OIDC configuration, route tree, database pool, CORS origin and health/readiness endpoints. Tenant API routes are not mounted in this process.
- `crates/pos-auth` contains the reusable token-introspection primitive currently in `apps/api/src/auth.rs`. The operator and tenant processes construct distinct authenticators with different expected audiences and disjoint allowed client IDs. This crate returns a verified issuer/subject only; it does not grant application roles.
- `apps/operator-web` is a separate console and OIDC client. It uses authorization code + PKCE, does not persist access tokens in localStorage, and calls only `apps/operator-api`.
- `apps/api/src/payment_requests.rs` remains the tenant-owned request adapter. The tenant API may select the explicit customer-progress columns, but cannot update them.
- `apps/operator-api/src/identity.rs` maps a verified `(issuer, subject)` to an active `operator_accounts` row and server-owned role. It never consults `person_identities`, tenant memberships or email.
- `apps/operator-api/src/payment_requests.rs` owns the queue query and revision-checked status transition; queue details, customer progress, private notes and audit events cross no other module boundary.

### Caller-facing domain types

```rust
enum OperatorRole { Support, Billing, ReleaseManager, SecurityAdmin }
struct OperatorIdentity { id: Uuid, role: OperatorRole }
enum PaymentRequestStatus { Submitted, UnderReview, Planned, InProgress, Released, NotPlanned }
struct PaymentRequestFilter { status: Option<PaymentRequestStatus>, country: Option<CountryCode>, cursor: Option<QueueCursor>, limit: PageSize }
struct PaymentRequestUpdate { expected_revision: i32, status: PaymentRequestStatus, customer_visible_update: Option<String>, internal_note: Option<String> }
struct PaymentRequestQueueItem { id: Uuid, tenant_id: Uuid, branch_id: Uuid, country_code: CountryCode, provider_name: String, terminal_model: Option<String>, capability: String, impact: BusinessImpact, details: Option<String>, status: PaymentRequestStatus, customer_visible_update: Option<String>, revision: i32, created_at: DateTime<Utc>, updated_at: DateTime<Utc> }
struct PaymentRequestUpdateResult { request: PaymentRequestQueueItem, event_id: Uuid }
```

`CountryCode`, `PageSize`, and `QueueCursor` validate at construction. The HTTP adapter converts JSON and SQL rows to these internal values; database and transport types stay private.

### API and state transitions

The operator service exposes:

- `GET /operator/v1/payment-integration-requests?status=&country=&cursor=&limit=` for the support role, bounded to at most 100 rows per page.
- `PATCH /operator/v1/payment-integration-requests/{id}` with an expected revision, status, customer-visible update and optional private note.
- `GET /operator/v1/me` for the provisioned operator ID and server-owned role, with no tenant identity data.

The status update takes one database transaction: set transaction-local `pos.operator_id`; confirm the active operator row and support role; update the request only when revision equals the supplied revision; increment revision; insert a public progress event when progress text is present; insert a separate private-note event when requested; append an immutable operator audit event; commit. Stale revisions return `409`. The operation rejects invalid status transitions, oversized values and common secret/payment-data markers. It does not change provider configuration, create a payment attempt or contact a provider.

### Database and authorization

Migration `0027_operator_accounts_and_request_workflow.sql` adds:

- `operator_accounts(id, issuer, subject, role, status, created_at, deactivated_at)`, unique on issuer+subject. Role is an enum/check, not a free-form grant. No email column is used for identity.
- `operator_payment_request_events` with immutable event IDs, request ID, operator ID, kind, status transition or customer-visible text, and created time. Private note text is a distinct event kind/column and is never returned by the tenant API.
- revision and customer-visible progress on hosted payment requests.
- operator RLS policies gated by transaction-local operator identity plus an active account/role lookup.

The operator pool uses a distinct `pos_operator_runtime` login with `NOSUPERUSER NOBYPASSRLS`. It receives only necessary columns for operator lookup, queue reads, request status/progress updates and audit/event inserts; it cannot delete requests/events or update immutable event rows. `pos_api_runtime` receives only tenant-visible status/progress columns, with no status update grant. Tenant API policies keep branch visibility independent of operator visibility. Even an operator token submitted to the tenant API is rejected because tenant authenticator client IDs and audience do not match.

### Authentication, provisioning and operations

- The operator identity-provider application has a dedicated audience and client ID allowlist; the tenant API's app client IDs are not included.
- Configure the identity provider to require passkeys or MFA for every operator sign-in. ZITADEL's current introspection response does not include `amr`, `acr` or `auth_time`; this service does not invent an MFA claim. The OIDC application policy is an explicit deployment invariant and must be verified in the IdP and an operator login smoke test.
- The operator authenticator rejects tokens with more than a five-minute remaining lifetime. The web app holds the token in memory only; a BFF/secure-cookie migration can follow before deployment if the browser auth library cannot meet the token-handling requirement.
- `pos-operator-admin provision` and `deactivate` run as an explicit one-shot CLI using a separate offline provisioning database URL. Provisioning records issuer, subject and role, inserts an operator audit event and never accepts email as the lookup key. The API has no first-user or self-promotion route. A deployment runbook provisions the initial security administrator out of band and removes access on offboarding.
- Each operator session is short-lived and each status change is auditable. The database is never reachable with the operator pool from the browser. Secrets are delivered at runtime and are not put in the web bundle.

### Verification surface

- Unit tests for issuer/subject mapping, role checks, status-transition rules, bounded filters and public/private field separation.
- Auth tests prove tenant audience/client tokens cannot enter operator routes, operator tokens cannot enter tenant routes, inactive/unprovisioned operators are denied, wrong issuer/subject is denied, expired/overlong tokens fail, and role-specific actions are rejected.
- Restricted PostgreSQL integration applies migrations twice; proves forced RLS and least grants; operator role can list only allowed queue fields and update only allowed status fields; tenant runtime cannot see the global queue or mutate status; tenant managers see only their branch's customer progress; cashier/operator cross-tenant boundaries fail; concurrent stale status edits return conflict; and request, event, audit and progress commit atomically.
- Web production build plus role-based browser review of login, queue filters, status update, private-note visibility and tenant-visible progress.
- Deployment test uses distinct tenant/operator database users and OIDC applications. Both must be live-tested before turning on operator access.

## Synthesis decision

Not selected yet. This candidate selects a process boundary and proposes sharing only the introspection primitive. It must be compared with an in-process isolated router/pool candidate before implementation.

## Tradeoffs accepted

- We accept one extra service, deployment and database credential in exchange for a process and credential boundary around global support access.
- We accept an auth-module extraction in exchange for one implementation of token validation without sharing either audience, client IDs, route state or authorization decisions.
- We accept IdP-policy reliance for MFA/passkeys until the provider exposes a verifiable assurance claim in the API token introspection contract; setup and login verification become a hard release gate.
- We accept an explicit CLI-based operator bootstrap in exchange for having no hidden first-user endpoint or master support account.

## Alternatives considered

- A single tenant API with `/v1/operator/*` routes and an operator pool reduces deployment overhead, but it colocates tenant and global identity/error paths in one process and is the runner-up only if operating a second service is not feasible.
- A database login protected only by static operator emails is rejected: emails are mutable, may be shared, and turn an external identity attribute into global authorization.
- Adding `BYPASSRLS` to an operator database user is rejected: application bugs could then read or alter every row without table policies.

## Open questions and risks

- Can production deployment provide a second database secret, application client and HTTPS origin for the operator service from day one?
- Does the chosen identity-provider operator application enforce passkeys/MFA for all federated and local accounts, and can CI or a deployment check verify that setting?
- Which roles may add private notes versus change only public status? The initial design gives the support role both; security admin can deactivate accounts but cannot read queue notes by default.
- Should a request that affects multiple tenants become a linked issue in the later Kanban, or remain independent until duplicate linking is built?

## Next implementation step

Compare this package against the independent-router/shared-process candidate, then synthesize the chosen auth, RLS and status-event contracts before writing migrations or routes.
