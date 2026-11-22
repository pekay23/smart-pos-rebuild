# OIDC resource-server verification decision

**Status:** Architecture selected; verifier implementation is gated on a real provider configuration and access-token profile proof.  
**Date:** 30 October 2026

## Decision

Keep the Rust API as the resource server. The first API identity slice uses authenticated ZITADEL token introspection with `private_key_jwt`. This accepts the provider's opaque or JWT access tokens and asks the provider to confirm active status/revocation before the API accepts the identity. The OIDC client flow belongs in browser/native clients; API verification is a separate contract.

Local `jsonwebtoken` verification with a bounded issuer-pinned JWKS cache remains a future optimization only if a live ZITADEL proof confirms a JWT access-token profile that the API can reliably distinguish from an ID token. Opaque access tokens are the provider default. Never accept an ID token as a bearer credential for POS API routes.

This staged choice follows the independent security, platform and POS workflow reviews in [`auth-council/`](auth-council/). ZITADEL's application settings support opaque or JWT token types; its docs also state that introspection accepts both token forms and checks revocation. The API project's audience must be distinct from native/web client IDs and from the operator portal's audience. Per-request introspection adds provider latency and availability dependence, so do not expose high-volume tenant routes until the live flow is load-tested and a safe session/token-cache strategy is chosen.

## Verifier contract

- Configure a single exact HTTPS issuer and its trusted discovery/JWKS or introspection endpoint server-side. Never select a host from token `iss`, `jku`, or `x5u`; reject discovery issuer mismatches and unapproved redirects.
- Bound bearer-token length and parse a strict Bearer header. Reject malformed/unsigned tokens, unknown keys, unsupported algorithms and incompatible JWK `kty`/`use`/`alg` before exposing any claims.
- Require and validate signature, exact `iss`, API `aud`, `exp`, and non-empty `sub`; validate `nbf` when present (require it if the configured provider profile guarantees it). Pin an explicit asymmetric algorithm allowlist. Verify `azp`/client ID against the allowed client set as an additional caller restriction.
- Verify and document the provider's actual access-token vs ID-token distinction before using local JWT validation. Do not assume RFC 9068 `typ=at+jwt` until the provider's emitted token profile is proven to use it. If audience and claims cannot robustly distinguish the types, use introspection.
- For introspection, pin the issuer and same-origin endpoint in server configuration, use `private_key_jwt` from a server-only key file, disable redirects, enforce timeouts and response/token byte limits, and validate active status, exact issuer, API audience, token type, subject, expiry, not-before, and allowed client ID. Treat provider errors as unavailable and invalid/inactive tokens as unauthorized; never log token material.
- If local JWT verification is later selected, use an immutable bounded cache per configured issuer, periodic refresh, one single-flight refresh on unknown `kid`, bounded network timeouts/body size, and a bounded stale-key window. Unknown `kid` after refresh is rejected. When no valid cached key is available and the provider is unreachable, fail closed with an availability error rather than bypass verification.
- Keep active tenant membership/role in PostgreSQL and check it on each authorized request. Provider token revocation does not revoke app memberships or local sessions; account disable, membership changes, lost-device response, and operator actions need an application-side session/version policy as well.
- Tenant routes and operator routes require separate audiences/authorization domains. No tenant token may authorize operator access. Client roles or tenant IDs in tokens are advisory only; tenant access comes from live membership and scoped grants.
- Return a uniform invalid-token response; log only bounded outcome/reason metadata and correlation IDs. Never log bearer tokens, full claims, email, auth codes, refresh tokens or provider secrets. Add source/IP rate limits before exposing login callbacks or token-bound APIs.

## Staged implementation gate

1. In a disposable ZITADEL instance, register separate POS API and operator resources/clients; prove Authorization Code + PKCE, API audience, token type, introspection client authentication, exact issuer, and ID-token rejection.
2. Load-test per-request introspection. If latency/availability are not acceptable, prove the provider's JWT access-token/ID-token distinction before adding local JWKS verification or a short-lived app session.
3. Map verified `(iss, sub)` to the internal identity; fetch active memberships and role grants from PostgreSQL within a tenant-scoped transaction. Keep branch/station scope and quick cashier unlock separate from cloud identity.
4. Prove owner/cashier invite and recovery for staff without personal email, lost passkey/device removal, membership downgrade, tenant suspension, operator audience separation, and native system-browser PKCE before exposing data routes.

## Primary references checked 30 October 2026

- [jsonwebtoken validation](https://docs.rs/jsonwebtoken/latest/jsonwebtoken/struct.Validation.html)
- [jsonwebtoken JWK sets](https://docs.rs/jsonwebtoken/latest/jsonwebtoken/jwk/struct.JwkSet.html)
- [OpenID Connect crate](https://docs.rs/openidconnect/latest/openidconnect/)
- [ZITADEL application token settings](https://zitadel.com/docs/guides/manage/console/applications-overview)
- [ZITADEL web keys](https://zitadel.com/docs/guides/integrate/login/oidc/webkeys)
- [ZITADEL token introspection](https://zitadel.com/docs/guides/integrate/token-introspection)
- [ZITADEL OIDC endpoints](https://zitadel.com/docs/apis/openidoauth/endpoints)
- [RFC 9068 JWT access-token profile](https://www.rfc-editor.org/rfc/rfc9068)
- [RFC 8725 JWT best current practice](https://www.rfc-editor.org/rfc/rfc8725)
- [RFC 8252 native app OAuth](https://www.rfc-editor.org/rfc/rfc8252)

