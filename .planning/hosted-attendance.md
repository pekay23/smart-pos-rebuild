# Hosted attendance design

## Usage

An employee clocks in/out under their authenticated tenant membership. A manager who temporarily switches into cashier mode keeps the same attendance identity. Managers set the branch schedule and review branch periods:

```ts
await clockIn({ tenantId, branchId, idempotencyKey })
await getMyAttendance({ tenantId, branchId })
await clockOut({ tenantId, branchId, idempotencyKey })
await saveSchedule({ tenantId, branchId, membershipId, schedule, expectedRevision })
await listBranchAttendance({ tenantId, branchId, fromDate, toDate, cursor })
```

## Shape

- `attendance_schedules` stores a revisioned schedule per active staff membership and branch. Its timezone follows the branch profile; edits affect future clock-ins only.
- `attendance_periods` is the operational current-state record. It snapshots the schedule revision, timezone, scheduled start and due instant when the employee explicitly clocks in. Server time owns `started_at` and `ended_at`.
- A partial unique index allows at most one open period for a staff membership across the tenant. Clock actions take a transaction-scoped membership lock and use an idempotency key.
- `attendance_events` appends `clocked_in`, `clocked_out`, and `scheduled_end` evidence in the same transaction as each period transition. Manager corrections are deferred until a reviewed correction policy exists.
- A restricted no-login function owner and separate runner can close due periods at the recorded `due_at`, including while staff devices are offline. The runner cannot read attendance tables or call the internal transition routine. No worker action creates a clock-in.
- Ambiguous daylight-saving starts use the earlier instant, ambiguous ends use the later instant, and nonexistent wall times advance to the first valid minute, matching local desktop attendance behavior. Overnight schedules use the prior workday's schedule.
- API permission checks and forced RLS both scope rows to the tenant and branch. Staff can access their own actions/periods; owner/admin/manager roles can manage branch schedules and inspect branch history. No provider identity, credentials, or private login data enter attendance responses.

## Synthesis

The selected period-row candidate scores 25/25 in the cross-judge against the event-only ledger's 23/25. Its current-state row directly answers clock status and branch reports and hosts the one-open-period partial unique index. Grafted from the ledger candidate: same-transaction immutable event/audit writes, idempotent clock actions, stable-order bounded worker selection, and scheduled end at `due_at`. Rejected: event-folding as the primary current-state query and a second mutable open-period projection; both add state/query complexity without improving first-release needs. The judge found no unresolved design conflict.

## Work phases

- [x] Ground local attendance and hosted auth/RLS patterns.
- [x] Compare two structurally different candidates and cross-judge the design.
- [x] Implement migration, API routes, RLS, audit, idempotency, and restricted worker.
- [x] Add hosted employee clock view and manager schedule/history UI.
- [x] Run Rust formatting and restricted PostgreSQL migration/API verification; update the tracker. Web and OpenAPI checks were already completed during implementation and were not changed by the final database fixes.

## Tradeoffs and limits

- The worker cadence bounds the delay before the automatic closure is materialized; the recorded end remains exact at the stored due instant.
- Attendance periods do not yet calculate payroll, breaks, overtime, location proof, or jurisdictional compliance.
- Manager corrections are not exposed until a reviewed compensating-event workflow is defined.
- A manager-created schedule cannot auto-start a work period; an employee must clock in.
