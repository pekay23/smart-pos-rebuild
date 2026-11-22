# API v1 first-slice synthesis

## Decision

Candidate A is the base: a stateless `POST /v1/sale-calculations` endpoint backed by the existing `pos-core` arithmetic. The cross-judge scored it 24/25 against the acceptance rubric versus 21/25 for Candidate B. It can be implemented with current domain code and produces a real cross-platform HTTP call without fabricating hosted identity or business records.

## Grafts and limits

- Keep Candidate B's hard rule that future tenant-data routes derive an `ActorContext` from verified server identity, never caller-supplied business/store IDs. Every later repository query must enforce that scope and fail closed.
- Keep B's separation of HTTP DTOs, application read models and persistence adapters when catalog/auth routes are implemented; do not add its empty port skeleton to this stateless slice.
- Keep health metadata minimal and explicitly liveness-only. Desktop SQLite users/passkeys are not cloud credentials.
- Preserve A's warning that caller-provided prices and tax rate are untrusted inputs. Calculation output is neither an authoritative quote nor checkout; no sale is stored.
- Add a checked-in OpenAPI v1 contract, request-size and line-count limits, stable errors, JavaScript-safe integer bounds, and exact opt-in CORS origins before calling this slice complete.
- Do not connect desktop checkout to this API yet. The existing Rust command remains authoritative for local sales.

## Rejected for this slice

- Candidate B's protected catalog endpoint is deferred until a real cloud identity/session and tenant-scoped persistent catalog exist.
- Health-only is too small to exercise a useful versioned domain operation.
- A sale mutation, local SQLite HTTP wrapper, hard-coded token or fake in-memory tenant/catalog would misstate current security and data authority.

## Evidence

The repository presently contains only `pos-core` and the Tauri desktop crate in the Cargo workspace. `pos-core` already exposes checked totals and stable line allocations. No cloud auth, PostgreSQL, or API service exists. The judge report is `.planning/clients/api-v1-candidates/CROSS-JUDGE.md`.
