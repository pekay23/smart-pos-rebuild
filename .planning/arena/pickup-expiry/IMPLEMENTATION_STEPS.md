# Pickup expiry worker implementation

## Phases

- [x] **Ground:** confirm pickup holds use server deadlines for ATP even without a worker; transitions and checkout lock inventory before reservation rows; `get_reservation` reports effective expiry without persisting it; API currently has no scheduler or worker role.
- [x] **Sketch:** compare tenant-safe expiry execution models for least privilege, lock ordering, retry/restart behavior, operational packaging, and migration compatibility. See the three candidates in this folder.
- [x] **Agree:** the independent cross-judge selected a dedicated scheduled worker command/container, an execute-only login, and a no-login forced-RLS function owner. The API runtime does not run global expiry work. See `cross-judge.md`.
- [x] **Implement:** migration 0054 adds typed system events, worker-only forced-RLS policies and column grants, plus the atomic `pos_worker.materialize_next_expired_pickup_reservation()` function. `pos-api pickup-expiry-worker` drains at most 100 holds per pass with bounded retry backoff; Compose provisions and supervises it.
- [ ] **Verify:** prove RLS/least grants, idempotent restart/replay, expiry during lock waits, no premature expiry, allocation release, and no change to sale/cancel/ready behavior; apply migrations twice. Latest run applied all 54 migrations twice and passed the execute-only invocation/direct-table denial checks. The role-provisioning SQL was also applied twice to a disposable PostgreSQL instance, confirming idempotence and role flags. The full HTTP suite timed out at an earlier hosted cash-expense request before reaching the added due-hold scenario; full materialization and concurrency checks remain open.
- [ ] **Scrap:** redesign if implementation needs a worker bypass or broader database privilege than the selected sketch permits.

## Design requirements

- Expiry remains effective for available-to-promise as soon as `expires_at` passes, even if materialization is delayed or the worker is offline.
- Worker discovery holds no reservation row locks. For each candidate, take sorted tenant/branch/product advisory locks, then lock and recheck the reservation using database time after lock waits.
- Expiry only transitions `held` or `ready` reservations with no sale and a due deadline; it appends one immutable system event and releases linked allocations atomically.
- The worker must not impersonate a staff member or gain broad runtime/migration privileges.
- Multiple API replicas may run the worker safely. Work is bounded, idempotent, observable, and retried after restart.
