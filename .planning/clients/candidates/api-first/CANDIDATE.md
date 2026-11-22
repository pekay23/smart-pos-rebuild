# Candidate: API-first with platform-specific clients

Status: independent candidate for comparison, not an implementation decision.

## Decision and current boundary

Make a versioned OpenAPI contract and a separately deployed Rust service the authority for shared business state. Web, Android, iOS/iPadOS, and HarmonyOS clients call HTTPS endpoints. Retain Tauri and SQLite on desktop for existing local operations and explicitly bounded offline work. Move business decisions out of Tauri handlers into reusable Rust application services. A desktop offline receipt is provisional until server acceptance; a local commit cannot promise cross-device stock or financial settlement.

Use Kotlin/Compose, Swift/SwiftUI, and ArkTS/ArkUI for mobile UI. Desktop keeps its existing React/Tauri UI and adopts desktop conventions. Web gets a separate responsive React UI. Share the contract and design tokens where useful, not entire screens or hardware assumptions. This changes the existing plan's mobile Tauri spike and costs multiple UI implementations and native release pipelines.

The current code is not a service. `apps/desktop/src/lib/api.ts` invokes Tauri commands and desktop pages use that facade. `apps/desktop/src-tauri/src` combines handlers, SQLite state, sessions, permissions, and workflows. `crates/pos-core` is small. No HTTP service, web app, or native mobile client exists. Local card/mobile-money tenders record a choice; they do not establish processor settlement.

## Caller usage

These proposed names illustrate the same use case at each boundary; they do not claim the API exists.

```text
Web catalog screen -> CatalogRepository.listProducts(cursor)
  -> generated client GET /api/v1/catalog/products
  -> HTTP session/scope adapter -> catalog service -> Postgres repository

Desktop register -> CheckoutRepository.submit(command)
  -> Tauri checkout_sale adapter -> checkout service
  -> SQLite transaction commits provisional sale and outbox entry
  -> sync worker POST /api/v1/sync/operations with stable operation ID
  -> server transaction accepts or rejects -> UI shows acknowledged or needs review

Android register -> Compose view model -> typed POST /api/v1/sales
  -> server checkout service -> Postgres transaction -> receipt or typed error
```

Mobile starts online-only. Desktop offline mode is opt-in per store. If an online request times out, query its operation status using the original key before retrying; do not silently switch to a new offline transaction. Each UI owns loading, permission denial, retry, and reconciliation states. Client repositories are use-case interfaces, not a way to import Tauri into shared UI.

## Domain and transport types

`RequestContext` is server-created from a verified session: user, authorized business/store, permissions, device/station, and correlation ID. Store selection is a requested scope that the server verifies against membership. A path or body ID is never proof of access. Local execution uses separately verified local context and records device and authorization lease.

Domain types include `Money { minor: i64, currency }`, validated quantity, product/sale/store/station IDs, revision, and UTC timestamps. Preserve sale-time price, discount, tax and cost snapshots. The server recomputes authoritative totals. Reject unsafe ranges or encode large integers as decimal strings where JavaScript cannot represent them safely. Contract fixtures must prove serialization and rounding across languages.

Version transport DTOs separately from domain structs. Proposed examples:

```text
CheckoutRequest { clientOperationId, stationId, expectedWorkspaceRevision,
                  lines, tenders, discountApprovalId }
SaleReceipt { saleId, receiptNumber, totals, status, acceptedAt }
SyncOperation { operationId, deviceId, sequence, baseRevision, kind, payload }
SyncResult { operationId, accepted | rejected | needsReview, serverRevision, reason }
```

Checkout verifies stock and approval and recalculates totals. A submitted total is at most an expectation. Use stable error codes, safe messages, correlation IDs and optional validation/conflict detail. Distinguish unauthenticated, forbidden, invalid input, stale revision, idempotency mismatch, transient failure and reconciliation required. Never return raw SQL or secrets. Use opaque store-scoped cursor/snapshot tokens; preserve the existing stable reporting behavior without exposing SQLite row IDs as a cross-database contract.

Check in OpenAPI v1 as the reviewable wire contract. Generate TypeScript, Kotlin and Swift clients where output is supported; use a tested typed ArkTS adapter against contract fixtures if generation is unsuitable. CI checks breaking schema changes, fixtures and actual service conformance. Additive fields must not break older clients; breaking semantics need a new version and deprecation window.

## Endpoint and module map

Paths are proposed workflows, not claims of remote implementation.

| API v1 group | Current desktop source to extract or assess | Service concerns |
| --- | --- | --- |
| `/auth`, `/sessions`, `/stores` | `commands.rs`, `roles.rs`, `staff.rs`, `passkeys.rs` | Separate cloud and local sessions; approval reauthentication must not replace the cashier session. |
| `/catalog/products`, `/categories`, `/media`, `/inventory/movements` | `commands.rs`, `media.rs` | Scope lookups and stock movements; authorize and limit uploads. |
| `/register/workspace`, `/discount-approvals`, `/sales`, `/sales/{id}/refunds` | `commands.rs`, `discounts.rs`, `sales.rs`, `refunds.rs` | Enforce workspace revisions, approval expiry and one-use; atomically commit sale, tender, stock, audit and idempotency result. |
| `/drawers`, `/cash-locations`, `/accounting`, `/reports`, `/audit-events` | `cash_drawer.rs`, `cash_locations.rs`, `accounting.rs`, `sales.rs`, `audit.rs` | Separate read/write permissions; withhold profit when cost information is incomplete. |
| `/customers`, `/suppliers`, `/purchases`, `/supplier-bills`, `/expenses` | `commands.rs`, `purchase_reports.rs`, `supplier_billing.rs`, `expense_reports.rs`, `expense_attribution.rs`, `expense_corrections.rs` | Transactional and idempotent receiving and payment mutations. |
| `/staff`, `/roles`, `/attendance`, `/stations`, `/settings` | `staff.rs`, `roles.rs`, `attendance.rs`, `stations.rs`, `store_settings.rs` | Validate role changes, membership, last-admin safeguards and station assignment. |
| `/payment-options`, `/payment-requests` | `payment_options.rs`, `payment_requests.rs` | Preserve local-only disclosure until delivery exists. Processor integration and webhooks belong on the server. |
| `/sync/changes`, `/sync/operations`, `/sync/operations/{id}`, `/health` | New service and sync modules | Tenant-scoped cursor, tombstones, retention/reset, replay and status lookup. Health exposes no tenant data. |

`backups.rs`, local file dialogs, window management, printing and hardware integration remain desktop adapters. Server backup/restore is separate. Export/import needs verified identity mapping, duplicate detection and reconciliation; sign-in must never overwrite local data.

## Extraction and service layout

1. Grow `crates/pos-core` or create domain and application crates for validated types, pure calculations, use cases, policies and repository/transaction interfaces. Avoid a rename merely for aesthetics.
2. Extract one vertical slice from `commands.rs` and related modules at a time. Tauri handlers translate inputs, obtain local context and call application services. Move permission checks, stock changes, sale/tender creation, discount approval and audit invariants with the use case. Remove `tauri::State`, window/dialog APIs and mutable global session dependencies from reusable services. Inject context, clock, IDs and persistence.
3. Build SQLite and Postgres adapters with explicit transactions and migrations. Existing `rusqlite` SQL is not portable by assumption: rewrite queries, snapshot pagination and locking for Postgres. Prove concurrency, atomic writes and parity using fixtures. A server process must not share desktop `AppState` or its single-user session model across requests.
4. Add `apps/api` for request validation, token verification, scope derivation, rate limits, DTO mapping and service calls. Keep secrets, payment integrations and durable jobs in server infrastructure. Select hosting after testing pooling, execution limits and background work.
5. Narrow `posApi` into a desktop adapter behind interfaces as use cases migrate. Replacing `invoke` with `fetch` alone does not make existing pages safe or responsive.

## Authentication, tenancy and offline invariants

- Use HTTPS, short-lived sessions, refresh rotation and revocation. Browser sessions need secure cookies and CSRF protection; native clients use OS credential storage and an appropriate system authorization flow. Never ship service credentials in clients. Rate-limit login and approval attempts; exclude secrets from logs.
- The server derives the actor from the verified session, checks store membership and permission on every read/write, and scopes queries and constraints to that store. Add database isolation policies as defense in depth where supported. Test object-ID substitution and cross-store joins, reports and media. Client controls do not enforce authorization.
- Scope idempotency keys to store and operation, bind each to a canonical request hash, and save the result in the same transaction as the mutation. Same-payload retry returns the original outcome; a changed payload is rejected. Retain keys longer than the maximum offline interval and provide status lookup. Atomic checkout includes stock, tender record, financial entries, audit and idempotency result as applicable.
- Initially permit offline desktop cash checkout only for an enrolled station with cached catalog and a time-limited local authorization lease. Define lease duration, enrollment/revocation, encryption and recovery before enabling it. If the lease expires, storage is full, the clock is untrustworthy or outbox persistence fails, stop offline checkout. Never cache raw card data or claim offline processor settlement. Refunds, approvals, role changes, stock adjustments, transfers and day closing remain online-only until each has an approved conflict and accounting policy. Web and mobile initially offer age-labelled cached reads and local drafts, not offline sales.
- Use unique operation IDs and monotonically ordered device sequences. Persist each local mutation and outbox record in one SQLite transaction. Authenticate the device, retry with backoff, retain durable results and stop dependent operations when a predecessor fails. Pull tenant-scoped changes and tombstones. On cursor expiry, obtain an authorized snapshot without losing unsent operations. Provide an audit trail and operator review queue; never silently discard or overwrite a rejected sale.
- Offline devices can sell the same stock or use stale prices. Server policy must either reject with a reason or accept with a visible discrepancy for reconciliation. Receipt numbers must be unique per station or assigned upon acknowledgement; provisional receipts must be identifiable. A lost response is resolved via operation status, not a second sale. Provide a procedure for rejected cash sales after goods have left, including till, stock and ledger reconciliation. Acceptance is not proof that a provider collected payment.

## Platform packages and delivery gates

| Package | UI and device boundary | Initial gate |
| --- | --- | --- |
| `apps/web` | React/TypeScript management and touch register; browser capabilities only | Tenant/permission tests, keyboard and touch, storage recovery, clear pending states. |
| `apps/desktop` | Existing React/Tauri with OS-specific scanner, print, drawer and display adapters | Windows, macOS and Linux builds and upgrade/migration checks; physical devices and SQLite outbox recovery. |
| `apps/android` | Kotlin/Compose, adaptive phone/tablet navigation, Android device SDK adapters | Emulator and representative devices; secure storage, offline restrictions and SDK certification. |
| `apps/ios` | Swift/SwiftUI for iPhone and iPad, accessibility and accessory APIs | Mac/Xcode signing, simulator and real devices; supported accessory and payment tests. |
| `apps/harmony` | ArkTS/ArkUI, HarmonyOS NEXT navigation and native device adapters | DevEco SDK and physical devices; validate networking, secure storage, distribution and SDKs before parity estimates. |

A capability manifest reports available permissions and tested hardware profiles. Unsupported printer, scanner, payment or second-display behavior must be visible and recoverable. Do not claim an OS or device before its build, signing, hardware and accessibility checks pass.

## Rollout and gates

1. Establish local transaction, authorization and migration tests. Inventory Tauri dependencies and existing stores requiring migration.
2. Specify OpenAPI and extract a read-only catalog slice. Test Tauri and HTTP adapters against shared fixtures. Deliver a web screen with server tenant and authorization tests.
3. Extract online checkout with Postgres transactions, concurrent request tests, idempotency, approvals and failure injection. Prove receipt and report parity. Ship online-only before replication.
4. Test migration and desktop sync using a copy of an existing store. Exercise interrupted import, duplicate delivery, lost acknowledgement, expired cursor, revoked device, conflicts and reconciliation. Enable bounded offline checkout only after accounting and recovery procedures are accepted.
5. Build native slices against the same contract, beginning with catalog and online checkout. Validate UI and real hardware on each OS. Expand only when the operation's authorization and device policy are ready.
6. Drill server backup/restore, observability and retention. Track supported client versions and API compatibility. Roll out store by store with rollback and reconciliation plans; binary rollback must not discard server-acknowledged data or local queued operations.

## Alternatives and consequences

- One Tauri/React application everywhere reuses UI but fails the requested platform-custom mobile UI and assumes unsupported HarmonyOS NEXT and hardware bridges. Keep Tauri for desktop where it exists.
- An embedded Rust engine on every device provides disconnected autonomy but multiplies packaging and migrations and complicates authorization revocation, stock conflicts and accounting. Desktop retains a deliberately bounded form of this tradeoff.
- Native clients with duplicated business rules will drift on tax, refunds, stock and approvals. Server decisions remain authoritative; client validation provides immediate feedback only.
- HTTP-only everywhere simplifies reconciliation but removes existing offline-first desktop continuity. A bounded local outbox preserves continuity at the cost of provisional state and conflict handling.
- Exposing desktop commands over HTTP or treating SQLite as the cloud service does not resolve per-request identity, tenant scoping, concurrency or durable jobs. Extract use cases and prove a server persistence layer first.

Open decisions before implementation include hosting, database and identity providers, offline lease duration, negative-stock policy, financial treatment of rejected cash sales, device enrollment, and required hardware. These are release gates rather than hidden assumptions.
