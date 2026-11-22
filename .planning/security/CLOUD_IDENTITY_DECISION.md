# Cloud identity and tenant data decision

**Status:** Preferred direction for the first hosted prototype; provider and production deployment remain unconfigured.  
**Date:** 30 October 2026

## Decision

Use ZITADEL as the initial OIDC identity provider and PostgreSQL as the POS data store, accessed only through the Rust API. Browser and native clients will use Authorization Code with PKCE. The API will validate access-token signature, issuer, audience, expiry, and token type, then map `(issuer, subject)` to an internal person identity. It will resolve tenant, branch, station, role, and entitlement permissions from application-owned database records on each request.

ZITADEL organization membership and token claims may help onboarding, but are not POS authorization. Clients cannot choose an authorized tenant by sending its ID. No database credentials, OIDC client secrets, signing keys, or payment secrets go into desktop/mobile bundles. The separate operator portal will use a distinct issuer audience/client and separate operator permissions.

## Why this shape

Three independent council reviews recommend ZITADEL OIDC plus Rust/PostgreSQL as the best first hosted shape for this cross-platform, Rust API product. It gives the native clients a standard OIDC/PKCE boundary while keeping branch/station policy in one place. Supabase is the closest managed alternative, but its passkey guide currently marks the feature experimental; Neon is a database rather than an identity provider, and Better Auth would make us operate another authentication service. The reviews and primary vendor references are in [`auth-council/`](auth-council/).

This is a preference, not a procurement or production approval. ZITADEL must pass the proof points below. PostgreSQL schema and provider identifiers remain vendor-neutral so identity or hosting can be changed without changing POS user IDs.

## First domain contract

- An external identity is keyed by stable OIDC `(issuer, subject)`, never by email or username. Email is optional contact data, not an identity key; invited cashiers may not have a personal email address.
- One person can hold memberships in multiple businesses. Membership roles and grants can be scoped to the whole tenant, a branch, or a station; the API must resolve the scope against the referenced resource's tenant before allowing an action.
- Branches own inventory and stations belong to one branch. Stations are devices, not users. Quick cashier switching on shared tills remains a separately audited POS profile unlock and does not swap the cloud account/session.
- Tenant-facing clients only call the Rust API. PostgreSQL is not directly exposed to them. Every tenant-owned row carries `tenant_id`; repository methods take the authenticated principal's resolved tenant context, not a raw request tenant ID.
- Database RLS is a second boundary when the runtime database role is configured as a non-owner, non-superuser role without `BYPASSRLS`. It does not replace Rust authorization. The initial migration defines data invariants; RLS policies are added together with transaction-scoped tenant context and verified using the actual runtime role before hosted data routes ship.
- Audit records identify the internal actor and tenant. Secrets, passkey material, access tokens, card data and full diagnostic payloads are prohibited from audit metadata.

## Required proof before provider integration

1. Confirm username-only creation/provisioning for staff who lack email or phone, plus owner-mediated recovery that does not disclose credentials.
2. Reserve a stable authentication domain and validate its WebAuthn RP ID/origin and native app association requirements before enrolling production passkeys.
3. Prove system-browser Authorization Code + PKCE on Windows, Android, iOS and HarmonyOS targets; retain a safe fallback where a platform authenticator is unavailable.
4. Verify Rust JWT validation, issuer/audience separation, JWKS refresh/key rotation, short access-token lifetime, logout/revocation behavior, and tenant suspension handling.
5. Verify membership removal and role changes take effect promptly for high-risk actions, even while an access token remains cryptographically valid.
6. Select a PostgreSQL host/region only after backup restore, pooling, migration, data-location, support, uptime and cost review. Free-tier quotas are prototype limits, not a service guarantee.

The access-token resource-server decision is detailed in [`OIDC_RESOURCE_SERVER_DECISION.md`](OIDC_RESOURCE_SERVER_DECISION.md). ZITADEL's default opaque token behavior means local JWT verification is not assumed; the exact JWT access-token profile must be proven or API introspection used.

## Consequences and exclusions

The API remains a stateless calculation preview until PostgreSQL repositories, authentication middleware, domain routes, and persistence are implemented. The migration in `apps/api/migrations/postgres/` is a versioned starting schema, not an applied database or completed tenant-security control. Cloud passkeys, account recovery, tenant billing, provider connections, branch stock routing, sync and operator workflows remain unimplemented.

Official documentation checked 30 October 2026:

- [ZITADEL pricing](https://zitadel.com/pricing)
- [ZITADEL OIDC web keys](https://zitadel.com/docs/guides/integrate/login/oidc/webkeys)
- [ZITADEL passkeys](https://zitadel.com/docs/concepts/features/passkeys)
- [ZITADEL native application types](https://zitadel.com/docs/guides/manage/console/applications-overview)
- [Supabase passkeys](https://supabase.com/docs/guides/auth/passkeys)
- [Better Auth username plugin](https://better-auth.com/docs/plugins/username)

