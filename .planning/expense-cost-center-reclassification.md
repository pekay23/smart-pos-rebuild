# Audited expense cost-center correction

## Council decision

Accounting, finance, and cashier-operations personas agree this is an analytical attribution correction, not a financial reversal. Keep the expense source, GL posting, cash location events, cash movements, and Z-close snapshots unchanged. Append a versioned correction with the previous and replacement cost center, actor, reason, idempotency key, and system-recorded time. The replacement applies to the original expense date in current reports; linked reimbursements use that same corrected center on their own return date. Audit history retains when the reclassification was recorded.

The correction applies to the complete expense family, including partial or complete reimbursements. Initial workflow supports moving the full expense between one station, store overhead, or legacy-unassigned. No splits, cross-store reassignment, or custom effective date.

Require the dedicated `expenses:reclassify` permission. Owner and administrator roles get it by default; an owner may grant it to a custom manager role. Require an expected revision to prevent stale concurrent corrections. Same-key retry with identical details returns the existing correction; key reuse with different details fails.

## Contract sketch

- `ReclassifyExpenseCostCenterInput`: `expense_id`, `expected_revision`, `cost_center_kind`, optional `station_key`, `reason`, `idempotency_key`.
- `ExpenseCostCenterReclassification`: expense/store/business IDs, revision, old/new kind/key/name snapshot, reason, actor ID/name, recorded time, idempotency key.
- Command boundary: require `expenses:reclassify`; lock with `BEGIN IMMEDIATE`; load current effective center (source row or latest event); check expected revision; validate target station is active and same-store; append immutable event and audit event atomically.
- Reports select the latest correction for current attribution. Expense reimbursements join through their parent expense so original and return amounts move together. GL and cash workflows do not read or write this table.
- Expense list page snapshots include the correction table's row-ID high-water mark so pagination/export cannot mix old and new labels.
- History query returns the full correction chain, actor, time, reason, and before/after values.

## Acceptance checks

- Reclassifying an expense with partial/full reimbursements moves the original and returns together while preserving store totals, GL, cash locations, drawer movements, and saved Z JSON.
- Station A to B, station to overhead/unassigned, and corrections back to a prior center append history rather than editing it.
- Cashier or unauthorized staff, cross-store/unknown/inactive station, invalid reason, stale revision, and conflicting idempotency key are rejected with no partial writes.
- Identical retries return one stored correction.
- A page/export keeps its original correction snapshot while a fresh query sees the latest correction.
- Rebuilt historical P&L uses current attribution and remains dated by original expense `occurred_at` and reimbursement `returned_at`.

## Remaining limits

There is no formal accounting-period lock or immutable signed management-report snapshot in the local product. Do not imply that report regeneration preserves the values seen before a later correction. Drawer Z snapshots remain immutable and are not accounting-period locks.

## Current implementation status

- [x] Migration 0032 adds append-only reclassification events, immutable triggers, idempotency uniqueness and owner/admin default permissions.
- [x] Rust command validates same-store active stations, reason, expected revision and retries; it appends an audit event and leaves the source expense, reimbursement, journals and cash records unchanged.
- [x] History UI shows old/new centers, reason, actor/time and expense returns.
- [x] Expense report snapshots include a reclassification high-water mark; P&L and station attribution read the latest event while retaining original expense/return dates.
- [x] Rust tests cover migration constraints/default roles, valid and invalid destinations, multistep revision, retry replay/conflict, stale revision, immutability, unchanged source/reimbursement records, and corrected P&L.
- [ ] Add direct command-boundary authorization tests, including cashier denial and explicit custom-role grant.
- [ ] Review the workflow in the current authenticated Windows UI.

The authoritative product-wide checklist is [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md).

