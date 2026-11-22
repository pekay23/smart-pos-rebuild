# Advanced POS implementation roadmap

> Continuation source of truth: [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md). Use this roadmap for implementation history and detailed domain decisions.

## Completion rule

Call a feature complete only when its screen, permission checks, persisted behavior, recovery path, and relevant acceptance checks all work together. Keep a plan item open when a build passes but the user workflow has not been exercised. Record provider certification, hosted credentials, and hardware results as external gates rather than claiming them from local code.

## Delivery order

1. **Local reporting and finance.** Remove the 250-sale limit from reports and history. Add period-correct reporting, drawer sessions and cash movements, X snapshots, idempotent Z close, linked refunds and voids, sale cost/tax/discount snapshots, complete receipt reprints, backup and restore, and audit review. Keep attendance periods separate from drawer sessions.
2. **Tenant payment setup.** Add country, currency, provider, bank/acquirer, terminal model and intended capability discovery. Keep manual methods explicit. Save unsupported integration requests in a durable local outbox and allow sanitized export. Show a request as submitted only after an authenticated service acknowledges it.
3. **Shared service boundary.** Extract checkout, stock, refunds, permissions, and report rules into the Rust domain. Publish a versioned OpenAPI contract. Build a separately deployable Rust HTTP API with PostgreSQL adapters and tenant-scoped authorization.
4. **Cloud business operations.** Add secure identity, branch and station ownership, idempotent offline sync, backups, audit, work-period reconciliation, subscriptions, signed licences, and entitlement checks.
5. **Operator and integration services.** Build a separate operator portal with issue Kanban, subscription management, provider request intake, safe tenant updates, and audited support access. Implement provider connectors only for documented, approved APIs or terminal protocols after sandbox and certification work.
6. **Web client.** Build management workflows first, then the touch-focused browser register. Deploy the client to Vercel. Keep durable API jobs, payment webhooks, and scheduled work on independently deployable services.
7. **Other clients and devices.** Validate desktop on Windows, macOS, and Linux. Run Android and iOS adapter spikes. Validate HarmonyOS availability and SDK requirements before committing to a release. Publish a tested device, OS, and peripheral matrix.
8. **Release readiness.** Verify migration, backup restore, tenant isolation, sync replay, financial reconciliation, accessibility, update signing, demo isolation, crash recovery, and device behavior before public paid transactions.

The legacy Win32 installer reconstruction in `PLAN.md` remains a separate reference workstream. Its completion does not prove Advanced POS feature completion, and Advanced POS does not require the original product's executable or UI to be copied.

## Completed slice: store identity, tax, and currency safety

The Settings page now edits the business name, branch/store name, address, phone, currency and sales tax. Tax is stored per store so branches can use different rates; migration 0024 moves the legacy business-wide rate into each existing store. Receipts and the signed-in session use updated profile names immediately, and new checkout calculations read the assigned store's rate. Currency remains business-wide and cannot change after products or financial records exist, preventing saved minor-unit prices and ledger amounts from being reinterpreted without an exchange-rate workflow. Profile changes require `settings:write` and create an audit event.

Acceptance checks:

- Legacy tax settings migrate to each existing store without losing the saved rate.
- Store tax is selected by the active store during discount approval and checkout; historical receipts retain their saved tax amounts.
- Invalid currency codes and tax rates are rejected by the backend.
- Currency changes are rejected after a catalog or financial activity exists in the business.
- Profile updates are store/business scoped, audited, and refresh the active session names/currency.
- Frontend build, Rust tests, formatting, migration validation, and desktop executable build pass. Authenticated settings and receipt rendering review remains open.

## Implemented slice: thermal receipt layout settings

Store preferences now include an optional receipt header, a configurable footer, and supported 58 mm/80 mm roll widths. Migration 0025 supplies safe defaults for existing stores. The Settings page can save these choices; register receipts use the selected width for OS print-dialog output and include the header/footer in both the print view and the copy/email/WhatsApp draft. The printer still uses the operating system's print dialog, so direct USB/Bluetooth discovery and model-specific status are not claimed.

Acceptance checks:

- Only 58 mm and 80 mm widths are accepted by the backend; header/footer length limits are enforced.
- Receipt preferences load for cashier sessions and update the print layout and share text.
- Previous settings rows receive the standard footer and 80 mm width during migration.
- Rust suite, frontend production build, lint, and Windows executable build pass. Physical printer and authenticated receipt-preview checks remain open.

## Completed slice: complete local sales queries

### Caller contract

Sales history and reports send one typed query with optional UTC half-open date bounds, a literal search term, an optional register station key, page size, offset, and an optional SQLite row-ID high-water mark. The Rust command derives the store from the active session. It returns one deterministic page, whole-filter totals, tender totals, station identity/name snapshots, and the high-water mark to reuse for later pages.

Capture the per-store high-water mark and calculate page rows and aggregates in one SQLite read transaction with the same validated filter. Order by creation time and sale ID. Convert each displayed local calendar midnight to UTC independently. Export every row matching the active filter, not only the visible page. Future hosted APIs must use a durable ledger revision or cursor instead of SQLite row IDs.

### Acceptance checks

- A store with more than 250 sales can browse every matching sale and report exact whole-period totals.
- Tied timestamps sort deterministically. Date ranges include the first local midnight and exclude the next local midnight.
- Search, date filters, station filtering, aggregates, page rows, and CSV exports use the same store-scoped filter. Sales history filters by station and retains the saved station label in its rows and exports.
- Reports exposes the register filter for sales totals, payment mix, transaction rows, and full CSV export. Station contribution reports include station-direct costs and exclude shared overhead; the unfiltered store P&L includes direct, overhead, and legacy unassigned expenses.
- Cashiers without report permissions cannot call the reporting command. A second store's records never appear.
- CSV export covers all matching pages and protects spreadsheet formula-leading text.
- Invalid bounds fail with a clear error. A new checkout during page navigation does not move rows across the captured high-water mark.

## Completed slice: register cash control

The desktop register now opens one cash drawer per store/station and records an opening float, append-only paid-in/paid-out/cash-drop movements, cashier identity, notes, and timestamps. Cash sales require an open drawer and link to its shift inside the same checkout transaction. X is a read-only audited snapshot. Z records counted cash, expected cash, difference, and the full report snapshot atomically with closing the shift; retries return the saved result. Closed Z reports are browsable from the register (managers see the store's history; other staff see their own). Attendance auto-end remains independent and never balances or closes a drawer.

Acceptance checks:

- Cashier cannot submit a cash sale without an open drawer; successful cash sales are linked to the current store/station shift.
- Expected cash is opening float + linked cash payments + paid-ins − paid-outs − cash drops. Non-cash sales and another store's cash sales are excluded.
- Paid-ins, paid-outs, and drops are append-only, require a positive amount and reason, and are audit logged with the acting account.
- X does not close the shift. Z records counted amount and difference, closes once, and repeated close requests return the original saved report.
- A scheduled attendance end does not close or reconcile a drawer.
- Per-store station creation, display-name editing, and current-device assignment are implemented. Checkout, refunds, drawer sessions, and Z reports now carry the assigned station; historical drawer rows and CSV use the saved station name. This local assignment does not synchronize independent computers or branches; authenticated cloud/LAN coordination remains open.
- Sales history filters transactions by register. Reports filters sales aggregates, payment mix, transaction rows, and station contribution; full sales CSV export uses the same filter, and report rows show the saved station label. Cost center attribution is implemented for new expenses; audited reclassification of historical expenses remains open.
- Live authenticated review of station settings and historical drawer browsing remains open; rerun the native suite and verify offline backup/restore rebinding before calling station management complete.

## Implemented slice: complete closed drawer history and export

Closed drawer history is now paged from the Rust command with a deterministic close-time/ID order and a captured row-ID ceiling. Staff with report access see store history; other register staff see only their own shifts. The UI pages through the full history and exports every matching session to a formula-safe CSV. Closed sessions read their saved Z report snapshot, so later sale changes do not rewrite the recorded close.

Acceptance checks:

- History requests validate page size, offset, and snapshot values at the backend boundary.
- Counts, page rows, store/user scope, and snapshot ceiling share a SQLite read transaction.
- Pages and full-history export share one row-ID ceiling; concurrent new shifts cannot shift or enter the export.
- Ordering is deterministic for matching close timestamps.
- Cashiers can view only their own history; sessions with report access can view the store history.
- Historical closed sessions use the persisted Z snapshot instead of recalculating close totals from later sales state.
- CSV exports all matching history pages and protects spreadsheet formula-leading cells.
- Rust history/snapshot tests, frontend production build, lint, and live authenticated drawer-history review are required before the feature is fully verified.

## Completed slice: sale snapshots and linked refunds

Checkout now stores item cost snapshots plus deterministic line-level discount and tax allocations in the same sale transaction. A one-time backfill assigns legacy sale-level tax and discounts to lines so old sales can be refunded using their original amounts. Managers and administrators can inspect sale details, return selected quantities, issue a full void only for an untouched sale, and record cash or externally confirmed tender refunds. The refund transaction restores stock, records item and tender history, updates the sale state, writes audit data, and creates a cash drawer outflow when cash is paid. A request key makes retry safe. Sales reports subtract refunded subtotal, discount, tax, and tender amounts.

Acceptance checks:

- Line allocations sum exactly to the sale discount and tax, including minor-unit remainder cases.
- Refund preview and backend use the same cumulative per-line allocation, so repeated partial returns reconcile to the original item values.
- Item quantities cannot be returned beyond sold quantity; full sale void requires every untouched line to be returned in full.
- Return stock movements, refund items/tender, sale status, audit event, and cash drawer paid-out entry commit together or roll back together.
- Cash refunds require an open drawer; external tender refunds require a confirmed reference and are presented as manual records.
- Repeated request keys return the prior refund; a key cannot be reused for a different sale.
- Managers and administrators can refund; cashier permissions cannot invoke the refund command.
- Reports net partial and full refunds from original tax/discount snapshots and show refund tenders as negative tender movement.

## Implemented slice: verified local backup and recovery

Settings users with `settings:write` can create and list full local SQLite snapshots and restore a selected snapshot. Each snapshot is written through SQLite's online backup API into a staging file, then checked for SQLite integrity, foreign-key consistency, required Advanced POS tables, and the active business identity before it is made visible. Restore accepts only an app-generated backup ID, checks that the snapshot belongs to the active business, creates and verifies a rollback snapshot first, restores the selected file, validates the result, and recovers the previous database if post-restore validation fails. A successful restore invalidates the in-memory session so the app returns to sign-in. Backup and restore actions are audit recorded.

The Settings panel shows local snapshots, size and time, and asks for an explicit restore confirmation. Local recovery snapshots remain on-device and unencrypted. Settings also offers portable `.ntbackup` export and import: files are encrypted with Argon2id-derived keys and AES-256-GCM, require a 12-character passphrase, and are capped at 256 MiB. Restore decrypts and validates the SQLite image in memory, checks integrity, foreign keys, required tables and business identity, and creates a rollback snapshot before replacing an existing local database. A passphrase-protected file can initialize an empty workspace without an account; restoring into a configured workspace still requires `settings:write` and a matching business. The derived key and Rust-owned serialization/decryption buffers are zeroized when released; OS and SQLite internal allocations are outside that guarantee. Retention defaults to keep all; users with `settings:write` can opt into keeping the latest 10, 25, or 50 regular snapshots. Older verified regular snapshots are pruned only after a new verified snapshot succeeds; restore rollback snapshots are excluded. The business-scoped limit and prune attempts are audit logged. Cloud backup and authenticated desktop workflow review remain open.

Recorded verification: 42 Rust library tests passed at this milestone, including portable encryption roundtrip, wrong-passphrase/tamper rejection, in-memory SQLite validation, empty-workspace restore authorization, and snapshot retention behavior; Rust formatting passed. The Windows desktop executable was built into the isolated `target-review` directory, then removed during the 2026-11-01 generated-output cleanup. Lint previously passed with three existing React effect warnings in Products, People, and Register; the backup panel added no lint warnings. Actual authenticated desktop export/restore workflow review remains open because no usable demo login is available.

Acceptance checks:

- Backup creation uses a transactionally consistent SQLite snapshot and does not expose incomplete staging files.
- Restore is restricted to a valid generated ID, a complete/integral database, and the active business identity; wrong-business and malformed snapshots are rejected before replacing data.
- Every restore creates a verified rollback snapshot first; failed post-restore checks automatically restore that snapshot or report a recovery error clearly.
- Restore invalidates the old in-memory session and records an audit event.
- Only sessions with `settings:write` can list, create, or restore local snapshots or export/import for an existing configured business; an encrypted backup can initialize only a genuinely empty workspace.
- Portable files authenticate both the header and ciphertext; wrong passphrases, tampering, malformed SQLite, wrong business identity, and files over the size limit are rejected before restore.
- Restore validates the imported image and creates a rollback snapshot before replacing the live database; successful restore invalidates the current session.
- Retention defaults to unlimited; only the selected 10/25/50 regular-backup limit triggers cleanup, after a new verified backup is finalized. Restore rollback snapshots are retained and prune attempts are audited.
- Rust backup validation tests, full native library tests, frontend build, and a desktop restore workflow review pass before this slice is marked fully verified.

## Implemented slice: cost-aware profit and loss report

The Reports screen requests a store-scoped period summary for net revenue before tax, net tax, customer total, net cost of goods, recorded expenses, and operating result. Sales contribute revenue and cost on the sale date; refunds reverse revenue, tax, and item cost on the recorded refund date using original cost snapshots. If any sale or return units in the period lack a cost snapshot, gross and operating profit are withheld instead of guessed. Expenses use their recorded occurrence date; this is an operating summary, not a complete accounting ledger or tax filing. Station attribution and regression verification are tracked in the station contribution slice below.

Acceptance checks:

- Date bounds are validated as UTC half-open ranges and every query is scoped to the active store.
- Refunds reduce revenue, tax, and item cost on the refund's recorded date, including returns from sales in earlier periods; returned stock no longer contributes to cost of goods sold.
- Store expenses in the period reduce operating result; another store's expenses and sales are excluded.
- Missing cost snapshots on sale or refund events surface an uncosted quantity and prevent a misleading gross or operating profit value.
- Staff require report permission at the Rust command boundary; client route visibility is not the authorization check.

## Implemented slice: profit and loss CSV export

The Reports page now exports the selected-period P&L summary separately from the detailed sales CSV. It includes the period, sales count, net revenue, tax, customer total, cost of goods, costed and uncosted units, gross profit, operating expenses, and net operating result. Amounts include exact integer minor units and a readable formatted value. Cost of goods is marked incomplete, and profit values stay blank with an explanation whenever cost snapshots are missing. CSV text is formula-safe; the file identifies itself as a management summary rather than a complete accounting ledger or tax filing.

Acceptance checks:

- Export metrics use the selected local-calendar period represented by the loaded report.
- The detailed sales export remains available separately and still walks all matching sale pages.
- Money values retain exact minor units and currency; counts are identified as counts.
- Missing costs never become a zero-cost profit figure; withheld metrics include an explanation.
- Formula-leading text is escaped and the CSV records the report's limits.
- Frontend production build and lint pass; visual interaction review of both report exports remains open until the authenticated Reports screen is reviewed.

## Implemented slice: complete expense browsing and CSV export

The expense register now queries the Rust backend with store scope, literal search, local-calendar period bounds, pagination, and a captured row-ID high-water mark. The period total and matching count come from the same filtered snapshot as the visible page, so older expenses are not silently omitted at 500 rows. CSV export pages through the entire matching snapshot, includes currency, and prefixes formula-leading fields to prevent spreadsheet formula injection. A store/date/order index supports the query. The old capped `get_expenses` API was removed.

Acceptance checks:

- A store with more than 500 expenses can browse past row 500 and totals include every matching expense.
- Pages share one store-scoped high-water mark; expenses created during navigation or export cannot shift the result set.
- Search and period filters have identical predicates for totals, rows, and CSV export; local dates convert to independently computed UTC half-open bounds.
- Other-store rows never appear; `expenses:read` is enforced at the Rust command boundary.
- CSV contains every matching row and escapes quoted, newline, and formula-leading text safely.
- Rust query tests, frontend production build, and native tests pass (29 tests); lint adds no expense-panel warning. The desktop app launches successfully to its development sign-in screen. Live authenticated UI review and opening a generated CSV in a spreadsheet remain outstanding.

Owner-controlled aggregate cash opening and the persistent cash-location ledger are implemented locally. One isolated cash-sale/Z-close workflow has been reviewed live; other role and accounting workflows remain open. Migration 0030 now adds partial/full expense reimbursements with actual-tender journals, cash-location inflows and current-register paid-in entries; the reimbursement UI and X/Z breakdown are implemented. Continue finance work with command-level retry/authorization tests, live UI review, additional source-correction workflows, and cloud backup.

## Implemented slice: actual-tender expense reimbursements

Migration 0030 adds an immutable, store-scoped expense-reimbursement source event and migrates existing migration-0029 records without changing their IDs or references. A manager, administrator, or owner with expense-write access can record a partial or full reimbursement only after confirming the funds arrived, entering the actual receiving tender, optional/required reference, reason and cash destination. One immediate transaction validates the remaining source amount, store, destination and exact open register shift when applicable, then appends the reimbursement, matching cash-location inflow, paid-in movement for the receiving shift, actual-tender contra journal, and audit event. Expense browsing, totals, all-page CSV, P&L and X/Z reports include the signed return on its actual date/shift. The UI describes a manual confirmation and does not claim a provider refund was verified.

Expense report pagination captures separate expense and reimbursement row-ID ceilings because the two SQLite tables have independent row-ID sequences. This keeps page navigation and export stable when a correction is added while a report is open. A regression check confirms migration preserves the cash-count review foreign key and records no `PRAGMA foreign_key_check` violations.

Acceptance evidence:

- Same-store, role, remaining amount, actual tender, destination, immutable-source and idempotency constraints are enforced in the command/database.
- The actual-tender reimbursement journal balances; cash safe receipts update location balance and register receipts are linked to the exact current shift.
- Search, report snapshots, CSV and period P&L include signed reimbursements without editing the source expense; X/Z show reimbursement paid-in as a subtotal.
- Migration coverage passes for preserving legacy IDs/references and review foreign keys, rejecting over-return, accepting a safe receipt, and a clean foreign-key check. Expense-report query tests verify signed rows and stable snapshots.
- All 74 Rust library tests pass, `cargo fmt --all -- --check` passes, and `cargo check --bin smart-pos-desktop` passes. Coverage exercises current-device station matching, closed-shift rejection without writes, retry replay after the receiving shift closes, and preservation of a saved Z snapshot when a later shift receives a reimbursement. Frontend production build and lint passed previously (lint has the existing three React effect warnings). Live authenticated UI review remains open.
- The frontend build reports the existing single JavaScript bundle slightly over 500 kB.

## Implemented slice: station expense attribution and contribution reporting

Migration 0031 adds immutable expense cost centers: direct to one active station, shared store overhead, or legacy unassigned for historical rows. Cash source and cost center remain independent. Expense entry defaults to overhead, checks station/store scope and staff authority, and shows the saved station name in history and CSV. Reimbursements inherit the original expense center for reporting.

Reports now pass the optional register filter through to the profit query. Refunds reverse the original sale station's revenue and saved COGS. The station result is explicitly a contribution before overhead; it includes only direct station expenses, while showing overhead and legacy expenses separately. Whole-store P&L includes all three categories and exposes legacy unassigned sales as a visible bucket. The P&L CSV uses the same selected scope and explains the limits of a contribution report.

- Rust regression covers station sale/refund, assigned and unassigned sales, direct/overhead/legacy costs, and station/store result calculations.
- Frontend production build, all 75 Rust library tests, `cargo fmt --all -- --check`, and `cargo check --bin smart-pos-desktop` pass. Lint retains three pre-existing React effect warnings. Live authenticated UI review and command-level authorization tests remain open.
- Audited reclassification of historical expense cost centers and authenticated UI review remain open.

## Implemented slice: complete purchase receiving history

Purchase history no longer stops at the newest 250 deliveries or filters only the currently loaded browser rows. A Rust query derives store scope from the active session and returns date/search filtered pages, matching count and purchase total from one SQLite read transaction. It captures a per-store purchase row-ID ceiling for stable page navigation and complete CSV export. Search covers supplier reference/name, received item names, and staff member. The CSV includes currency and escapes formula-leading text.

Acceptance checks:

- A store with more than 250 deliveries can browse every matching row and the summary total covers the full filter.
- Search, local-calendar date bounds, count, total and rows use the same store-scoped predicates.
- Snapshot paging and full export exclude deliveries received after the first page.
- Supplier, reference, product, staff and another store's purchase data do not leak across scope.
- CSV export walks every matching page and protects spreadsheet formula-leading fields.
- Rust purchase query tests pass (37 native library tests total), frontend production build and lint pass. Authenticated desktop review and opening the CSV in a spreadsheet remain outstanding.

## Completed slice: local and hosted payment integration request capture

Store admins can save a local country/provider/terminal/capability request and copy structured JSON without store or payment-account identifiers. Hosted branch managers can record the same request through the authenticated API. Migration 0026 uses forced branch RLS, least-column runtime grants, idempotent submission and a transactional audit event; the web settings panel lists status for managers with `payments:write`. Server validation normalizes country codes and rejects common credential/payment-account markers in details. Requests remain `submitted` until operator workflow is implemented. The hosted form explicitly says that the developer inbox and notifications are not connected; it must not imply that a provider is integrated or that a developer was notified.

Verification: 58 API unit tests passed (one PostgreSQL-only test ignored), the restricted PostgreSQL verifier applied all 26 migrations twice and exercised request creation, exact replay, changed-payload conflict, credential-marker rejection, status listing and cashier denial over HTTP. The web production build and OpenAPI YAML parsing pass. Operator inbox/Kanban, customer-visible progress updates, diagnostics and provider connectors remain open.

## Isolated desktop workflow verification (2026-10-30)

The review executable was built with a temporary unique Tauri application identifier and then the source configuration was restored. This gave the test process a separate Windows app-data database, leaving the user's original Advanced POS profile out of the sale/close test. Fresh demo seeding succeeded after fixing its sample mobile-money expense to use the current `not_applicable` cash-origin value and after adding explicit zero opening events for the seeded Main register and Main safe.

In the isolated profile, a counted zero-float drawer was opened, one Pine timber was sold for GH₵65 cash, inventory decreased from 30 to 29, and the Z close counted GH₵65 with GH₵0 variance. The close swept GH₵65 to Main safe and retained GH₵0 in the till. This verifies this single local cash path; it does not establish full product parity, remote payment execution, printer hardware behavior, cloud synchronization, or all role/platform flows. The order ticket was visually inspected in a 1362×892 desktop window; tablet and phone-sized visual checks remain open.

The earlier first attempt used only an `APPDATA` override, which Windows Known Folder APIs did not honor. That attempt reached the regular local profile and logged in as the seeded demo administrator, but no sale or drawer opening was recorded. The review run was restarted under the unique application identifier before the verified sale and Z close. The work-activity timestamp may have been touched by that login; no financial totals were changed in that attempt.

Historical verification evidence: `cargo fmt` and all 64 Rust library tests passed; the isolated Windows executable built successfully (existing linker PDB warnings only). That executable was removed during the 2026-11-01 generated-output cleanup. This build was a runnable local product checkpoint, not a complete app rebuild or 1:1 match.

## Implemented slice: manager-approved ticket discounts

Cashiers can request a discount with a reason. An active same-business approver with `sales:discount:approve` reviews the saved ticket quote and authenticates with a personal PIN or password without ending the cashier session. The approval record binds to ticket, requester, store, discount, reason, customer, lines, prices, currency, and tax; it expires after 15 minutes, is invalidated when the financial quote changes, and is atomically consumed with checkout. Failed credential attempts are rate-limited. The cashier's payable amount and customer-facing display exclude discounts until approval is valid.

Historical verification for this slice: the desktop frontend production build passed, lint passed with the existing React effect warnings, Rust formatting passed, the desktop Rust suite passed (35 tests), and migrations 0018-0019 applied in an in-memory SQLite smoke check. The Windows executable artifacts were removed during the 2026-11-01 generated-output cleanup. Authenticated UI review remains open.

Acceptance checks:

- No nonzero discount reaches checkout through a direct un-ticketed request or without a valid approval record.
- A forged, cross-store, replayed, expired, consumed, or quote-mismatched approval is rejected; completed-sale idempotency still returns its prior receipt.
- Deactivated or demoted approvers cannot authorize checkout; changing products, quantity, price, customer, tax, amount, or reason requires approval again.
- Wrong credentials are rate-limited and neither the cashier session nor staff attendance session is switched during approval.
- Cashier and customer display totals match the approved price; pending or expired discounts are labeled and excluded.
- Native Rust build/tests and authenticated checkout/held-ticket/retry visual review pass before marking this slice fully verified.

Acceptance checks:

- Requests are scoped to the active store and require `settings:write` at the backend command boundary.
- Country, provider, terminal model, intended capability, and bounded detail text are validated; no credentials or cardholder data are requested.
- Saved requests survive restart and are exportable without user, store, or payment-account identifiers; free-text detail is clearly disclosed for human review.
- A future API may change the status to queued only after a durable outbox write and to acknowledged only after a server acknowledgement.
- The operator inbox groups duplicate provider/country/model reports and links their progress to connector work.

The cashier order ticket is positioned independently of page scrolling in desktop/tablet cashier mode. Mobile keeps a dedicated Products/Current sale panel switch. Actual responsive visual review at desktop, tablet, and phone sizes remains an acceptance item.

## Implemented slice: store-scoped activity log

Managers and administrators with `reports:read` can search, page, and export a store's audit event metadata. The query is bound to the active business and store, uses one SQLite read transaction for the row count and page, and captures a row-ID ceiling so new events do not enter later pages or exports. Search is bounded and covers action, entity type/ID, and actor name/username. Raw `details_json` is never returned to the client because it can contain operational notes or other sensitive values. CSV export pages through the full matching snapshot and escapes spreadsheet formula-leading cells.

Acceptance checks:

- The Rust command requires `reports:read`; route visibility alone is not authorization.
- Query parameters validate page size, offset, and snapshot ceiling; each query is limited to the session business and store.
- New events are excluded from subsequent pages and exports after the first page establishes a snapshot.
- The client receives safe metadata only; raw event detail JSON remains in the local database.
- Rust tests cover store isolation, actor search, snapshot stability, and invalid page size (31 native library tests pass); desktop build and frontend lint pass. Live authenticated UI review remains open before marking the feature fully verified.

Owner-controlled aggregate cash opening and persistent cash-location ledger are implemented locally. Next finance slices: source-scoped corrections/reversals and cloud backup. Authenticated review remains open for reports, audit activity, drawer history, and settings backup/restore.

## Implemented slice: supplier settlement posting coverage

Trial balance coverage now includes supplier invoices and supplier payments in the selected store and accounting period. Invoice source counts use the same recorded timestamp as their ledger journal; payment counts use the recorded settlement timestamp. If a source lacks its corresponding ledger journal, the accounting page includes it in the unposted count and explicitly identifies the source type. This prevents a balanced-looking trial balance from hiding supplier settlement rows that were never posted.

Acceptance checks:

- Supplier bills and payments are counted by the same timestamps used for their journal effective dates.
- Posted source counts reduce the unposted count; unrelated businesses and stores remain excluded by the active session scope.
- Frontend coverage totals and warnings include supplier bills/payments, not just sales, refunds, expenses, and receipts.
- Rust suite, frontend production build, lint, and formatting pass. This does not yet add cash-drawer-to-ledger postings or source correction workflows.

## Finance council decision: source-aware correction workflows

Do not expose a generic journal-only reversal. It could erase a transaction from the ledger while leaving sales, inventory, supplier allocations, cash drawer totals, and operational reports unchanged. Preserve the original source and posted journal. For each supported source, a correction must append its event, compensate operational effects, write a linked contra journal, record an actor/reason/audit trail, and commit those writes atomically. Restrict high-risk corrections to owner/admin permission initially; require a reason and idempotency key.

Source boundaries: use the existing refund/void workflow for sale returns; receipts require an inventory return or stock adjustment; supplier bill changes require supplier credits and allocation changes; supplier payment changes require evidence that the external payment was returned; expenses need an appended correction state included by every expense report and export. Accounting classification errors use a separately authorized adjustment with evidence. Do not implement the generic reversal endpoint until the relevant source-specific workflow exists. Reconcile each source against journals, tenders, inventory, AP, and drawer records, and surface unexplained differences rather than silently adjusting them.

Migration 0026 records source origin and links till-paid expenses and supplier payments to one paid-out movement. Migration 0028 adds persistent physical cash locations and immutable location events; cash-origin rows now link to a tracked register, safe, or petty cash location. X/Z snapshots cover cash flows, and Z close records counted variance, cash swept to a safe, and retained till float. Historical unknown cash remains unassigned.

## Implemented slice: transactional local accounting journal

The local database now has a business-scoped chart of accounts and immutable, balanced double-entry journals. Checkout, refunds, expense entry, and new supplier receipts write their journal lines in the same SQLite transaction as the operational source event. Existing sales, refunds, and expenses are backfilled idempotently at startup. Sales record saved tender, gross revenue, discounts, tax in a clearly unclassified liability, and cost of goods only when the sale-time cost snapshot exists. Refunds reverse the saved line amounts and known cost; expenses use the recorded category and tender. Supplier receipts debit recorded inventory cost and credit Goods Received Not Invoiced (GRNI). New supplier invoices can be allocated across one or more receipts, reclassifying GRNI to Accounts Payable; manually recorded supplier payments can be allocated across invoices and reduce Accounts Payable. Historical purchases remain unposted because their settlement status is unknown. Missing cost snapshots, unknown tender, and any tax amount mark coverage partial rather than guessing. The manager report shows a date-bounded trial-balance movement and safe CSV export.

This is a management ledger slice, not a full accounting system. New supplier receipts post completely as balanced accrual journals against the GRNI liability until an invoice is matched; historic purchases remain unposted because their payment status is unknown. Supplier invoices are exact allocations to received stock, and settlement records an external payment without initiating or verifying it. Physical cash-location balances and transfers, period close/reversal UI, and certified tax invoices are not represented. In Ghana, taxable persons must issue a tax invoice using a certified invoicing system integrated with the Commissioner-General's system; Advanced POS does not yet provide that certification or filing workflow.

## Implemented slice: owner-controlled cash opening balance

Each store can record one immutable, owner-only opening balance for aggregate cash-on-hand account 1000. The entry posts against opening-balance equity, includes a cutoff timestamp and reason, is idempotent, and writes its audit event in the same transaction. It does not create or modify a drawer shift and must not be used to add an opening float already represented by earlier activity. Migration 0028 now adds the separate physical cash-location subledger, paired transfers, source-linked outflows, shift/Z variance handling, close sweeps, and retained float. Location baselines do not create another GL asset.

Acceptance checks:

- Only the authenticated business owner can read or create the store's one-time opening entry.
- A positive amount posts exactly one balanced adjustment journal (debit account 1000, credit opening balance equity) with a stable source identity.
- The cutoff is rejected when cash-on-hand journal activity already exists on or before the date, preventing an opening amount from duplicating prior cash transactions.
- Replaying the same key and payload returns the saved record; a changed payload or second opening entry is rejected.
- The source record, journal, lines, and audit event commit or roll back together; the saved entry cannot be updated or deleted.
- No historical cash source is inferred, and cash-shift opening floats or Z reports are unchanged.
- Rust migration/ledger tests and frontend build pass before marking this slice verified.

## Implemented slice: tracked till, safe, and petty-cash ledger

Migration 0028 adds immutable, store-scoped cash-location events and paired transfers for register tills, safes, petty-cash boxes, and other cash. Owners/managers can create locations, record each physical opening count once, inspect tracked cash against GL account 1000, transfer cash between locations, and review pending shift-opening or Z-close count differences. Register open compares its counted float to the tracked till balance without creating another balance. Cash sales, cash refunds, drawer-paid expenses, outside-register cash expenses, supplier cash payments, paid-in movements, cash drops, manual drawer outflows, and Z-close variance/sweep/retained amounts are saved atomically with their economic or shift source. Legacy records remain unassigned.

Acceptance checks:

- A store cannot transfer across business/store/currency boundaries or from a location without an opening count; event/transfer histories are immutable.
- A cash sale/refund/expense/supplier settlement and its location event commit together; drawer-paid sources cannot be counted twice.
- Opening a shift never adds cash; a mismatch is retained as a pending variance. Z records expected, counted, difference, safe sweep, and retained cash separately.
- A Z-close cash sweep updates the till and destination safe atomically; the retained till balance carries forward without duplicate float.
- Variance acknowledgement is a separate immutable review record; it never rewrites the count or saved Z report.
- Migration 0028 fixture test, all 64 Rust library tests, `cargo check`, and frontend production build pass. Authenticated live settings, sale/refund, and Z-close screen walkthrough remains outstanding.

Acceptance checks:

- Each posted entry balances before the source transaction can commit, and source plus journal commit atomically.
- Journal source keys are unique per business; retries do not duplicate entries. Updates and deletions of posted headers or lines are rejected by SQLite triggers.
- Historical migration is idempotent and keeps unknown historical amounts visibly unposted or partial; it does not infer a supplier settlement.
- New receipts debit inventory and credit GRNI inside the stock-receipt transaction; saved line calculations must reconcile to the receipt total. Zero-value receipts create no empty journal.
- Migration 0021 preserves existing journals and line relationships while extending allowed source types; foreign-key checks remain clear.
- Trial balance derives business/store scope from the active session and reports only the selected store and period.
- Rust accounting migration/posting tests and the full Rust suite pass (53 tests); frontend build, lint, and formatting pass. The purchase journal expectation and supplier migration fixture were corrected when the full suite exposed stale assertions. Authenticated desktop report review remains open.

## Implemented slice: staff account lifecycle

The administrator Staff profiles panel now lists active and deactivated business accounts and allows deactivation or restoration. These changes require `users:write` in Rust, are transactionally audit logged, prevent deactivating the signed-in account, and preserve at least one active administrator. Inactive accounts stay out of the normal PIN/password profile switcher and cannot sign in. The lifecycle command is idempotent when asked to set the current status again.

Acceptance checks:

- Only `users:write` sessions can list or change account status; queries are restricted to the active business.
- A user cannot deactivate their current account or the business's last active administrator.
- Deactivation/reactivation and its audit event commit together; repeated status updates do not create duplicate audit events.
- Inactive accounts remain visible to administrators for recovery but are excluded from profile switching and sign-in.
- Rust tests cover self/last-admin safeguards, cross-business isolation, inactive-account visibility, and audited reactivation; frontend build/lint and live administrator workflow review are required.

## Implemented slice: protected business owner and custom staff roles

New workspaces now create a protected Owner identity separate from the editable permissions JSON. Existing workspaces are migrated to an owner only when the local audit history identifies the original active creator; an unassigned workspace offers an administrator password-confirmed ownership claim. The owner cannot be deactivated through staff lifecycle controls. The demo administrator signs in with protected owner identity.

Owners can create and revise custom roles from cashier, manager, or administrator starting points, choose a cashier or management workspace, and grant only the catalogued operational permissions. Role writes require owner identity and password reauthentication, use optimistic revisions, and create audit events. Staff creation and existing staff profiles can be assigned business roles; owner assignment is never available in either path. Administrators cannot grant permissions they do not hold, demote the last active administrator, or change their own role.

Acceptance checks:

- The owner identity is stored in `businesses.owner_user_id`; role JSON cannot create or transfer ownership.
- Legacy ownership is recovered only from a trustworthy local creator event; otherwise the database remains unclaimed until the active administrator confirms with their password.
- System templates cannot be edited and custom roles cannot receive owner, role-governance, license, backup, user-administration, or settings privileges.
- Role changes reject stale revisions and invalid or duplicate permission identifiers; account assignment stays within the active business.
- The native Rust suite passes (35 tests); Strawberry Perl was installed to build vendored OpenSSL on Windows. Frontend production build and formatting pass; visual owner, custom-role, and account lifecycle review remains open.

Remaining people-access work: live account lifecycle, role, and discount workflow review.

## Decisions

The product plans now select a Rust HTTP API and OpenAPI contract. This resolves the earlier TypeScript API wording in `PRODUCT_COMMERCIAL_PLATFORM_PLAN.md`. React and TypeScript remain the client stack. Vercel remains the web-client host, not a requirement that durable payment, sync, or scheduling jobs run inside short-lived web handlers.

Payment providers remain manual until an approved connector can prove the charge state. A provider request saved on an offline device remains local or queued until the service acknowledges it. Never put payment credentials in an issue or diagnostic bundle.
