# Hosted attendance candidate: period records as current state

## Problem

Hosted staff are tenant memberships, and a membership may change its active cashier profile while retaining the same employment identity. Hosted attendance is not implemented yet. The desktop model already supports schedules, manual start/end, late starts, scheduled auto-end without auto-start, overnight shifts, and daylight-saving boundaries. The hosted design must retain those rules while adding branch authorization, concurrent requests, restricted database access, and automatic closure when a worker is offline.

## Usage (caller's view)

The cashier clocks in against their signed-in employment identity; the server supplies timestamps and resolves the active branch schedule:

```ts
await attendance.clockIn({ tenantId, branchId })
await attendance.myCurrentPeriod({ tenantId, branchId })
await attendance.clockOut({ tenantId, branchId, periodId })
```

An authorized manager sets a weekly schedule for an employee and reviews their branch periods:

```ts
await attendance.saveSchedule({ tenantId, branchId, membershipId, schedule, expectedRevision })
await attendance.branchPeriods({ tenantId, branchId, fromDate, toDate, cursor })
```

The API stores the stable staff membership ID, not a temporary cashier-mode/session ID. Ending a temporary cashier profile therefore does not end employment attendance.

## Shape

Use a row-per-work-period model as the authoritative current state. A period row contains its tenant, branch, staff membership, schedule revision snapshot, scheduled local boundaries and timezone, server clock-in timestamp, due timestamp, end timestamp, end reason, and revision. `ended_at IS NULL` identifies an open period. A partial unique index enforces at most one open period per staff membership across the tenant; clock-in takes a membership-scoped transaction lock and checks the schedule and open-period constraint together.

Keep a separate append-only event table for `clocked_in`, `clocked_out`, `auto_ended`, and audited manager correction events. Period state and its corresponding event are written in one transaction. Clients cannot directly write period or event rows. The API owns all transitions, stamps times from the server, validates expected revisions for any manager correction, and returns a typed period object. The first release should not expose corrections until a reviewed correction policy is implemented.

A schedule row is scoped by tenant, branch, and staff membership, with timezone, weekdays, local start/end, active flag, revision, and actor audit fields. Store the schedule revision and resolved UTC boundaries on each period so later schedule edits cannot rewrite historical attendance. Resolve ambiguous/nonexistent DST boundaries using the existing desktop policy: earlier valid start, later valid end, nonexistent time advances to first valid minute. Overnight shifts use the prior workday schedule.

A dedicated restricted worker scans overdue open rows and closes them at `due_at`, not worker execution time. It inserts exactly one `auto_ended` event and advances the row revision transactionally. Database uniqueness/locking makes repeated or concurrent passes idempotent. Reads may also display a derived due/overdue badge, but must not be the mechanism that closes attendance; the worker provides persistence while users are offline.

Suggested Rust boundary:

```rust
pub struct WorkScheduleInput { /* validated weekday/time/timezone values */ }
pub struct WorkPeriod { /* stable membership, branch, schedule snapshot, server times, state */ }
pub enum AttendanceError { InvalidInput, Forbidden, Conflict, NotFound, Database }

pub async fn clock_in(ctx: BranchActorContext, pool: &PgPool) -> Result<WorkPeriod, AttendanceError>;
pub async fn clock_out(ctx: BranchActorContext, period_id: Uuid, pool: &PgPool) -> Result<WorkPeriod, AttendanceError>;
pub async fn save_schedule(ctx: BranchManagerContext, membership_id: Uuid, input: WorkScheduleInput, expected_revision: i64, pool: &PgPool) -> Result<WorkSchedule, AttendanceError>;
pub async fn list_branch_periods(ctx: BranchReportContext, range: BusinessDateRange, cursor: Option<AttendanceCursor>, pool: &PgPool) -> Result<AttendancePage, AttendanceError>;
pub async fn end_due_periods(pool: &PgPool, limit: NonZeroUsize) -> Result<usize, WorkerError>;
```

Validation sits at HTTP boundaries (date/time, timezone, bounded page/range, revision); the module owns transition rules and database transaction/RLS setup. API authorization checks branch access plus attendance permission before calling the module. Forced RLS uses transaction-local tenant, membership, branch, and staff-scope context. Runtime role receives only the exact schedule/period/event columns needed; the worker uses a separate no-login function owner and execute-only wrapper, following the restricted expiry-worker pattern. No provider subject or personal login secret is returned in staff attendance responses.

### Module map

- `apps/api/src/attendance.rs`: domain types, schedule boundary resolution, clock transitions, branch queries, audit/event write, RLS context.
- `apps/api/src/lib.rs`: authenticated self clock routes and manager schedule/branch-report routes; authorize role/branch before module calls.
- `apps/api/migrations/postgres/0057_hosted_attendance.sql`: schedule, period and event tables, constraints, forced RLS, narrow grants, audit policies, and worker wrapper.
- `apps/api/src/bin/attendance_worker.rs` plus Compose service: bounded auto-end pass using its isolated worker URL.
- `apps/web/src/HostedAttendancePanel.tsx`: employee clock status/actions, manager schedule editor and branch attendance report, gated by API permissions and current branch.

The interface hides timestamp authority, timezone resolution, concurrency, audit coupling, and worker idempotency. Callers only choose a branch, submit a schedule or explicit clock action, and render a typed period. The surface exposes the manager's schedule revision because stale edits must be reported instead of silently overwriting them.

## Synthesis decision

This candidate chooses a current period row plus append-only event evidence. Its main advantage is direct, indexed queries for current attendance and simple uniqueness enforcement. The alternative event-ledger candidate may be preferable if multiple corrections and reconstruction become central; compare both before implementation.

## Tradeoffs accepted

- We accept duplicated transition state in the period row and its event history in exchange for simple current-status reads and durable audit evidence.
- We accept a dedicated worker and restricted database path in exchange for guaranteed persisted auto-end during user/device offline periods.
- We accept snapshotting schedule boundaries per period in exchange for stable historical interpretation after schedule changes.
- We accept per-membership single-open-period enforcement across all branches in exchange for preventing simultaneous attendance from multiple locations; an authorized correction/transfer workflow can address genuine branch moves later.

## Alternatives considered

- **Events-only ledger:** append `clock_in`/`clock_out` events and derive open state on each query. It keeps one source of truth and makes history natural, but complicates uniqueness under concurrent clocks and makes active/branch reports scan or maintain projections. Rejected for this first vertical slice because attendance needs fast current status and strong one-open enforcement.
- **Reuse local desktop work-period tables:** exposes local SQLite assumptions and user IDs to hosted callers and cannot provide cloud-wide concurrency or automatic closure while the desktop app is offline. Rejected because it cannot satisfy hosted multi-station behavior.

## Open questions and risks

- Should a manager be allowed to manually close an abandoned period before scheduled end? This candidate excludes that action until the audit/correction workflow is specified.
- How frequently may scheduled periods be auto-closed? Worker cadence sets the maximum delay after the due time, while the recorded end remains exact at `due_at`.
- Does attendance later need payroll breaks/overtime, geolocation, or legal retention controls? Those require jurisdiction-specific policy and should not be inferred from this basic work-period record.

## Next implementation step

Implement the migration and restricted worker transition first, then the authenticated API routes and role-scoped web panel against the same period contract.
