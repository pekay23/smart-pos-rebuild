# Candidate A: identity-only lookup at the API boundary

## Caller usage

```http
GET /v1/auth/identity
Authorization: Bearer <access-token>
```

After provider introspection succeeds, the API performs an exact lookup using the verified issuer and subject. A registered active identity receives its internal ID and active status. No matching identity is represented as `notRegistered`; inactive identity is `403`. This endpoint returns no tenant, membership, branch, station, role, or provider profile attributes.

```json
{
  "data": {
    "identityId": "...",
    "status": "active",
    "registered": true
  }
}
```

## Type and function sketch

```rust
pub struct RuntimeDatabase { pool: PgPool }

pub enum IdentityLookupError {
    NotRegistered,
    Inactive,
    Unavailable,
}

pub struct InternalIdentity {
    pub id: Uuid,
    pub status: IdentityStatus,
}

impl RuntimeDatabase {
    pub async fn find_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<InternalIdentity>, sqlx::Error>;
}

pub fn app_with_services(
    allowed_origins: Vec<HeaderValue>,
    authenticator: Option<Authenticator>,
    identity_database: Option<RuntimeDatabase>,
) -> Router;

async fn authenticated_identity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response;
```

Handler sequence:

1. Require configured `Authenticator`; parse bounded bearer token; introspect and validate it.
2. Require configured runtime identity database. Connect failure or missing config returns `503`.
3. Query `person_identities` by the verifier-produced `(issuer, subject)` with a bound SQLx query. Do not create rows and do not inspect email.
4. Return a minimal registered/active response; unknown identity returns `404` or an explicit `registered: false` response, while inactive identity returns `403`.
5. Keep the existing verifier failure behavior (`401` invalid/revoked, `503` provider unavailable).

## Module map

- `apps/api/src/config.rs`: read optional `POS_API_DATABASE_URL`; validate non-empty and keep it out of logs/debug output.
- `apps/api/src/main.rs`: create a bounded `PgPool` at startup when configured; pass it into the application state. Do not migrate on startup.
- `apps/api/src/lib.rs`: add the database handle to `AppState`; extend the existing identity handler after introspection; preserve health and calculation routes.
- `apps/api/src/identity_repository.rs` (or a narrow type in `lib.rs` for a first slice): exact `(issuer, subject)` SQL lookup and row mapping. Prefer a module once query/error concerns need independent tests.
- `apps/api/Cargo.toml`: enable SQLx UUID support if IDs are decoded as `Uuid`; otherwise select IDs as text and parse them explicitly.
- `apps/api/openapi.yaml`, `README.md`, `IMPLEMENTATION_TRACKER.md`: document the new contract, runtime credential and limits.
- `apps/api/migrations/postgres/README.md`: describe the required least-privilege grants for reading `person_identities`; runtime role stays distinct from migrator.

## Rationale

This is the smallest boundary that proves the transition from an externally verified person to an app-owned account. It fits the current single API state object and existing `person_identities` unique key, avoids introducing an abstraction that has only one caller, and keeps all tenant authorization for a later slice. It is useful for a first linked-account check without implying that tenant access is already authorized.

## Security and availability

- `issuer` and `subject` come only from successful introspection, never from request parameters or client-controlled identity headers.
- Query values are parameter-bound. The unique `(issuer, subject)` index keeps lookup deterministic.
- Runtime connection string remains server-only and is distinct from `POS_API_MIGRATION_DATABASE_URL`; pool bounds and acquisition timeout are configured.
- No automatic account provisioning, email matching, tenant discovery, tenant name, or role disclosure.
- Database absence, connect failure, or query failure fails closed with `503`; no fallback accepts the external identity as an app user.
- Identity inactive status must deny use even if the provider token is active.
- This route is not a tenant-data route and does not yet claim runtime RLS or authorization is complete. Before any tenant endpoint, use a non-owner/non-superuser/non-`BYPASSRLS` runtime role, add transaction-scoped authorization and RLS, and prove cross-tenant isolation under that exact role.

## Verification contract

- Endpoint is unavailable when verifier or runtime DB is not configured.
- Invalid, expired, revoked, wrong-audience, and provider-failure cases preserve `401`/`503` behavior and never query the DB.
- A registered active `(issuer, subject)` returns its internal ID; same subject under another issuer does not match.
- Unknown identity does not auto-create/link by email; inactive identity is denied.
- Database timeout or query error returns `503` and does not return partial identity data.
- PostgreSQL integration test uses the migration schema and a restricted runtime credential; query is parameterized and returns only the matching identity.
- Migration command remains explicit and separate from server startup.

## Rejected alternatives

- Returning membership, branch, station, and role grants in the identity handler: mixes identity linking with tenant authorization and exposes organization metadata before RLS and scoped authorization are verified.
- Automatically creating or linking identities by email: email is mutable, optional and non-unique in the schema; this risks account takeover or unintended merges.
- Putting Postgres query code directly inside the token-verifier module: provider cryptography and app persistence have separate trust boundaries and should not share ownership.
- Treating a valid provider subject as a registered POS account when PostgreSQL is unavailable: breaks local account suspension and internal provisioning policy.
