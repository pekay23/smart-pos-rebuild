# Operator console architecture grounding

## User goal and current scope

Build a separate operator/developer console for subscription support and issue triage. The immediate slice is the hosted payment integration request inbox: operators need to review a request, assign its workflow status and provide customer-visible progress that tenant managers can read. This design must leave a path for billing, tenant/device controls and the larger Kanban, but those features are not implemented by the immediate slice.

## Current authenticated tenant flow

- `apps/api/src/config.rs::Config::from_env` loads one `Authenticator` from `POS_API_OIDC_*` and one database URL, `POS_API_DATABASE_URL`.
- `apps/api/src/main.rs::serve` creates one restricted `pos_api_runtime` pool and passes it to one Axum router.
- `apps/api/src/lib.rs::app_with_runtime` builds one shared `AppState` and one `/v1` route tree.
- Each tenant handler calls `resolve_identity`: verify a Bearer token through Authenticator introspection, then resolve the verified `(issuer, subject)` to active `person_identities`.
- Tenant handlers call `identity::list_tenant_memberships` and set transaction-local identity/membership/branch scope. Tenant routes are additionally guarded by server-owned permissions and forced RLS.
- Migration 0026 adds `hosted_payment_integration_requests` with forced RLS. Tenant managers with `payments:write` can insert/list within a branch. The tenant runtime can select limited fields and insert; it has no update/delete grants or operator read policy.

## Existing request UX and API

- `apps/web/src/BranchPaymentOptionsPanel.tsx` mounts `PaymentIntegrationRequestPanel` for staff with `payments:write`.
- `apps/web/src/PaymentIntegrationRequestPanel.tsx` saves country, provider, terminal model, capability, business impact and optional bounded detail. It truthfully says that no developer inbox or notification is connected yet.
- `apps/api/src/payment_requests.rs` normalizes/validates fields, rejects common credential/payment-data markers, uses branch transaction context, idempotency keys, forced-RLS writes and tenant audit events. Tenant GET is branch-scoped.
- Progress status exists as a read-only column but currently remains `submitted`; there is no operator identity, queue, update route or operator UI.

## Security and product constraints

- Operator and tenant bearer tokens must never share an authorization path. A separate audience alone is insufficient: use disjoint audiences and client allowlists, plus a provisioned operator account keyed by verified `(issuer, subject)` with explicit server-owned roles. Never match by email or tenant membership.
- The current ZITADEL introspection implementation validates issuer, subject, active state, audience, client ID, expiry, not-before and token type. Official ZITADEL documentation says `amr`, `acr` and `auth_time` are not returned by its introspection endpoint. Therefore passkey/MFA assurance must be enforced by the operator IdP/application policy or a verifiable alternate signal; do not claim API-level MFA enforcement without evidence.
- Product plan requires distinct operator authentication, short sessions, explicit support/billing/release/security roles, audited time-limited support access and no hidden tenant-client superadmin controls.
- Database operator access should be separate from `pos_api_runtime`, use `NOBYPASSRLS`, forced RLS and least-column privileges. No operator gets tenant financial records beyond fields necessary for their task.
- Operator status changes must use expected revisions, be append-audited, and expose only an explicit customer-visible progress field to the originating tenant. Internal notes must be separate.
- No bootstrap superuser, universal support PIN, hard-coded email allowlist or plaintext secret storage.

## Candidate design alternatives to compare

1. Separate operator service process and web app, independent authenticator, separate PostgreSQL login/role, shared database with strict operator RLS.
2. Same Rust process but separate operator route tree/state/authenticator/pool, plus a separate operator web app and database login. This reduces deployment duplication but shares process failure and memory boundary.

## Gradeable design rubric

1. Uses disjoint operator and tenant OIDC audiences/client IDs and maps only provisioned `(issuer, subject)` operator accounts to explicit roles.
2. Keeps tenant DB credentials, routes and permissions unable to enumerate or mutate other tenants' requests.
3. Uses operator-specific `NOBYPASSRLS` access, forced RLS, least-column grants and transaction-local operator identity; keeps audit append-only.
4. Provides revision-checked status updates and an explicit separation between internal notes and tenant-visible progress.
5. Defines safe operator provisioning, lockout/deactivation, short session and MFA/passkey assurance without inventing unsupported token claims.
6. Gives a small, testable interface that can extend to subscriptions and Kanban without turning the first inbox slice into a global admin bypass.

## Evidence references

- `apps/api/src/auth.rs`, `apps/api/src/config.rs`, `apps/api/src/main.rs`, `apps/api/src/lib.rs`, `apps/api/src/identity.rs`
- `apps/api/migrations/postgres/0026_hosted_payment_integration_requests.sql`
- `apps/api/docker/grant-runtime.sql`, `docker-compose.yml`, `tools/verify-api-postgres-migrations.ps1`
- `apps/api/src/payment_requests.rs`, `apps/web/src/PaymentIntegrationRequestPanel.tsx`
- `docs/PRODUCT_COMMERCIAL_PLATFORM_PLAN.md` section “Operator / developer console”

## Grounding sources

- CodeGraph trace of `resolve_identity → Authenticator::introspect → Authenticator::new`, route wiring, and `Config::from_env`.
- Read-only auth flow and database-boundary explorations completed in this turn.
- Official ZITADEL docs: [token introspection](https://zitadel.com/docs/guides/integrate/token-introspection) and [claim availability](https://zitadel.com/docs/apis/openidoauth/claims).
