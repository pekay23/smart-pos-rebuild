# Auth context design cross-judge

## Rubric scores

Scores are 1–5 (5 is strongest for the criterion).

| Criterion | Candidate A: API boundary lookup | Candidate B: service + repository | Assessment |
|---|---:|---:|---|
| Exact verified `(iss, sub)` mapping; no email association | 5 | 5 | Both use only verifier-produced issuer and subject and reject email linking. |
| Minimize tenant-data leakage; avoid implying authorization | 5 | 4 | A returns only internal identity ID/status. B returns membership counts; this is low sensitivity but exposes unnecessary tenant-derived state and may be mistaken for access. |
| Clear fail-closed configuration and database failures | 4 | 5 | Both fail closed. B names distinct stable outcomes more completely; A should settle the unknown identity contract (`404 identity_not_provisioned`) and unify public error codes. |
| Testability and fit for the current single caller | 4 | 5 | B's repository contract enables isolated service tests. A can still test SQL behavior against PostgreSQL and handler behavior; introducing an async trait/service for one lookup is premature. |
| Smallest maintainable public surface | 5 | 3 | A extends current AppState and route. B adds service, trait, repository implementation and a new context route before a second consumer exists. |
| **Total** | **23/25** | **22/25** | Candidate A narrowly wins for this first slice. |

## Recommendation

Use Candidate A as the base: keep the existing `/v1/auth/identity` route, perform an exact parameter-bound database lookup only after token introspection succeeds, and return the internal identity ID/status. Use a distinct runtime PostgreSQL credential and bounded pool; never migrate on startup. Choose one stable response for an unprovisioned identity (`404 identity_not_provisioned`) and return `403 identity_inactive` for suspended or removed records. Database/configuration failures return `503` and never fall back to provider-only identity acceptance.

Preserve Candidate B's testing discipline: cover identity resolution outcomes at the route and add PostgreSQL integration coverage for exact issuer/subject matching, same subject under different issuers, no email matching, status handling and database failures. Its repository abstraction can be introduced later if the lookup gains a second caller or substantive policy logic.

Do not return membership counts, tenant identifiers or names, branches, stations or roles in this slice. The endpoint establishes only that a verified provider identity maps to an internally provisioned person; it grants no tenant access. Tenant routes remain gated on an authorized membership model, least-privilege runtime role, RLS, and cross-tenant integration tests.

## Grafts and rejections

- Grafted from B: explicit HTTP/error test matrix and PostgreSQL integration cases.
- Rejected for now from B: service/trait layering and membership-count response. One caller does not yet justify the extra abstraction; counts add tenant-derived data without enabling a permitted operation.
- No changes to source code were made by this design review.
