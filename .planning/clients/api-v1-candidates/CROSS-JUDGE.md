# API v1 first-slice cross-judge

**Decision:** Choose Candidate A, the stateless sale-calculation service, as the implementation base. It is the only candidate that can produce a runnable service with a useful `/v1` operation using repository capabilities that exist today. Preserve Candidate B's explicit server-derived actor boundary and fail-closed rule for future tenant-data routes, but defer its catalog use case until actual cloud identity and tenant-scoped persistence adapters exist.

This is a review of the two proposals against `.planning/clients/api-v1-implementation-todo.md`, not implementation evidence. No production code was changed.

## Acceptance rubric scores

Scores are 1–5; 5 means the proposal directly meets the criterion within its stated first slice.

| Criterion | Candidate A | Candidate B | Judgment |
|---|---:|---:|---|
| 1. Independently runnable HTTP service within this workspace | 5 | 4 | Both define a new `apps/api` service and health route. A also defines an immediately usable domain endpoint. B's binary can run health, but its protected endpoint cannot serve data until adapters are supplied. Neither has been implemented or run. |
| 2. Explicit `/v1` wire contract, stable versioning, no local DB/domain leakage | 5 | 4 | A gives concrete request/response/error shapes and maps through `pos-core` into service DTOs. B correctly separates wire DTOs, application read models and SQL representation, but the actual catalog contract and error envelope are less complete. Neither provides a checked-in OpenAPI document yet. |
| 3. No unauthenticated tenant/business data; safe health metadata | 5 | 5 | A's endpoint only calculates over caller-provided input and reads/writes no tenant data; B requires real principal resolution and tenant-scoped reads, with no allow-all adapter. Both keep health minimal. A must retain its explicit warning that submitted prices/tax are untrusted and not authoritative. |
| 4. Browser or CLI consumer can run and call without Tauri | 5 | 3 | A provides a concrete curl call and browser-native use case. B's examples are future contracts; its protected route is intentionally unavailable without cloud adapters, so a consumer can only call health in the first runnable increment. |
| 5. Truthful startup/configuration and security/product limitations | 4 | 5 | Both accurately state major missing capabilities. B is especially clear that health is the only live operation before real adapters. A documents limits well, but its acceptance depends on implementing and documenting request limits, safe integer bounds, CORS/configuration and error mapping rather than treating the proposal's suggested settings as already achieved. |
| **Total** | **24/25** | **21/25** | **A is the better first implementation slice.** |

## Why A is the base

The todo asks for a service that is independently runnable and that a non-Tauri client can call. Candidate A reaches both goals without inventing tenant auth or business data: it exposes `POST /v1/sale-calculations`, delegates arithmetic to the existing `pos-core`, and explicitly labels results as non-authoritative previews. The operation provides a real versioned DTO and exercises the cross-platform HTTP boundary now.

Candidate B's security design is directionally sound, but its protected endpoint is a port sketch, not a presently implementable catalog feature. It has no real `PrincipalResolver`, catalog storage adapter, product dataset, or cloud session. As written, the only immediately usable route is health, which is too small to meet the todo's meaningful consumer-call goal. Implementing a fake token or seeded catalog to make its example work would contradict its own security honesty.

The recommendation aligns with `.planning/clients/MULTI_CLIENT_ARCHITECTURE.md`: establish a separately runnable Rust API and versioned cross-platform boundary, keep DTOs separate from local models, and do not expose desktop commands or local SQLite as cloud authority. It also leaves room to add authenticated catalog and server-authoritative sales after the cloud foundations are real.

## Exact grafts from B into A

1. Keep B's rule that no data-bearing route is mounted until a verified cloud identity adapter exists; construct a server-derived `ActorContext`, never accept tenant scope from request data, and fail closed on missing or invalid identity.
2. When catalog work begins, retain B's separation between HTTP DTOs, application read models and storage adapters. Require tenant predicates from server-derived actor scope in every query.
3. Preserve B's explicit statement that health is process liveness only and reveals no business, secret, configuration or database details.
4. Keep B's prohibition on treating local SQLite accounts, passkeys, desktop keyring state or staff profiles as cloud credentials.
5. Retain B's future sale boundary: server-side recomputation and transaction authority, with idempotency bound to canonical request content and tenant scope. Do not extend A's caller-priced calculation endpoint into checkout.

## Rejections and deferrals

- **Reject B's catalog endpoint as the first implementation slice.** Its operation cannot return tenant data with the current repository, and a fake resolver or in-memory catalog would create false security/product confidence. Revisit it after a cloud identity/session decision and a real tenant-scoped persistence adapter are selected.
- **Defer auth, PostgreSQL, catalog, checkout, and other tenant-data routes.** They require decisions and infrastructure absent from this slice; they are not implied by a runnable calculation service.
- **Do not graft B's broad application/adapter skeleton prematurely.** Keep A's first service limited to config, router, DTO mapping, safe errors and `pos-core` calculation. Add ports when a real use case needs them.
- **Do not claim a checked-in OpenAPI contract exists.** A has an explicit JSON contract in its proposal but leaves OpenAPI generation versus checked-in specification open. Resolve that choice during implementation and add the artifact/conformance check before describing the contract as OpenAPI-backed.
- **Do not use the desktop HTTP adapter as a prerequisite.** A's suggested desktop preview integration is optional follow-on work; first prove the API from CLI/browser without Tauri, and leave checkout on its current path.

## Repository premise check

The grounding premise is true in the current checkout: root `Cargo.toml` lists only `crates/pos-core` and `apps/desktop/src-tauri`. The candidate files and searched app/crate source contain no HTTP service/framework, cloud auth implementation, PostgreSQL driver/provider, schema or migration adapter. In particular, B's `PrincipalResolver`, `CatalogReader` and `ActorContext` are proposed interfaces, not existing implementations; A's proposed Axum service is also not present. The current desktop SQLite/passkey boundary is installation-scoped and is not a cloud identity provider.

Therefore, neither proposal is implemented evidence. Candidate A is implementable now as a stateless calculation API using existing Rust domain logic; it must not be represented as cloud POS, tenant-authenticated, catalog-backed, persisted, or an authoritative quote.

## Next bounded implementation

Implement Candidate A's `apps/api` and `/healthz` plus `/v1/sale-calculations`. Enforce request and line-count bounds, checked arithmetic and browser-safe integer limits, stable error DTOs, non-sensitive health output, and an explicit local CORS/configuration policy. Add a versioned OpenAPI artifact or generated schema with a conformance check. Verify the service from CLI or browser without launching Tauri. Document that tenant auth, PostgreSQL, authoritative prices/tax, persistence and checkout remain absent. Update the tracker only after those checks actually pass.
