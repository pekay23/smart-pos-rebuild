# Hosted attendance implementation

## Work phases

- [x] Ground existing local attendance behavior and hosted identity/authorization boundaries.
- [x] Compare event-ledger and period-record designs; record the selected shape and rejected tradeoffs in `hosted-attendance.md`.
- [x] Implement migration 0057, API routes, RLS, audit, idempotency, restricted worker roles, and PostgreSQL integration coverage.
- [x] Add hosted attendance and schedule UI for staff and authorized managers.
- [x] Finish Rust compile/unit verification and full restricted PostgreSQL migration/integration verification; update the implementation tracker.

## Grounded constraints

- Hosted attendance has a schedule, current-period row, append-only event table, self-service and management API, restricted scheduled-end worker, and web UI. All 57 PostgreSQL migrations applied twice; restricted operator and tenant HTTP scenarios passed, including clock-in/out, replay protection, scheduled closure and execute-only worker checks. Cargo build cleanup now uses a 20 GiB managed-output threshold and a 20-command backstop.
- A hosted worker is identified by tenant membership; temporary cashier profiles must not replace employment identity.
- Local desktop behavior is the reference: explicit clock-in, schedule-based automatic end, no automatic start, late login keeps scheduled end, overnight shifts and daylight-saving boundaries are handled.
- Hosted writes must use authenticated identity context, tenant/branch permission checks, forced RLS and least-column runtime grants.
- Scheduled automatic end must work even if the staff member is offline and must never create a clock-in.
