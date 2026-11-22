# Operator console architecture decision

Date: 1 November 2026

## Decision

Use a separately deployed operator API and web client, with its own OIDC client/audience, authenticator, session, database credential and runtime process. The first operator feature is a narrowly scoped hosted payment-integration request inbox. Do not mount global operator routes in the tenant API or grant operator access through `pos_api_runtime`.

Operator accounts are provisioned out of band and keyed by the verified `(issuer, subject)` pair. Server-owned roles determine access; tenant membership, email, caller-supplied role and tenant tokens confer no operator authority. The operator DB role must be `NOBYPASSRLS`, not a table owner, and use forced RLS plus least-column grants. No default operator or first-user endpoint is created.

The operator workflow uses revision-checked updates, append-only operator/request events and audit records. Tenant APIs expose only the request's explicit customer-visible status/progress projection; internal notes and operator identities remain private. MFA/passkeys are a deployment requirement enforced by the operator IdP policy. The current ZITADEL introspection path does not prove factor use, so the API must not claim an MFA assertion it cannot verify.

## Candidate comparison

Both complete candidates recommend the same security boundary and vertical slice: a separate process, separate operator client, provisioned identities, isolated DB role, forced RLS, audited revision-checked status updates and distinct public/private messages. Candidate A adds more operational safeguards: startup rejection for overlapping auth allowlists, event and sensitive-read audit, deactivation/suspension behavior, mutation idempotency, CSRF/session guidance and explicit status-transition checks. Candidate B gives a slightly smaller endpoint/type sketch. Select Candidate A's safeguards while keeping the first release bounded to support-role request triage.

The shared-process operator router/pool alternative is rejected for the initial release because it shares process and wiring failure modes with tenant auth. A shared database remains acceptable only with the operator-specific non-bypass runtime role, strict RLS and grants.

## Deferred implementation decisions

- Implement only support access to payment requests first; do not introduce tenant impersonation, payment execution or broad financial data access.
- Define the exact status transition matrix and text bounds in code and API documentation before enabling updates.
- Decide if support needs tenant/branch display names; begin with IDs and submitted request fields, then add only an audited minimal projection if operationally necessary.
- Document and verify IdP MFA/passkey policy, short token lifetime and the offline account-provisioning/deactivation runbook before production enablement.
- Keep billing, release, security administration, tenant/device controls, support grants and Kanban as later modules with separate capabilities and policies.

## Evidence

Compared `.planning/arena/operator-console/candidate-a.md` and `candidate-b.md` against `.planning/arena/operator-console/grounding.md`. Existing tenant auth, request storage, forced RLS and grants were traced in the grounding note. No application implementation was made as part of this design decision; operator inbox implementation remains open.
