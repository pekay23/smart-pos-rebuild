# Pickup expiry worker: security design

## Recommendation

Run a separate, small Rust worker process with its own PostgreSQL login, `pos_api_expiry_worker`. It must be `NOSUPERUSER`, `NOBYPASSRLS`, `NOCREATEDB`, `NOCREATEROLE`, and `NOINHERIT`, and must not inherit from or assume the API runtime role. Give it only the column privileges needed for due-reservation discovery, reservation lines, expiry transition, matching event insertion, and allocation release. Do not grant access to customer data, sales, catalog descriptions, membership identities, or general audit tables. Keep the credential only in the worker service environment.

The connection's database role is the worker identity. It never sets `pos.identity_id`, `pos.membership_id`, or any other staff context, and no event may claim a staff actor. Add an explicit event actor discriminator such as `actor_kind='membership'|'system'`; keep human events membership-backed and require a membership for them. Permit `actor_membership_id IS NULL` only for `actor_kind='system'` expiry events. Preserve the tenant, pickup branch, reservation, revision and timestamp on the system event. Do not put customer contact snapshots in it.

## Transaction and locking protocol

Process a bounded batch of candidate IDs per drain pass (for example, 100), with one transaction per reservation:

1. Discover due `held`/`ready` IDs (`expires_at <= clock_timestamp()`) without row locks. This query is only a hint; it must not change state.
2. Read only that reservation's product IDs and allocation keys, sorted. Do not read customer/contact or descriptive product columns.
3. Acquire the existing sorted inventory advisory locks using `advanced-pos:inventory:v1:{tenant_id}:{branch_id}:{product_id}` before taking a reservation row lock.
4. Lock and re-read the reservation. Capture `clock_timestamp()` after lock acquisition and recheck tenant, eligible status, no sale, and deadline against that time. If no longer eligible or already expired, commit a no-op.
5. In the same transaction, move `held|ready -> expired`, increment revision, insert exactly one immutable `expired` system event at the new revision, and release its allocations with `release_reason='expired'`. Commit all or none.

The existing status trigger permits human transitions but currently rejects `expired`; the event schema requires a non-null membership actor; and the allocation release policy currently accepts only `cancelled`/`collected`. These are required schema/guard changes, not assumptions that the worker can bypass. Add narrowly scoped trigger branches and forced-RLS policies for system expiry that require the actual `current_user` to be the dedicated worker role, an actually due eligible reservation, no sale, matching tenant/branch/revision, and the matching expired event. Add only the needed `SELECT`, `UPDATE`, and `INSERT` column grants. In particular, do not use a caller-settable custom GUC as proof of worker identity. Keep the existing human RLS and trigger checks intact.

## Tenant isolation and least privilege

- Apply `ENABLE ROW LEVEL SECURITY` and `FORCE ROW LEVEL SECURITY` to each touched table. Worker policies must be operation-specific and narrow; the worker role must not own those tables or any security-definer function that bypasses policy evaluation.
- Bind every read/write to the same tenant and reservation IDs, and to the reservation's pickup branch. For allocations, require matching `pickup_reservation_id`, active/unreleased state, and only the `expired` release reason. A line may expose product UUIDs for lock acquisition but no snapshots or contact data.
- Restrict reservation updates to status, revision, and timestamp; event inserts to system expiry fields; allocation updates to release timestamp/reason. Use column-level grants in addition to RLS.
- Keep allocation expiry arithmetic based on `expires_at` independently of worker execution. `available-to-promise` must stop counting a reservation at its deadline even while its persisted status remains `held`/`ready`.
- No staff member is needed to authorize the background lifecycle. Human cancel/ready/collect routes retain their current authenticated membership context and policies.

## Reliability

Multiple replicas can select the same candidate. The shared inventory lock and reservation row lock serialize them; the later transaction rechecks and no-ops. Revision uniqueness and a matching-event constraint make state and audit materialization atomic and prevent duplicate transitions. Retry transient database errors with bounded backoff, then rediscover due IDs; restart needs no cursor. Yield after each bounded pass. Log counts and opaque IDs only, never names, phone numbers, request bodies, or contact snapshots. Alert on repeated failures and oldest overdue materialization age.

Provision the worker role in local database bootstrap and production deployment separately from the migrator and runtime role. Migrations must be repeatable and must not make the worker a table owner. Verify RLS with independent connections for runtime, worker, and a deliberately unauthorized role; prove wrong-tenant access is denied, worker cannot select contacts or update unrelated rows, and expiry has exactly one event and releases only its own allocations.

## Alternatives

### A. Dedicated restricted login and Rust worker — recommended

The database connection identity is auditable, privilege boundaries are explicit, and the current API runtime stays unable to perform global expiry writes. Existing Rust inventory-lock helpers can be reused without embedding scheduling into request handlers. Costs: an extra secret, deployment role provisioning, and a small worker service that needs monitoring.

### B. `SECURITY DEFINER` SQL procedure owned by a no-login role

The API could receive only `EXECUTE`, with a no-login owner given the minimum table privileges and worker-only policies; this avoids a worker password and centralizes the transition. It is a reasonable fallback if hosting cannot run a worker. Review burden is higher: owner and migration-role separation, `search_path` hardening, definer privileges, SQL lock-order parity, and ensuring the function cannot be used as a general cross-tenant oracle. Never own it with the superuser, table owner, or migrator, and do not expose arbitrary tenant/reservation parameters without validating them against due rows.

### C. Lazy expiry during authenticated queue reads — reject as primary mechanism

This avoids scheduling and separate credentials, but persistence depends on staff traffic and inactive branches may never materialize expiry. It also tempts attribution to the reading staff member or requires the same privileged system path anyway. It cannot be the sole implementation of automatic persisted expiry.

## Decision gates

Use alternative A unless the deployment platform cannot run a worker process. Do not enable the feature until the schema supports a null membership actor only for system events; all touched tables retain forced RLS; the role/column grants are narrow; expiry transition, event, and release guards agree; and race, restart, tenant-denial, deadline-with-worker-offline, and contact-omission checks pass. The deadline remains authoritative even if materialization is delayed.
