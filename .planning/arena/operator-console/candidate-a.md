# Candidate A: Separate operator service with a narrow request inbox

## Problem

Give provisioned Powered Solutions support operators a separate, auditable way to review hosted payment-integration requests, update workflow status, and send tenant-visible progress. Keep tenant credentials and tenant routes incapable of enumerating or changing requests outside their existing branch scope. Make this first slice a foundation for a later operator console without adding a universal administrator path.

## Usage (caller view)

The operator web client obtains a short-lived access token from the operator identity provider client and calls only the operator API. A support operator opens a paginated inbox, filters by status/country/provider, and opens a request containing the business and integration details already submitted by a tenant. They choose a permitted next status, enter an optional customer-visible update, and may separately add a private support note. The UI sends the request's current revision. A successful response returns the new revision and timestamp; a stale revision returns `409 Conflict` and requires the operator to reload before retrying.

The tenant manager continues to use the tenant web client and tenant API. Their request list shows the current status and only the explicitly published progress update. They cannot see internal notes, operator identity details, or the operator queue. Their existing branch membership continues to determine which submitted requests they may read.

The operator service and operator web client have separate OIDC client registrations, callback URLs, session cookies/storage, API origin, configuration, and deployment identity. No tenant token or tenant session is accepted by an operator endpoint; no operator token is accepted by a tenant endpoint.

## Shape

### Data/type sketch

```rust
enum OperatorRole {
    Support,
    Billing,
    Release,
    Security,
}

enum OperatorAccountStatus { Active, Suspended, Deactivated }

struct OperatorPrincipal {
    operator_id: Uuid,
    issuer: Url,
    subject: String,
    role: OperatorRole,
}

enum PaymentRequestStatus {
    Submitted,
    UnderReview,
    WaitingForTenant,
    InProgress,
    Resolved,
    Declined,
}

struct OperatorRequestSummary {
    request_id: Uuid,
    tenant_id: Uuid,
    branch_id: Uuid,
    country: String,
    provider: String,
    terminal_model: Option<String>,
    capability: String,
    business_impact: String,
    submitted_at: DateTime<Utc>,
    status: PaymentRequestStatus,
    tenant_visible_update: Option<String>,
    revision: i64,
}

struct UpdatePaymentRequest {
    expected_revision: i64,
    status: PaymentRequestStatus,
    tenant_visible_update: Option<BoundedText>,
    private_note: Option<BoundedText>,
}
```

Operator account identity is the exact verified `(issuer, subject)` pair. Store a stable generated `operator_id`, explicit role, active status, provisioning metadata, and timestamps in `operator_accounts`. Do not put email in the identity key or use tenant `person_identities`, tenant memberships, or tenant roles to authorize operators. Email may be displayed as non-authoritative IdP profile metadata only if a later product need justifies it.

Add an append-only `operator_request_events` table. Each event records event ID, request ID, operator ID, event kind, prior and resulting status, tenant-visible update (if any), private note (if any), expected/resulting revision, and timestamp. A private note is a separate column/value from the tenant-visible update, with separate API response types and explicit tenant query projections. Add a separate append-only `operator_audit_events` stream for authentication/provisioning/role/deactivation and sensitive read/update actions. Never serialize private note fields in tenant DTOs.

The existing request row holds the current status, current tenant-visible update, revision, and update timestamp for efficient tenant reads. The event log is the immutable history. Updating the row and inserting the corresponding event/audit records happen atomically.

### Module/service map and signatures

Use a distinct `apps/operator-api` service (or an independently deployable operator API binary) and `apps/operator-web` client. Keep shared protocol primitives small and non-authorizing; do not share `AppState`, tenant `Authenticator` configuration, tenant `resolve_identity`, or tenant database pools.

```rust
// operator-api/src/config.rs
struct OperatorConfig { /* operator OIDC, DB, session and bind settings */ }
impl OperatorConfig { fn from_env() -> Result<Self, ConfigError>; }

// operator-api/src/auth.rs
struct OperatorAuthenticator { /* operator-specific issuer/audience/client allowlists */ }
async fn authenticate_operator(
    headers: &HeaderMap,
    auth: &OperatorAuthenticator,
    accounts: &OperatorAccountStore,
) -> Result<OperatorPrincipal, AuthError>;

// operator-api/src/payment_requests.rs
async fn list_requests(
    principal: OperatorPrincipal,
    query: RequestQueueQuery,
    repo: &OperatorRequestRepository,
) -> Result<Page<OperatorRequestSummary>, ApiError>;
async fn update_request(
    principal: OperatorPrincipal,
    request_id: Uuid,
    input: UpdatePaymentRequest,
    repo: &OperatorRequestRepository,
) -> Result<OperatorRequestSummary, ApiError>;

// operator-api/src/provisioning.rs; called by controlled CLI, not public HTTP
async fn provision_account(
    actor: ProvisioningActor,
    issuer: Url,
    subject: String,
    role: OperatorRole,
) -> Result<OperatorAccount, ProvisioningError>;
async fn set_account_status(
    actor: ProvisioningActor,
    operator_id: Uuid,
    status: OperatorAccountStatus,
) -> Result<(), ProvisioningError>;
```

An operator request transaction must set transaction-local context only after account lookup and role authorization, for example `SET LOCAL pos.operator_id = ...` and `SET LOCAL pos.operator_role = ...`. The repository layer starts a transaction, sets context, performs a revision-checked update (`WHERE revision = expected_revision`), inserts immutable events, and commits. It returns conflict when no row matched. Never use a caller-supplied role or tenant ID as authority.

### Auth and provisioning

- Register a dedicated operator OIDC client and API audience. Operator accepted audiences and client IDs are separate configuration sets from `POS_API_OIDC_*`; validate both lists, issuer, subject, active state, expiry, not-before, token type, audience, and client ID. Reject overlap between operator and tenant client IDs and audiences at startup so accidental configuration collisions fail closed. An issuer may be shared if the IdP supports distinct clients/audiences and policies.
- Authenticate the bearer token with the operator authenticator, then resolve the verified `(issuer, subject)` only against `operator_accounts`. Require an exact active account and read role from that server-owned row. Unknown, suspended, or deactivated accounts receive `403` and produce a security audit event without leaking whether the subject exists.
- Require MFA/passkey through the operator IdP/application policy. The current ZITADEL introspection response does not carry `amr`, `acr`, or `auth_time`; do not represent the current API as proving MFA. Before production enablement, document and verify an IdP policy that enforces the required factor for this client. If policy cannot be verified, fail the deployment readiness check or keep operator access disabled. If stronger API-verifiable assurance is later needed, add a supported signed assurance signal with issuer-specific validation rather than trusting an unverified header or email.
- Keep access-token lifetime short at the IdP and operator app; do not persist bearer tokens in local storage. Use a dedicated secure, HttpOnly, SameSite session cookie if the operator client uses a backend-for-frontend. Enforce no-store on authenticated responses, CSRF protection for cookie-authenticated mutations, and session revocation on deactivation.
- No bootstrap operator is created by a migration, environment email, shared PIN, or unauthenticated endpoint. Provision the first account with an offline, audited CLI run in a controlled environment, authenticated using a separate provisioning credential unavailable to the API runtime. Require the operator's exact IdP issuer and subject. The tool must display the target issuer/subject/role for operator confirmation, be idempotent, reject duplicate/conflicting mappings, and write an audit event. Subsequent provisioning and role changes use the same controlled tool or a separately designed privileged workflow with two-person review; the inbox API cannot provision accounts or elevate the caller.
- Deactivation sets status to `Deactivated`, revokes active sessions, and records actor/reason/time. Suspension is reversible and distinct from deletion. Preserve identity and audit history; never delete an account row that appears in events. Provide a documented break-glass recovery procedure requiring offline database administration and an out-of-band audit record, not a hidden API superuser.

### Database roles, RLS, and least grants

Create a dedicated PostgreSQL login `pos_operator_runtime` with `NOBYPASSRLS`; it must not inherit `pos_api_runtime`, schema-owner, migration, or superuser privileges. The operator service gets a separate `POS_OPERATOR_DATABASE_URL` secret and connection pool. The role must not read/write tenant sales, payments, credentials, customer profiles, inventory, or unrelated tenant tables.

Enable and force RLS on `operator_accounts`, hosted integration requests as needed for the operator queue, `operator_request_events`, and `operator_audit_events`. Policies must require the transaction-local operator ID to resolve to an active `operator_accounts` row with the required role. A missing/empty context returns no rows and permits no writes. The support role receives access to the payment-request queue only. Future billing/release/security roles should get separate explicit policies for their own tables and commands; no catch-all `operator = true` policy.

Grant only the columns necessary to the operator runtime, preferably through narrow views and security-invoker functions. For requests: SELECT only request/business fields needed for triage; UPDATE only status, tenant-visible update, revision, and update timestamp, constrained to the expected revision and permitted transitions; never update the tenant-submitted payload, tenant/branch ownership, or payment records. Grant INSERT on append-only event tables; grant no UPDATE/DELETE/TRUNCATE on events or audit. The runtime must not own these tables or have `BYPASSRLS`. Migration/owner credentials remain outside the service.

The operator and tenant runtime roles continue to use separate RLS policies and grants. Existing tenant manager branch policies remain the only tenant read path. Tenant GET should project status, tenant-visible update, revision, and timestamp only; private notes/events are not granted to `pos_api_runtime`.

### Status, revision, and audit flow

1. Authenticate with operator audience/client allowlists; resolve provisioned operator account; authorize `Support` role for this first feature.
2. Validate request body size, status enum, transition table, update/note bounds, and reject credential/payment-card content with existing request validation conventions.
3. Begin DB transaction; set local operator context; load allowed current state under forced RLS.
4. Update by request ID and expected revision. Apply an explicit transition matrix (for example `submitted → under_review → waiting_for_tenant/in_progress → resolved`, with `declined` available only from permitted triage states); prevent silent reopening of terminal statuses unless a future explicit transition is designed.
5. Increment revision and update current tenant-visible fields only when supplied; insert immutable request event and audit event containing actor, action, request ID, revision, status delta, timestamp, and reason. Do not place private note text into tenant-facing columns or audit summaries that are exposed to tenants.
6. Commit. If the expected revision is stale, return `409` with the current revision but no private notes. If the request does not exist or RLS hides it, return a non-enumerating not-found response.

Use idempotency keys for mutation retries, scoped to operator and request, so a network retry does not append duplicate updates. Audit access to request details as well as status changes, since request metadata may contain sensitive business information. Apply retention/visibility policy separately to operational notes and immutable audit metadata.

### API and UI surface

Operator API, under a separate origin and route tree:

- `GET /operator/v1/payment-integration-requests?status=&country=&provider=&cursor=` — support-role queue, stable cursor pagination, bounded page size.
- `GET /operator/v1/payment-integration-requests/{id}` — one request with explicit response fields and event history split into tenant-visible updates and private notes, subject to support policy.
- `PATCH /operator/v1/payment-integration-requests/{id}` — expected revision plus new status and optional tenant-visible update/private note. Return `409` on stale revision.
- No public endpoint for account provisioning, arbitrary SQL, tenant switching, or access to tenant financial data.

Tenant API enhancement:

- Existing tenant list/detail routes continue to use branch-scoped identity and return only current status, tenant-visible update, revision, and timestamps. They do not expose operator queue fields, internal notes, audit events, or operator account data.

Operator UI (`apps/operator-web`) is a separate application with its own OIDC client and a clear Powered Solutions operator identity. First screen: request inbox with status/country/provider filters, pagination, and request age. Detail screen: submitted request, status history, separate “Update visible to tenant” and “Private operator note” controls, revision-conflict refresh, and an audit-friendly confirmation before status transitions. Display operator role and session expiry. No tenant-client navigation link should carry or exchange credentials into this app.

Keep an extension seam for later console areas as separate modules/routes (`billing`, `release`, `security`, `support`) whose permissions are explicit capability checks, not UI hiding alone. The immediate slice implements only `support:payment_request:read` and `support:payment_request:update` capabilities.

### Tests and operations

Security and behavior tests should cover:

- Tenant token rejected by operator API; operator token rejected by tenant API; startup rejects overlapping audience/client allowlists; invalid issuer, audience, client, expired token, inactive token, and wrong token type rejected.
- A valid token whose `(issuer, subject)` is absent, suspended, or deactivated in `operator_accounts` is denied; email equality, tenant membership, or a tenant owner/manager role never grants operator access.
- Support role can only read/update payment requests; future role values cannot gain access by default. Tenant role cannot list other branches or see private notes; operator can read only granted request fields and no unrelated tenant data.
- RLS with no context returns/changes nothing; wrong operator ID/role is denied; `pos_operator_runtime` is `NOBYPASSRLS`, not table owner, and has no event update/delete or unrelated table grants.
- Status transition matrix, revision conflict (`409`), atomic current-row/event/audit behavior, idempotent retry, tenant-visible update propagation, private-note non-leakage, and non-enumerating missing-row behavior.
- Provisioning CLI creates only the specified issuer/subject/role, is idempotent, refuses ambiguous changes, audits actor/reason, and deactivation immediately invalidates subsequent requests. No default/bootstrap admin is created by migration.
- MFA/passkey readiness is documented as an IdP policy assertion and deployment gate; tests must not fabricate unsupported introspection claims.

Operationally, alert on repeated authentication failures, denied unknown subjects, unusual request exports/volume, failed audit inserts, and stale revision conflicts. Keep audit logs protected from the API runtime's update/delete privileges and export them to a separately controlled sink. Use separate secrets and rotation runbooks for tenant and operator DB users and OIDC clients. Rate-limit list/detail/update routes; avoid logging bearer tokens, private notes, or full request payloads. Include health/readiness checks that verify database grants/RLS role properties, disjoint auth configuration, and required MFA policy deployment state without exposing secrets.

## Rationale

A separate service, web client, auth configuration, and database login creates a clear boundary around operator authority. This matches the product requirement for a separate operator portal and reduces the chance that tenant route middleware, client session state, or runtime database credentials accidentally acquire operator powers. A shared database is workable because the operator task needs a narrow cross-tenant request queue, but forced RLS, transaction-local operator context, a non-bypass role, and column-level grants limit that capability to a specific support workflow.

The existing request data and tenant request UI already provide the first half of the workflow. Add an explicit operator projection and event trail instead of giving operators broad tenant impersonation. Keeping current status on the request row makes tenant progress reads inexpensive; append-only events retain accountability. Separate DTOs and policies make the privacy boundary reviewable.

## Tradeoffs accepted

- Separate deployment and OIDC client setup add configuration, release, and monitoring work. This cost is accepted for a high-value trust boundary; shared libraries may hold protocol types/validation but not authentication state or authorization decisions.
- The operator service can see submitted request business details across tenants, because that is necessary for triage. The service cannot see unrelated tenant records and its access is auditable.
- MFA/passkey enforcement initially rests on a verified IdP/application policy because current introspection lacks assurance claims. If operators need an API-verifiable factor guarantee, implementation must add a supported assurance signal before claiming it.
- The first release provisions accounts out of band and supports only support-role request handling. Self-service provisioning and additional operator roles wait for their own permission and audit design.

## Concrete alternative rejected

Reject adding operator routes to the existing tenant API process and reusing its `AppState`, `Authenticator`, or `pos_api_runtime` pool, even if routes are nested under `/operator`. A second route tree inside the same process is smaller to deploy, but a middleware/state/pool wiring mistake could let tenant configuration or credentials cross the boundary, and shared session assumptions complicate the separate operator-client requirement. The alternative remains a possible future simplification only if process-level isolation proves operationally prohibitive and equivalent boundaries can be mechanically enforced; it is not the selected first implementation.

## Risks and open questions

- Confirm whether the deployed IdP supports a dedicated operator client with enforced MFA/passkey policy and a reliably short access-token lifetime. The current ZITADEL introspection data cannot attest to MFA/passkey usage.
- Decide whether support operators need to view tenant/branch display names. If needed, add a minimal operator-only projection with explicit fields and audit it; do not grant general tenant-profile access.
- Define exact transition rules, update/note length bounds, retention periods, and whether private notes require separate support-lead visibility before implementation.
- Determine whether request submissions may contain identifying details. Existing validation rejects common payment credentials, but operators should be trained not to request credentials or cardholder data in free text.
- Plan operational ownership for separate operator API/web deployments, secrets, backups, availability, and incident response.
- A separate process is not a complete security boundary if its database policies or deployment secrets are misconfigured; migration checks and runtime readiness checks must verify role properties and grants.

## Next implementation step

Implement a vertical support-only slice: migration for provisioned operator accounts, append-only request/audit events, current status/revision fields and forced-RLS policy; create a separate operator API service with its own authenticator and `pos_operator_runtime` pool; add list/detail/revision-checked update endpoints; then add the separate operator web inbox and tenant-visible progress projection. Start with migration/grant/RLS and auth-isolation tests, and do not enable production operator access until the IdP MFA/passkey policy and provisioning runbook are verified.
