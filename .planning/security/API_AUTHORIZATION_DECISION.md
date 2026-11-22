# Hosted API authorization decision

## Context

The hosted API currently verifies an opaque identity-provider token, maps the
verified issuer and subject to an internal identity, and lists active tenant
memberships with branch/station grants. That discovery route is not an action
authorization mechanism. No hosted POS workflow route is exposed yet.

## Council review

- The normalized-model review recommends a permission catalog, tenant role
  definitions, normalized role-permission links, and grants tied to a role
  definition. Custom permissions must not come from JSON or provider claims.
- The API/RLS review recommends completing the restricted PostgreSQL proof
  first, then adding a small server-owned canonical role evaluator. It warns
  that free-text role keys and RLS grant existence must never be treated as
  sufficient permission checks.

## Lead decision

1. Keep the current membership route limited to access discovery.
2. Add a server-owned baseline evaluator for the four shipped roles as an
   intermediate layer. Unknown and tenant-defined role keys receive no
   permissions. Check each action against the requested resource scope.
3. Before exposing business routes, complete the PostgreSQL integration proof
   under the restricted runtime credential and connection pool.
4. Add normalized tenant role/permission persistence and audit controls before
   enabling tenant-created roles or delegable permission changes.
5. A protected route must resolve active identity, membership, tenant, branch,
   station and resource ownership on the server; enforce the permission;
   evaluate subscription/device entitlements separately; set transaction-local
   RLS context from those resolved records; and write required audit events.

This sequence uses a small canonical evaluator to establish role semantics
without claiming that it is the final custom-role model. Neither the evaluator
nor membership discovery alone authorizes hosted sales, inventory, finance,
staff, subscription or operator actions.

## Current verification and remaining work

- `cargo test -p pos-api`: 22 passed, including five permission/scope tests.
- The disposable PostgreSQL verifier could not start because Docker was
  unreachable. It also could not contact Docker for cleanup; no Docker engine
  restart was attempted because other workspaces may use its containers.
- The evaluator is not yet consumed by an HTTP route. No protected POS workflow
  endpoint should be added until restricted-role RLS integration is proven.
- No local `psql`, `postgres`, `pg_ctl`, or `initdb` executable is available;
  the bundled toolchain also has none in its checked location.
- Next: restore Docker availability or use an independently provisioned
  disposable PostgreSQL instance, run and fix the verifier, then design
  normalized tenant roles and wire a read-only protected route before writes.
