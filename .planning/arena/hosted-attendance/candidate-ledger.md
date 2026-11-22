# Hosted attendance candidate: append-only event ledger

## Problem

Hosted attendance must follow a staff member across their business roles and cashier-profile switches, remain branch-authorized, and close an overdue period even when that staff member is offline. The existing desktop model establishes important behavior: work periods are tied to a stable staff identity, a worker explicitly starts work, schedules may set the end time but never start work automatically, and local time boundaries must handle overnight shifts and daylight-saving changes. The hosted API currently has no attendance routes or schema.

## Usage (caller's view)

An owner or manager configures the employee's schedule for a branch:

```ts
await attendance.saveSchedule({
  tenantId, branchId, membershipId,
  timezone: 'Africa/Accra',
  weekdays: [1, 2, 3, 4, 5],
  startTime: '08:00', endTime: '17:00', expectedRevision: 3,
})
```

The worker clocks in under their own membership identity. A manager who switches into a temporary cashier profile still clocks their employment identity, not the temporary role:

```ts
const period = await attendance.clockIn({ tenantId, branchId, idempotencyKey })
```

The worker clocks out, or the restricted expiry runner records a scheduled automatic end:

```ts
await attendance.clockOut({ tenantId, branchId, idempotencyKey })
```

Managers query periods for branches they can manage; a worker can read their own periods. All timestamps and elapsed/late calculations come from the service/database, never from a client clock.

## Shape

Use immutable `attendance_events` as the authoritative history. Each event records a worker membership, branch, event kind (`clock_in`, `clock_out`, `scheduled_end`, or reviewed correction), server timestamp, actor membership when user initiated, idempotency key, and optional correction reason. Add a small `attendance_open_periods` row per worker as a transactional current-state projection and lock target; it is not authoritative history. The projection allows a unique constraint to enforce one open period across branches and makes concurrent/replayed clock actions safe. Period history is reconstructed from events and schedule snapshots.

Keep schedules as revisioned branch-scoped records, keyed by tenant, branch and worker membership. Snapshot the schedule revision, branch timezone, scheduled start and due timestamp into the `clock_in` event/period metadata so later edits cannot rewrite a completed period's expected hours. Resolve local time boundaries using the same policies as the desktop implementation: overnight shifts refer to the prior scheduled weekday; ambiguous fall-back starts choose the earlier instant and ends choose the later instant; nonexistent spring-forward boundaries advance to a valid local minute. A clock-in outside a scheduled window uses a bounded safety end, matching the desktop's manual-start policy.

The transaction boundary owns authorization context, event insertion, projection updates, audit and idempotency. RLS remains forced on schedule, event and projection tables. Runtime grants are column-limited; only a no-login worker owner may perform scheduled-end transitions through an execute-only wrapper. Managers/admins may configure schedules and read branch history through `attendance:manage` / `attendance:read`; staff may clock in/out and read their own attendance under a dedicated self-service permission. A temporary cashier profile never substitutes for the membership identity attached to an attendance event.

```rust
struct WorkSchedule {
    membership_id: MembershipId,
    branch_id: BranchId,
    timezone: Tz,
    weekdays: WeekdaySet,
    start: LocalTime,
    end: LocalTime,
    revision: Revision,
}

enum AttendanceEventKind { ClockIn, ClockOut, ScheduledEnd, Correction }
struct AttendanceEvent { /* immutable server-owned event and schedule snapshot */ }
struct AttendancePeriod { /* derived start/end, schedule, late and elapsed values */ }

async fn save_schedule(actor: AuthorizedManager, input: ScheduleInput) -> Result<WorkSchedule>;
async fn clock_in(actor: AuthenticatedMember, branch: BranchId, key: IdempotencyKey) -> Result<AttendancePeriod>;
async fn clock_out(actor: AuthenticatedMember, branch: BranchId, key: IdempotencyKey) -> Result<AttendancePeriod>;
async fn list_periods(actor: AuthorizedReader, query: PeriodQuery) -> Result<Page<AttendancePeriod>>;
async fn expire_due_periods(worker: RestrictedRunner, limit: BoundedLimit) -> Result<ExpirySummary>;
```

`attendance::clock_in` locks the worker's open-period projection, reconciles already-due periods, loads the branch schedule, derives the scheduled window from server time, appends `clock_in`, creates the projection and commits atomically. `clock_out` takes the same lock and appends exactly one closing event. `expire_due_periods` selects due projections in stable order and appends `scheduled_end` using the stored due instant, not current runner time. Reads fold immutable events into periods, using a bounded query and indexed membership/branch/time ordering.

### Module map

- `apps/api/src/attendance.rs`: validated schedule, clock, period-query and restricted worker domain transactions.
- `apps/api/src/lib.rs`: route adapters and permission/scope gates only.
- `apps/api/migrations/postgres/0057_hosted_attendance.sql`: forced-RLS schedule, event, open-period projection, indexes, event integrity checks and least-column grants.
- `apps/api/src/bin/attendance-expiry-worker.rs` and Compose service: bounded scheduled-end runner using a separate execute-only credential.
- `apps/web/src/HostedAttendancePanel.tsx`: authorized schedule management and period review; employee self clock action can later reuse the same typed API client.

## Synthesis decision

Pending peer comparison. This candidate intentionally explores immutable events as the source of truth, with a narrow mutable projection used only for uniqueness, locking and fast current-state reads.

## Tradeoffs accepted

- We accept one projection table and transaction lock in exchange for database-enforced single-open-period behavior under simultaneous sessions.
- We accept a bounded asynchronous runner and a short interval between due time and materialized end in exchange for reliable automatic closure while staff devices are offline.
- We accept schedule snapshots and explicit DST rules in exchange for stable historical attendance after schedule changes.
- We accept event-folding complexity in exchange for a durable audit trail and correction history that cannot rewrite original punches.

## Alternatives considered

- **Mutable period rows as the sole source of truth:** simpler period queries and less event folding, but corrections and payroll review would need to overwrite history or bolt on a second audit representation. This exposes historical consistency risk to every writer.
- **Pure event ledger with no open-period projection:** smaller schema, but every clock-in and worker pass must replay/lock a potentially growing event range to enforce one open period. It adds latency and concurrency complexity to callers' core action.

## Open questions and risks

- Should approved corrections be modeled as compensating events only, or should the first release require a separately permissioned review workflow before corrections can affect exports?
- Hosted authentication currently represents a person's tenant membership, while local desktop can distinguish login identity and cashier profile. The contract should keep this distinction explicit if cloud user switching is later introduced.
- Automatic closure requires production scheduling, monitoring, retry/backoff and an operational alert when the runner is down; the event design alone does not provide that service reliability.

## Next implementation step

Implement migration and restricted database transaction functions first, proving authorization, idempotency, DST-safe schedule snapshots, single-open concurrency and automatic-end behavior before adding the hosted web panel.
