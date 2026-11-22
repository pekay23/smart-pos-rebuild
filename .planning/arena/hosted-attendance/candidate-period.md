# Hosted attendance candidate: period aggregate

## Problem

The hosted product needs reliable employee time records, branch-specific schedules, role-aware access and automatic scheduled end even when a device is offline. The current desktop implementation anchors work periods to the staff user, does not auto-start a scheduled shift, and resolves overnight and daylight-saving boundaries explicitly. Hosted identity must instead use the durable tenant membership because a manager or owner may switch into a separate cashier profile without changing who is employed.

## Usage (caller's view)

An owner or manager sets the branch schedule for one staff member:

```ts
const saved = await attendance.putSchedule({
  tenantId, branchId, membershipId,
  timezone: 'Africa/Accra', weekdays: [1, 2, 3, 4, 5],
  startTime: '08:00', endTime: '17:00', revision: 4,
})
```

The staff member explicitly clocks in/out. The caller supplies an idempotency key, never a time; the service uses authenticated membership identity and server time:

```ts
const period = await attendance.clockIn({ tenantId, branchId, idempotencyKey })
await attendance.clockOut({ tenantId, branchId, idempotencyKey: newKey })
```

A manager retrieves a bounded branch history, while a staff member retrieves only their own records:

```ts
const page = await attendance.listPeriods({ tenantId, branchId, from, to, cursor })
```

## Shape

Model each work period as one aggregate row, authoritative for its clock-in snapshot, current status and final end. Its lifecycle is `open -> closed`; only an automatic-end worker or authenticated staff clock-out can close it. A partial unique constraint on `(tenant_id, membership_id) WHERE ended_at IS NULL` guarantees that one employee cannot have simultaneous open work periods across branches. Begin each start/end/worker transaction by locking the membership's advisory key, then validate expected state, write the period transition and append its audit event in the same transaction. Idempotency rows or a unique `(tenant_id, membership_id, idempotency_key)` constraint make replay deterministic.

Schedules are branch- and membership-scoped records with a monotonically increasing revision. Clock-in snapshots the selected schedule revision, local schedule date, timezone, scheduled boundary and due instant into the work-period row. A history record separately captures `clocked_in`, `clocked_out`, `scheduled_end`, and any manager-approved correction as append-only events; period updates are accepted only through transaction functions that append matching history. A later schedule edit never changes the snapshot. Overnight windows attach to the previous workday. Resolve ambiguous DST starts to the earlier instant, ambiguous ends to the later instant, and nonexistent boundaries to the first valid local minute, matching local attendance behavior. If an explicitly clocked-in worker has no active schedule window, use a bounded safety duration; never create a period merely because schedule time begins.

Tenant authorization gates schedule and history APIs. Schedule writes require branch-scoped `attendance:manage`, manager history reads require `attendance:read`, and staff punch operations require the current authenticated membership plus branch access and a dedicated `attendance:self` capability. Owners/managers without an attendance schedule can remain non-shift workers; if they work, they clock in under their membership identity. Temporary cashier mode changes only sales permissions and never the `membership_id` on a period. All tables use forced RLS and column-level runtime grants. Only a no-login worker role may execute a narrowly scoped, bounded close-due-period function; the runner cannot read or update period tables directly.

```rust
struct ScheduleInput {
    membership_id: MembershipId,
    timezone: Tz,
    weekdays: WeekdaySet,
    start_time: LocalTime,
    end_time: LocalTime,
    expected_revision: Option<Revision>,
}

struct WorkPeriod {
    id: PeriodId,
    membership_id: MembershipId,
    branch_id: BranchId,
    scheduled_start: Option<DateTime<Utc>>,
    started_at: DateTime<Utc>,
    due_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    end_reason: Option<EndReason>,
    revision: Revision,
}

async fn put_schedule(actor: BranchManager, input: ScheduleInput) -> Result<Schedule>;
async fn clock_in(actor: AuthenticatedMember, branch: BranchId, key: IdempotencyKey) -> Result<WorkPeriod>;
async fn clock_out(actor: AuthenticatedMember, branch: BranchId, key: IdempotencyKey) -> Result<WorkPeriod>;
async fn list_periods(actor: AttendanceReader, query: PeriodQuery) -> Result<Page<WorkPeriod>>;
async fn close_due_periods(worker: RestrictedRunner, limit: BoundedLimit) -> Result<CloseSummary>;
```

`clock_in` selects and locks the schedule and open-period key, checks branch access, derives the current scheduled window from database time, inserts a new period with immutable schedule snapshot and audit event, and commits atomically. `clock_out` checks current membership ownership, locks the same key, and closes the single open period at database time. `close_due_periods` locks due open rows with `SKIP LOCKED`, closes them at their stored `due_at`, appends a `scheduled_end` audit event and commits each bounded batch. The period row makes current-state screens and payroll exports straightforward; audit events preserve every accepted transition.

### Module map

- `apps/api/src/attendance.rs`: period aggregate rules, schedule-boundary calculation, clock actions, listing and worker batch operation.
- `apps/api/src/lib.rs`: thin authenticated route handlers and `attendance:*` permission checks.
- `apps/api/migrations/postgres/0057_hosted_attendance.sql`: schedules, periods, append-only audit, partial unique open-period index, forced RLS, transaction context and least-column grants.
- `apps/api/src/bin/attendance-expiry-worker.rs`: bounded scheduled-end runner with a dedicated execute-only connection; operational restarts, alerting and metrics remain deployment work.
- `apps/web/src/HostedAttendancePanel.tsx`: manager schedule editor and branch timecard review; later staff self-service uses the same typed API client.

## Synthesis decision

Pending peer comparison. This candidate deliberately uses a period aggregate as the current authoritative state, with append-only audit rows recording each permitted transition.

## Tradeoffs accepted

- We accept updating the period row on close in exchange for simple, bounded timecard and payroll-range queries.
- We accept duplicating key transition data into an append-only audit row in exchange for reconstructable history and reviewability.
- We accept a small per-membership lock plus a partial unique index in exchange for a strong one-open-period invariant across branches and concurrent devices.
- We accept a background runner and bounded delay around scheduled end in exchange for automatic close when the worker is offline.
- We accept storing a schedule snapshot on every period in exchange for historical stability after schedule edits.

## Alternatives considered

- **Immutable event ledger as source of truth plus a current projection:** better event-first audit semantics, but period queries require folding event history or maintaining another projection that has nearly the same write invariants. This exposes more moving pieces than current reporting needs.
- **One mutable row without append-only audit:** smallest database interface, but corrections and transitions would overwrite evidence. It fails the product's need to explain work records and approval history.

## Open questions and risks

- Should the first version permit manager-approved corrections, or should it record only staff punches and scheduled auto-ends until a dedicated approval workflow is ready?
- Who is authorized to see the full branch timecard list: manager only, or manager and admin? Permission grants should be explicit rather than inferred from UI role names.
- The automatic close depends on runner monitoring and retry. Production cannot claim reliable automatic end until that runner's schedule, alerting and database recovery are deployed and tested.

## Next implementation step

Implement the schedule and period migration with its partial unique open-period constraint, forced RLS, transition audit trigger and restricted auto-close function, then add and verify the API transactions before the UI.
