# Candidate B: identity context service with a PostgreSQL repository

## Caller usage

```rust
let verified = authenticator.introspect(bearer_token).await?;
let context = identity_context.resolve(&verified).await?;
return Json(AuthContextResponse::from(context));
```

The authenticated caller receives only the internal identity ID, identity status, and counts of memberships by status. It receives no tenant names, tenant identifiers, branch data, station data, roles, or business records. This proves that provider identity maps to an internally provisioned person and gives the client enough state to understand whether account setup or access is pending. It does not authorize a tenant operation.

## Type and signature sketch

```rust
pub struct IdentityContextService {
    repository: Arc<dyn IdentityContextRepository>,
}

pub trait IdentityContextRepository: Send + Sync {
    async fn lookup_by_provider_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<IdentityMembershipSummary>, IdentityRepositoryError>;
}

pub struct IdentityMembershipSummary {
    pub identity_id: Uuid,
    pub identity_status: IdentityStatus,
    pub active_membership_count: u64,
    pub invited_membership_count: u64,
    pub suspended_membership_count: u64,
}

impl IdentityContextService {
    pub async fn resolve(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<ResolvedAuthContext, IdentityContextError>;
}
```

Concrete SQL implementation:

```rust
pub struct PgIdentityContextRepository { pool: PgPool }

impl IdentityContextRepository for PgIdentityContextRepository {
    async fn lookup_by_provider_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<IdentityMembershipSummary>, IdentityRepositoryError>;
}
```

The SQL statement binds the verified issuer and subject as parameters. It joins memberships to tenants only to count active memberships belonging to active tenants. The query groups by the one matched `person_identities` row. It does not use email or other mutable profile claims for matching and does not auto-create identities.

## Module map

- `auth.rs`: remains the sole source of `VerifiedIdentity` after OIDC introspection.
- `identity_context.rs`: small orchestration service and public result/error types; no Axum types.
- `identity_context/postgres.rs`: SQLx implementation of `IdentityContextRepository`.
- `lib.rs`: exposes `GET /v1/auth/context`; performs bearer verification, calls the service, maps errors to stable API envelopes.
- `config.rs` / `main.rs`: loads an optional runtime database URL separately from the migration URL, opens a bounded pool at startup, and installs the repository-backed service. The URL is never logged or returned.
- `openapi.yaml`, API README, and tracker: document the route as identity/account context only, with no tenant authorization guarantee.

## Failure and authorization semantics

- Invalid or expired token: `401 invalid_bearer_token` from the existing authenticator.
- Identity provider unavailable: `503 identity_provider_unavailable`.
- Runtime database not configured or unavailable: `503 identity_context_unavailable`.
- No exact `(issuer, subject)` row: `404 identity_not_provisioned`; provisioning remains an administrator workflow.
- Mapped identity is suspended/removed: `403 identity_inactive`.
- Active identity with no active memberships: return success with zero active membership count; tenant access remains unavailable.
- Active identity with memberships: return counts only. Do not treat counts as permission and do not use this route as middleware for tenant resources.

The route is allowed only when both token verification and the runtime database lookup are configured. It must not silently fall back to returning only provider claims when the database is absent.

## Verification plan

- Unit tests for context service outcomes using an in-memory fake repository: missing identity, active identity with zero memberships, active memberships, suspended identity, repository failure.
- PostgreSQL integration test against the versioned migration: exact issuer/subject lookup, same subject under different issuers remains distinct, email collisions do not link accounts, status counts include only expected membership/tenant states, and a no-match identity returns no row.
- HTTP tests verify token validation happens before the repository call; unconfigured DB fails closed; response JSON contains no tenant name/ID, branch/station, or role fields.
- Re-run migration smoke verification and API/Desktop compile/tests.
- Keep tenant routes gated on a later separate milestone for least-privilege database credentials, tenant authorization, FORCE RLS and transaction-local scope tests.

## Rationale and alternatives

This boundary isolates authentication from database details and gives the HTTP layer a stable service contract. It allows deterministic unit tests without a live database while a focused PostgreSQL integration suite proves the actual schema/query. A count-only response exposes less organization metadata than returning memberships or role grants and makes it harder for clients to treat provider identity as tenant authority.

Rejected alternatives:

- Returning only verified `(issuer, subject)`: does not establish that the person has an Advanced POS account.
- Looking up or linking by email: email may be absent, mutable, unverified, shared, or reused; it is not the identity key.
- Returning tenant IDs, names, branches, stations, or roles from this first lookup: this expands the data surface before tenant authorization and RLS are ready.
- Embedding SQL in the Axum handler: couples transport, mapping policy, and persistence and makes status decisions difficult to test without PostgreSQL.
- Auto-provisioning an identity during bearer verification: confuses authentication with account invitation and can create unauthorized accounts.

## Synthesis status

Candidate B only. This is a design proposal, not an implementation or a cross-judged choice. Compare it with candidate A before implementation.
