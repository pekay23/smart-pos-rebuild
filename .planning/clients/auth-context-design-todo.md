# Auth context implementation slice

Purpose: resolve an already verified provider identity to Advanced POS's internal account and tenant membership context without adding business-data routes.

## Phases

- [x] Ground current token, API configuration, PostgreSQL migration, and identity schema.
- [x] Sketch two materially different lookup boundaries and choose one by security, data minimization, and verifiability. Candidate A won 23/25: only internal identity ID/status; candidate B's test matrix is grafted. See `auth-context-cross-judge.md`.
- [x] Implement exact internal identity lookup after token introspection. `POS_API_DATABASE_URL` is separate from the migration URL, uses an eight-connection bounded pool, and requires TLS certificate/host verification except loopback development. Unknown identities return 404, inactive records 403, DB/config failures 503. No email linking, auto-provisioning, or tenant data.
- [ ] Verify the exact identity query and restricted column privileges against PostgreSQL. The expanded Docker migration script stalled at `docker port`; Docker Desktop currently lists its disposable `advanced-pos-pg-check-12936` PostgreSQL 16 container as unavailable at host port 61135. Docker CLI commands remain unresponsive, so the migration never ran and the temporary container could not be cleaned up. Do not restart Docker Desktop while its other workspace containers may be serving projects; retry when its engine API recovers.
- [x] Update the API contract and implementation tracker with evidence and remaining limits. OpenAPI describes 200/401/403/404/503; runtime TLS, separate DB credentials and the tenant-data limits are documented. Keep the PostgreSQL integration proof pending.

## Boundaries

- Only accept `(issuer, subject)` after the existing access-token verifier succeeds.
- Never create or link an internal identity automatically using email or display name.
- Do not expose tenant business data or let provider claims determine app roles.
- Do not add tenant endpoints until the runtime database role, authorization policy, and RLS gates in `migrations/postgres/README.md` are proved.
- If PostgreSQL or a matching active internal identity is unavailable, return an explicit fail-closed outcome.
