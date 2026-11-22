# Candidate: database-owned expiry transition with a narrow worker runner

## Caller usage

```rust
let mut worker = PickupExpiryWorker::connect_from_env().await?;
loop {
    let count = worker.materialize_batch(50).await?;
    if count == 0 {
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
```

The standalone worker uses a dedicated login that can execute one database function and cannot read or mutate tenant tables directly. A no-login, non-BYPASSRLS role owns that function and has only the column grants and RLS policies needed to expire pickup holds. The API and browser never receive the worker login.

## Type and function sketch

```rust
struct PickupExpiryWorker { pool: PgPool }

impl PickupExpiryWorker {
    async fn connect_from_env() -> Result<Self, WorkerError>;
    async fn materialize_batch(&mut self, limit: u16) -> Result<u16, WorkerError>;
}
```

```sql
-- One invocation is one bounded transaction and handles at most one hold.
materialize_next_expired_pickup_reservation() RETURNS boolean
```

## Database transition

1. Discover one due ID in stable `(expires_at, tenant_id, id)` order without a row lock. `clock_timestamp()` controls all deadlines; transaction-start `now()` is unsuitable after lock waits.
2. Read that reservation’s line product IDs, sort and deduplicate them, then acquire the existing `advanced-pos:inventory:v1:<tenant>:<pickup branch>:<product>` advisory locks in that order.
3. Lock the reservation row only after its inventory locks. Re-read status, revision and deadline; capture database time after the lock wait. If another actor changed it or it is no longer due, return no change.
4. For `held` or `ready` with `expires_at <= captured_time` and no sale, update to `expired`, increment revision once, insert a matching immutable system event, release its unreleased allocation rows with reason `expired`, and commit atomically.

Expiry remains effective without this worker: availability excludes allocations after their deadline, and the API already projects a due `held`/`ready` record as `expired`. Materialization records the state/event revision and releases ledger rows for durable history and cleanup.

## Schema and RLS changes

- In an additive migration, add `actor_kind` to `pickup_reservation_events` with existing rows defaulted to `membership`; allow a null `actor_membership_id` only for `actor_kind='system'`. Keep all existing member events non-null and identity-bound.
- Extend the state trigger only for `held|ready -> expired`, preserving `sale_id IS NULL`; require a matching event at the new revision. Permit a system expiry event only when its actor kind is system, membership is null, station is null, acting branch is the reservation pickup branch, and its event time is at or after the reservation deadline.
- Add worker-only SELECT/UPDATE/INSERT policies for due reservations, their lines, matching expiry events, and their linked unreleased pickup allocations. Bind policies to `current_user='pos_api_expiry_worker'`; do not use a caller-controlled GUC as the privilege boundary.
- Keep forced RLS. Give the no-login function owner only the required table/column grants and no `BYPASSRLS`. Give the separate runner login `EXECUTE` on the single function only. Revoke function execution from `PUBLIC`.
- Use the immutable `pickup_reservation_events` expiry row as the one canonical audit fact. Do not attribute automatic expiry to the requesting cashier or manager in the generic human `audit_events` table.

## Concurrency and retry guarantees

- Cancellation and checkout already lock inventory before reservation rows. Expiry uses the same order, so two paths serialize without a row-lock/advisory-lock inversion.
- Multiple worker instances may discover the same due ID. The advisory locks plus post-lock row recheck make the loser a no-op; the status revision/event uniqueness prevents duplicates.
- If a worker fails before commit, status, event and allocation release all roll back. If it loses the response after commit, the next run sees no due `held`/`ready` record and does not repeat the transition.
- A hold that reaches its deadline while the worker waits is expired only after locks are acquired and the database clock is checked again. A hold cannot expire early because a worker captured a stale timestamp.
- Process one hold per database function transaction. The Rust loop drains a bounded number per pass, releases locks between records, sleeps when idle, logs failures and retries on the next pass.

## Alternatives considered

| Shape | Assessment |
| --- | --- |
| Direct worker login with table UPDATE/INSERT grants | Rejected. Even with RLS, a leaked or misused worker credential could issue arbitrary state writes and bypass the intended transition sequence. |
| Migration/superuser credential in a cron process | Rejected. It has unnecessary access to every tenant and schema object and makes worker compromise materially worse. |
| API background loop using the normal runtime role | Viable fallback, but it couples job health to every API replica and gives the runtime credential a callable maintenance capability. |
| Dedicated runner login invoking one database-owned, RLS-constrained function | Selected. It separates API and worker operations, supports one or many worker instances, keeps the critical transition in one transaction, and leaves the runner with execute-only access. |

## Rationale and deployment notes

This follows the prior pickup cross-judge’s rule: discover due IDs without row locks, then acquire sorted inventory locks, then lock and recheck each reservation. The current model already makes deadline expiry effective without a worker; the worker is a durable materializer, not the authority that determines whether a hold still consumes stock.

Provision two database roles outside ordinary application migrations: a no-login `pos_api_expiry_worker` function owner and a login-only `pos_api_expiry_runner` with an externally supplied password. Local PostgreSQL bootstrap and the restricted verifier should provision both. Production deployment must provision the same roles before migration 0054 and store the runner URL only in the isolated worker service. The Compose worker should use the same `pos-api pickup-expiry-worker` command; keep its connection pool small and bind no HTTP port.
