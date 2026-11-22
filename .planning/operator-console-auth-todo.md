# Operator service boundary work

Scope: implement a separate authenticated operator read/update path for hosted payment integration requests. Tenant access tokens must never authorize operator routes. This is not yet the full subscription, support, release or Kanban console.

## Architect workflow

- [x] Ground existing tenant OIDC introspection, route wiring, RLS and request storage in `.planning/arena/operator-console/grounding.md`.
- [x] Compare distinct operator service boundary designs and record the synthesis in `.planning/arena/operator-console/decision.md`.
- [x] Select the separate-process API/web boundary, disjoint auth configuration, provisioned `(issuer, subject)` operator identities, `NOBYPASSRLS` operator DB role, forced RLS, least grants, audited revision updates and public/private progress split.

## Implementation

- [x] Add operator-authenticated queue read through the separate `operator-api` executable, with its own OIDC environment prefix, five-minute maximum token lifetime, audience/client overlap checks, provisioned operator identity resolution and support-role authorization.
- [x] Add revision-checked status/progress updates, separate internal-note events, append-only audit, bounded cursor paging and idempotency handling.
- [x] Expose only customer-visible status/progress through the tenant request projection; keep operator policies scoped to `pos_operator_runtime` so it has no membership-table access.
- [x] Add migration 0027, role-scoped forced-RLS policies, least-column grants, local Compose role/service provisioning, OpenAPI and environment samples.
- [x] Add unit coverage for status transitions, unsafe text and cursor validation; restricted PostgreSQL verification confirms role isolation, request update, immutable events and denied tenant-detail edits/membership access.
- [x] Add authenticated HTTP coverage for disjoint tenant/operator tokens, inactive/unprovisioned accounts, revision conflicts, idempotent replay and end-to-end tenant progress visibility through separate restricted runtime roles.
- [x] Add an offline account provisioning/suspension/deactivation command using a separate non-bypass provisioning credential and append-only provisioning audit events. The actor label is operator-supplied audit context, not independently authenticated.
- [ ] Add an operator detail/event-history endpoint with field-level exposure review.
- [x] Build a separate operator-facing payment-request queue client after verifying its auth/API boundary. The standalone Vite app uses its own OIDC client configuration, session-only browser token storage, `/operator/v1/*` API client, queue filters/cursor paging, status transitions, public updates and private notes. Production IdP policy and visual/browser acceptance remain open.
- [ ] Verify production IdP MFA/passkey policy, deployment secrets and separate-service readiness before enabling operator access.
- [x] Update `docs/IMPLEMENTATION_TRACKER.md` with the architecture decision, verified backend slice and remaining console scope.
