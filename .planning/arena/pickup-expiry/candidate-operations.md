# Pickup expiry worker: operations design

## Proposed shape

Run a supervised expiry loop beside the API process at first, using the existing `POS_API_DATABASE_URL` connection pool to call a narrowly granted database routine. Keep it independent from request handlers and authentication state. If the API and worker need separate scaling later, run the same loop as a `pos-api expire-pickups` service command; the database routine and its lock protocol stay identical.

## Why this fits the current runtime

- `pos-api` currently has two modes: long-running HTTP `serve` and one-shot `migrate`. Its runtime pool allows eight connections with a three-second acquisition timeout.
- Docker Compose already separates long-lived API/operator services from a one-shot migration/provisioning chain. It has no scheduler or worker service today.
- The API Tokio runtime currently enables macros, networking, multithreaded execution, and signals; adding a timer loop is a small runtime change.
- The tracker already plans to deploy API and workers independently. Keeping the worker loop callable as its own command preserves that path without adding a scheduler dependency now.

## Runtime behavior

- Poll every 5–10 seconds and process at most 100 due reservations per pass. Each materialization call handles one reservation in its own database statement/transaction, allowing other work between reservations and keeping rollback scope small.
- The routine discovers due IDs without row locks, then follows the agreed order: sorted inventory advisory locks, reservation row lock, database-time deadline/status/revision recheck, and atomic status/event/allocation update.
- If several API instances run the loop, they may discover the same reservation. Advisory locks and the post-lock recheck make the loser a no-op. The expiry deadline already removes stock from ATP, so a worker delay affects persisted history only, not availability safety.
- Log each pass's materialized count and errors without customer contact details. On transient database errors, stop that pass, emit a structured error, and retry on the next interval with bounded exponential backoff and jitter. Do not spin on a single bad row.
- On shutdown, stop accepting new sweep work, allow the in-flight transaction to finish, then join the worker task. A later start resumes by rediscovering due rows; no cursor/checkpoint is required.

## Packaging and alternatives

- **Dedicated scheduled CLI/container:** best once production operations can run scheduled jobs or a separate worker deployment. It isolates API latency and uses the same idempotent routine; at-least-once invocation is safe. It requires a new runtime command and Compose/deployment service now.
- **Embedded supervised loop (recommended initially):** smallest integration with the current service and keeps expiry running anywhere the API is healthy. It needs multi-replica-safe database locking and careful shutdown, which the database protocol already supplies.
- **Database extension scheduler:** reject as a default. `pg_cron` availability and policy differ across Neon, Supabase, and self-hosted PostgreSQL, so it would couple the feature to a provider and complicate local parity.
- **Lazy read-only expiry:** already reports effective `expired` state and stops counting the allocation at the deadline, but it does not persist the state/event or release old allocation rows. Keep it as the safety fallback, not the materializer.

## Security and operations boundary

The HTTP runtime must only execute a stored routine that can expire due `held`/`ready` reservations. Give the routine a dedicated `NOLOGIN`, `NOBYPASSRLS` owner role with column-scoped grants and worker-only RLS policies; do not connect the worker using the migration account or invent a staff identity. Expose the execution grant only to the API runtime role. The worker may append the immutable `expired` system event and release its reservation allocations; it cannot alter sale, customer, inventory quantity, or other tenant records.

The separate worker command can later share the API runtime credential or receive a dedicated login that has only `EXECUTE` on the routine. API health checks should not fail because a sweep is temporarily unavailable; monitor sweep errors and backlog age/count separately so database incidents do not make checkout availability worse.

## Rejected approach

A one-shot process run only by developers is insufficient for production: it leaves reservations unmaterialized indefinitely. Relying on client activity or request-triggered cleanup also produces tenant traffic-dependent expiry and unclear operational monitoring.
