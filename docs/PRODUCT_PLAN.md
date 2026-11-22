# Advanced POS product build plan

> Continuation source of truth: [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md). This document retains product scope and discovery; current completed and remaining implementation items are consolidated there.

## Product goal

Build a complete point-of-sale product for small retailers, beginning with a cross-platform desktop application that works offline. Keep business rules and data access behind stable interfaces so a browser client can later use the same API and hosted PostgreSQL on Vercel/Neon/Supabase. Preserve the old installer reconstruction under the existing `src/` and `resources/` paths as reference material; new product code lives in `apps/` and shared packages in `packages/`.

## Demo discovery evidence

The installed demo is an Electron application at `C:\Program Files\Smart POS\Smart POS.exe`, serving its UI on `localhost:5000`. The visible login page has a username/password flow and an emergency admin reset that requires a separate master-tool code. No working account is available, so authenticated screens have not been browsed and no login guesses or resets will be attempted.

Read-only inspection of the installed package found these backend route modules: AI, alerts, analytics, auth, backups, business, categories, customers, dashboard, expenses, license, notifications, products, reports, roles, sales, setup, SMS, stock, suppliers, users, and shared utilities. Client page bundles expose checkout/search by name, SKU, barcode, or category; cashier view; product CRUD; categories; stock alerts; sales history and CSV import/export; expenses; reports; customers; suppliers; business settings; users/roles; scanner settings; backups; digital menu configuration; mobile POS/history/settings; menu board; license, AI insights, SMS, notifications, and day closure.

The demo database schema also reveals useful domain concepts, while exposing implementation problems the paid edition should avoid: monetary amounts stored as floating-point `REAL`, duplicated stock thresholds, loosely constrained payment statuses, and records tied to a single implicit `business_id`. The new schema will use integer minor-unit money, explicit stock movements, atomic checkout/refund operations, tenant/store IDs, auditable user actions, and versioned migrations.

## Architecture

- **Desktop app:** Tauri 2, React, TypeScript, Vite. Native shell targets Windows first and leaves macOS/Linux build targets open.
- **Local backend:** Rust commands and SQLite in the operating-system app-data directory. No database is stored beside the executable or in the source checkout.
- **Domain:** A shared Rust crate owns money calculations, checkout invariants, stock transitions, and domain types. Desktop persistence and the future HTTP API call into it.
- **Frontend boundary:** Typed application commands keep screens independent of SQLite. A future web adapter will call an HTTP API rather than importing desktop code.
- **Web release:** Add a web frontend and multi-tenant API after the local workflow is sound. Keep the API and SQL migrations provider-neutral so Postgres can run on Neon or Supabase while the frontend deploys to Vercel. Provider choice waits until the online phase, when current free-tier limits and requirements can be compared.
- **Offline behavior:** Checkout and catalog operations remain local-first. Cloud sync later uses an outbox and stable IDs; payment records are never treated as synchronized until the server acknowledges them.

## Domain sketch

```text
Product { id, sku, barcode?, name, categoryId?, priceMinor, costMinor?, stockOnHand, reorderPoint, active }
CartLine { productId, quantity, unitPriceMinorAtSale }
SaleTotals { subtotalMinor, discountMinor, taxMinor, totalMinor }
Payment { method: Cash | Card | MobileMoney | Other, amountMinor, reference? }
SaleState = Completed | PartiallyRefunded | Refunded | Voided
Checkout { storeId, cashierId, lines, payments, discountMinor, taxProfileId? }
```

Checkout is one database transaction: re-read live prices and stock, validate payment totals, insert sale and item snapshots, record payments, decrement stock, append movements/audit events, then commit. A failed step leaves no partial sale.

## Delivery sequence

1. **Scaffold:** Add Tauri/React app, Rust workspace/core crate, SQLite migrations, typed frontend boundary, and build/run scripts.
2. **First usable register:** First-run store setup, empty-state catalog, product/category management, barcode/SKU search, cart, cash/mobile-money/card tender entry, atomic sale, change due, receipt, and sale history.
3. **Inventory and people:** Suppliers, customers, stock receiving/adjustment/returns, low-stock alerts, user accounts, cashier/manager/admin roles, and audit log.
4. **Operations:** Expenses, cash drawer and day closure, refunds/voids, reporting, receipt printers, scanner setup, customer display, backup/restore.
5. **Paid edition:** Feature plan and licensing UI, team controls, multiple stores, cloud backup/sync, online reports, customer ordering/menu board, notifications/SMS/AI integrations. Paid feature access is enforced in both UI and backend.
6. **Platform completion:** Windows packaging, offline/recovery checks, migration and backup verification; then validate macOS/Linux packages using the same domain crate.
7. **Web edition:** Reuse the domain and visual components where appropriate; add auth, tenant isolation, hosted Postgres, API, sync conflict handling, and Vercel deployment after provider selection.

## Current progress

The local app now uses **Advanced POS** with **Powered Solutions** as its working developer identity. The public brand is still provisional pending name/domain/trademark clearance; the app retains its existing Tauri identifier and local preferences to preserve data across the display-name change. See [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md).

- [x] Inspect the old reconstruction and retain it as reference.
- [x] Inspect installed demo login screen and packaged feature/schema inventory without changing its account.
- [x] Choose an offline-first cross-platform architecture and define money, checkout, and tenant invariants.
- [x] Scaffold the new app and shared Rust domain crate.
- [x] Implement first-run store/admin setup, local login, product/category CRUD, stock corrections, cashier cart with product/category/barcode search, cash/mobile-money/card tender recording, atomic sale/stock movement, receipt summary, dashboard metrics, sales history, and CSV export.
- [x] Add customer and supplier directories with create/edit/archive flows, store-level permissions and audit events; allow customer selection at checkout.
- [x] Add an expense register with role-gated entry, category/search/date filters, CSV export, and atomic audit logging.
- [x] Add supplier delivery receiving with transactional stock/cost updates, stock movements, audit logging, and full store-scoped purchase history with search, date filters, stable pagination, aggregate totals, and complete CSV export.
- [x] Add a debug-only sample workspace with admin, manager, and cashier accounts for local workflow review.
- [x] Build and open the native Windows app; visually inspect the first-run setup screen and confirm database bootstrap succeeds.
- [x] Build an optimized Windows executable and NSIS installer, then launch the packaged executable and verify it opens the first-run setup screen.
- [~] Exercise inventory, customer, checkout, stock decrement, and sales history through a configured local store. Isolated review verified demo setup, drawer open, one cash sale, stock decrement, and exact-count Z close with cash swept to Main safe. Inventory editing, customer creation, and sales history browsing remain to verify.
- [~] Continue operations: cost-aware P&L, paged drawer and purchase history, local backup/restore plus encrypted portable backup export/import, receipt print/share drafts, searchable safe-metadata activity review, staff lifecycle, protected business owner and custom role creation, and a local double-entry journal for sales/refunds/expenses, supplier receipts to GRNI, supplier invoice matching, and manually recorded supplier payments are implemented. Supplier invoice/payment posting coverage is shown in the trial-balance report. Migration 28 adds tracked till/safe/petty-cash locations, atomic safe/till transfers, one-time opening counts, source-linked cash sales/refunds/expenses/supplier payments, cash variance events, and Z-close sweep/retained-float fields. Migrations 29–30 now preserve old reimbursement IDs while supporting partial/full returns by actual tender and destination, updating cash-location balances and the receiving open shift's paid-in total, and surfacing returns in expense reports, CSV, P&L, and X/Z reports. Migration, cumulative-limit, cash-safe, foreign-key, report, P&L, build, and Rust-suite checks pass; direct command retry/authorization coverage and live reimbursement workflow review remain open. Complete accounting, cloud backup, and authenticated workflow review remain open. Cash drawer open, paid-in/paid-out/drop ledger entries, X snapshots, counted idempotent Z close, partial linked refunds, full-sale voids, stock restoration, and tender-aware report netting are implemented; external refunds remain manually confirmed by reference.
- [x] Exercise the demo role views in the running desktop app: cashier register-only, manager operations and reports, administrator settings, and shared catalog access for checkout.
- [x] Implement period-based sales reporting and paginated history; query removes the 250-row cap, exports all matching pages, and calculates refund-adjusted totals from the same store-scoped query. Frontend build and native Rust checks pass; desktop visual review remains pending.
- [x] Add a period-scoped profit/loss summary using saved sale-time costs, refund-date reversals, and recorded expenses. Gross and operating profit stay unavailable if any sale or return units in the period lack cost snapshots. Live UI review remains pending.
- [~] Verify the register workflow in a fresh, isolated demo profile: demo workspace setup, Main register/safe initialization, counted drawer opening, cash checkout, stock decrement, and Z close with an exact count and safe sweep all succeeded. The test sale was one Pine timber at GH₵65; close counted GH₵65, variance GH₵0, swept GH₵65 to Main safe, and retained GH₵0. Sales history, customer creation, non-cash tender, printer output, role-specific end-to-end flows, and tablet/mobile layouts were not covered by this run.
- [ ] Implement paid edition services: plans/licensing, multi-store administration, cloud backup/sync, online reports, menu/ordering, notifications/SMS/AI integrations.
- [ ] Continue feature pages using the audited route inventory. The original Smart POS remains inaccessible without its own working account; Advanced POS's seeded local roles are available, but role-by-role workflow review is still incomplete.
- [~] Visually verify the **CURRENT SALE / Order ticket** layout. Desktop cashier mode was inspected at 1362×892: the order ticket stays in its own right column while catalog and ticket content scroll independently; the header keeps the full-screen control out of the ticket workspace. Tablet and phone-sized visual checks remain open; mobile is designed to switch between Products and Current sale panels.
- [~] Capture tenant payment-integration requests locally: country, bank/acquirer or processor, terminal model, desired capability, and extra detail. Requests are store-scoped and can be copied as structured JSON without store/account identifiers; admins must review free-text details before sharing. Status remains local-only until an authenticated support API acknowledges delivery. Provider catalog discovery, currency/capability matching, and onboarding guidance remain open.
- [~] Deliver tenant support to the separate operator inbox: provider-specific requests already have country/provider/model intake and public progress. General branch-scoped support requests and an operator inbox are implemented in migration 0060 with typed links, independent request progress and private operator history; restricted PostgreSQL verification is pending. Attachments, consented contact, multi-operator assignment, notifications, provider diagnostics and subscriptions remain open. Never request secrets, card data, PINs or credentials.
- [ ] Build certified provider and terminal connectors incrementally from explicit public/merchant-approved APIs, ECR protocols or SDKs; document per-country and per-model capabilities, sandbox coverage, certification and live-payment readiness before activation.

The current desktop package is a runnable early product build, not a complete reconstruction or a 1:1 clone. The original product's authenticated pages could not be inspected because no account was available. Card and mobile-money tender currently record the chosen payment type locally; no processor integration is included.

## Role-focused screens and access

The role matrix below separates working screens from reserved permissions and planned owner controls. Verification on 2026-10-29 used the seeded local accounts; it did not submit a financial checkout.

| Role | Main view | Available work | Restricted work |
| --- | --- | --- | --- |
| Cashier | Register-first, optional auto full screen, barcode/search, basket, customer selection, payment and receipt | Sell, adjust cart quantities, view products and select/create checkout customers | Product/catalog editing, stock adjustments, purchases, expenses, reports, sales history, store settings and staff administration |
| Manager | Store operations dashboard | Register, products and inventory, suppliers, purchases, expenses, sales history, reports and store activity log | Store settings, licensing, backups, user/role administration and owner controls |
| Owner / administrator | Operations dashboard with an administrator-only Store controls panel | Manager operations plus device-level cashier/display preferences and zoom; reserved permissions for user access, licensing and backups | Future owner-only critical actions should be audited and may require re-authentication |

The local permission system checks backend commands as well as hiding navigation. New stores have a protected Owner identity, and existing stores recover it only from creator audit history or a password-confirmed owner claim. Administrators have staff and settings access; owner status is recorded separately from role permissions. Cashiers see only the register, start there by default, and can read the products and customers needed for checkout; managers see the operational dashboard and reporting. Owners can create custom roles from built-in templates, select a cashier or management workspace, and grant catalogued operational permissions. Custom roles cannot receive owner, role-governance, license, backup, user-administration, or settings privileges. Staff profiles can be assigned a business role, with backend checks preventing privilege escalation and demotion of the last active administrator.

An administrator-only Store controls panel links to cashier and customer-display settings. Staff creation, PIN/password switching, account deactivation/reactivation, shift schedules, customer quick-add, and first-name receipt display are implemented. Staff lifecycle protections prevent self-deactivation and disabling the last active administrator; owner accounts cannot be deactivated. Custom roles use owner reauthentication, optimistic revisions and audit events, and staff role assignment is business-scoped and audit logged. Inactive accounts disappear from profile switching and sign-in. Ticket discounts now require a reason and a fresh personal credential from an active same-business account with `sales:discount:approve`; approval is bound to the saved ticket's quote, expires after 15 minutes, is invalidated when ticket contents or pricing change, and is consumed atomically with checkout. Cashier and customer display totals exclude a pending or expired discount, and approver attempts are rate-limited. The full desktop Rust suite passes (58 tests); the isolated Windows desktop executable build succeeds, and authenticated visual/workflow review remains open. Managers and administrators with report access can review a paged, searchable, store-scoped activity log and export safe metadata without raw event details. The local audit table supports operational review but is not tamper-proof or a complete security audit system. Settings includes cashier startup/full screen, display zoom, customer display, on-device snapshots with opt-in keep-all/10/25/50 retention, passphrase-encrypted portable backup export/restore (12-character minimum, 256 MiB maximum), and local payment-integration request capture. Business/store profile editing and per-store tax settings are implemented with backend validation, audit logging, and a migration of existing tax values. Thermal receipt width and custom header/footer are implemented; direct printer-driver integration, licensing, cloud backup and authenticated settings workflow review remain open. Sales history and reports use date-filtered pagination and complete CSV export, netting partial refunds and returns. Reports include a cost-aware profit-and-loss summary. Drawer history supports paging and export. Multi-store reports and live authenticated review of register, report, audit, drawer and backup workflows remain open. Backend command checks remain the authority even when a screen is hidden.

The release executable and MSI/NSIS installers were successfully built before the latest administrator-panel change. The updated frontend build passes, but Windows refused to replace the running packaged executable. Rebuild the installers after closing the currently open Advanced POS window.

## Competitor workflow reference

The user supplied a transcript of [Quicklogic POS demo](https://www.youtube.com/watch?v=x3Kli-OIPRk). The transcript describes role-based login; keyboard-assisted checkout, product sorting, and payment methods; sales drafts and quotations; order history and returns/refunds; customer profiles with credit limits; daily expenses; product price lookup; shift history; X/Z reports; store, currency, language and tax settings; printer and receipt customization; backups/cloud sync; product variants/categories/brands/tags; staff roles; and sales, expense, and profit-and-loss reports.

Treat those points as workflow examples, not proof of the reference product's reliability or security. Advanced POS will implement them behind role permissions and transaction/audit rules. Credit limits require a receivables ledger and collection workflow before enabling credit sales. A Z report must close a register day idempotently and lock/reconcile its totals; an X report is a read-only in-shift snapshot.

## Shift and end-of-day status

Checkout saves each sale immediately with its timestamp, stock movement, and station identity snapshot. Daily dashboard totals and reports summarize saved sales by date; this is not a register close. Owners/admins can add and name local register stations and assign the current device. Cashiers explicitly open a drawer with a counted float, record cash movements, inspect an X snapshot, and close with a counted-cash Z report. Cash checkout is blocked until the assigned station drawer is open. Cash refunds append a paid-out movement to the assigned station's current shift. A scheduled attendance end never closes or balances the drawer; day close remains an explicit staff action. Historic closed-shift browsing and export are available; local station records do not sync across computers or branches yet.

## Cross-branch stock discovery and fulfillment

When an item is low or unavailable at checkout, the cashier must search the same company's catalog across branches, see each branch's available-to-promise quantity with freshness timestamp and address/contact, then either direct the customer to purchase there or submit a customer-linked request for a hold or transfer. Requests are tracked through acceptance, allocation, dispatch, partial/full receipt and discrepancy resolution; the cashier can see status and communicate the next step. A request is not a sale or payment.

Inventory belongs to a branch (the current `stores` scope). Multiple register stations within that branch share its quantity and are shown as contact/pickup points, not separate stock locations. Canonical company product identity links branch-specific sellable records; ambiguous duplicate SKUs/barcodes require owner mapping rather than name-based matching. Available-to-promise is on-hand minus active reservations, accepted outbound allocations and safety stock, floored at zero. In-transit units are not sellable at the destination until received. Transfers create audited outbound/inbound movements and never inflate sales, cash, tax or revenue reports.

The current desktop database is local to an installation and cannot guarantee current stock across separate computers or branches. The cashier must see `stale` or `unknown` when there is no authoritative shared response; the app must not promise stock or accept a guaranteed hold offline. Live cross-branch availability, reservation and transfer writes depend on the shared tenant API and durable synchronization described in `.planning/arena/branch-stock-routing/`. Customer pickup at a branch already holding stock is distinct from moving stock between branches; cross-branch payment/return settlement remains a separate policy decision.
