# Operator support issue workflow

## Current extension (5 November 2026)

General branch-scoped tenant support intake is implemented alongside payment requests. It uses a distinct tenant-owned source table, `support:write` permission, exact idempotency, typed support-request issue links, operator-only notes and a public per-request status/message. Migration 0060, both web UIs and OpenAPI are in place. The restricted verifier applied all 60 migrations twice and passed both tenant/operator HTTP scenarios, including the link and private/public history checks.

Operator team assignment is now implemented with migration 0061. A separate forced-RLS directory returns active operator IDs and display names, never issuer/subject values. Provisioning and renaming display names use the separate audited provisioning credential. The issue board can assign or unassign active teammates, displays assignee names, rejects inactive IDs, and records revision-checked assignment events. Verification passed: 61 migrations applied twice, restricted operator and tenant HTTP scenarios passed, the operator web production build passed, and OpenAPI parsed successfully.

## Decision

Keep tenant-submitted payment integration requests as source records. Add a separate operator-only issue record for the work item. One issue can group several requests as duplicates or related reports, but each request keeps its own customer-visible status and progress message. Linking requests never merges tenant data or copies a public update between businesses.

This boundary follows the three council reviews: the current request table has payment-specific fields and tenant/branch policies; a global issue row cannot safely replace that tenant-owned source. A separate issue makes the internal Kanban lifecycle independent from the per-tenant request lifecycle and supports later non-payment support sources.

## Initial workflow contract

- Issue fields: bounded title, category, severity, workflow stage, optional current-operator assignment, internal due date, target release, revision and timestamps.
- Stages: `inbox`, `triaged`, `scheduled`, `in_progress`, `review`, `qa`, `release`, `resolved`, `blocked`, `duplicate`, `reopened`.
- Categories: `bug`, `feature_request`, `payment_integration`, `account_access`, `data_issue`, `other`.
- A payment request can be linked to at most one issue. An issue can link many payment requests with an explicit `primary`, `duplicate` or `related` relationship; an issue can have at most one primary request.
- Issue updates and links are revision-aware, idempotent and audited. Event rows are append-only; private notes and schedule details stay in the operator service.
- Assignment supports self claim/unclaim and team assignment via a separately scoped display-name-only directory. Only active directory entries can receive assignments.
- Only the separately authenticated support role can access issue routes or data. Tenant responses continue to expose only each tenant's own request status and customer-visible message; they never expose issue identifiers, internal stages, assignments, private notes, or links to other tenants.

## Delivery sequence

1. Add forced-RLS issue, request-link and append-only event tables with least-column grants for the operator runtime.
2. Add authenticated operator APIs for issue creation from a payment request, paged issue queue/history, revision-checked stage/assignment/schedule updates, and idempotent duplicate links.
3. Add a separate issues Kanban view in the operator desk; preserve the existing payment-request queue and its per-request public update controls.
4. Verify restricted HTTP behavior, request privacy across linked tenants, revision conflicts, idempotency, inactive/non-support denial, RLS and append-only grants.
5. [x] Extend intake to general tenant support requests using a tenant-owned record and a typed operator issue link. Migration 0060, forced-RLS/column-grant assertions and restricted HTTP privacy tests passed.
6. [x] Add a restricted operator display-name directory and active teammate assignment. Migration 0061, audited display-name provisioning, restricted HTTP tests and the operator UI are complete.
7. Add consent-based sanitized diagnostics and richer impact/contact consent to tenant requests; keep internal issue details operator-only.

## Acceptance boundary

This slice is complete when support operators can create issues, attach linked reports, move work through its lifecycle, claim/unclaim or assign active teammates, schedule work, add private notes, and inspect paged append-only history. Restricted PostgreSQL verification proves directory privacy, inactive-assignee rejection, tenant isolation and independent public progress.

The workflow does not grant operator access to tenant databases, accept raw diagnostics, or provide public updates on the canonical issue. General intake is a separate source record with independent public progress. Consent-based diagnostics, provider capability registry, customer notifications, health reporting and subscriptions remain later milestones.
