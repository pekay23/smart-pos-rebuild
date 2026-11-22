# Station contribution reporting

## Council decision

Accounting, cashier-operations, and finance personas agree that expense attribution, payment location, and recording context are separate facts. Never infer the cost center from the till, safe, payment method, cashier, or device.

New expenses must be explicitly assigned to one active station or store overhead. The general expense form defaults to store overhead so shared costs are not silently charged to whichever register the manager happens to use. Historical expenses migrate as legacy unassigned. A station report is a management contribution view before overhead, not a fully loaded profit statement.

Sales, discounts, refunds, and COGS follow the original sale's station. Expense reimbursements offset the original expense's cost center. Actual cash returns continue to affect only the receiving location and, for a register, its current open shift. The store-level general ledger remains authoritative and is not given inferred station dimensions.

## Required implementation

- [x] Migration adds immutable cost-center type, optional station key, and saved station-name snapshot to expenses; historical rows stay legacy unassigned.
- [x] Expense entry requires station-direct or store-overhead and validates store scope and staff authority. Cashiers may use their assigned station or store overhead; managers/report-authorized staff may choose another station.
- [x] Expense history and CSV show the cost center; a reimbursement inherits its original expense's cost center.
- [x] Profit reports accept an optional station. Station contribution includes that station's net revenue, original-sale refunds, saved COGS, and direct expenses. Store overhead and historical-unassigned expenses remain visible and are excluded from station contribution.
- [x] Store-wide P&L includes all expenses and separates overhead and legacy-unassigned amounts. Unassigned legacy sales remain visible rather than being guessed into a station.
- [x] Live reclassification of an already-posted expense is an append-only audited correction workflow; do not update the source expense in place. Migration 0032, role permission, history UI, report/P&L attribution and stable report snapshots are implemented.

## Acceptance scenarios

- A bank-paid expense explicitly attributed to Station B reduces B's contribution and does not alter Station A's drawer.
- A refund processed at B reduces the original A sale's revenue and COGS while the physical cash effect remains with B's drawer.
- A partial reimbursement returned at B reduces the expense's original cost center and increases only B's receiving location/current shift.
- Historical expenses and pre-station sales remain visible in unassigned buckets.
- Station renames do not change saved attribution names for existing expenses or sales.
- Direct station contributions plus store overhead and legacy-unassigned costs reconcile to the store-wide operating result when all COGS is known.
- Missing sale-time costs continue to withhold gross-profit and contribution values.

## Verification status

The migration, expense form/history/CSV, store-versus-station report, P&L export, and audited cost-center reclassification are implemented. Rust regression coverage includes a station sale/refund, a legacy unassigned sale, direct/overhead/legacy costs, current report attribution after correction, append-only history, identical retries, conflicting retry rejection, stale-revision rejection, invalid station rejection, and unchanged source expense/reimbursement rows. The latest full run passed 77 Rust library tests; the extra P&L assertion and final desktop binary check are being rerun. Command-boundary authorization and live authenticated UI review remain open. See [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md) for the single continuation checklist.

