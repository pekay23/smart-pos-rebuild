# Advanced POS platform and web service plan

> Continuation source of truth: [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md). This document retains platform architecture and acceptance detail.

## Product boundary

Advanced POS is one retail system with several clients. The clients share a versioned application contract, but each uses native platform services for local storage, scanners, receipt printers, cash drawers, display control, and secure credentials. Checkout, tax, discount, stock, refund, and permission rules have one authoritative implementation and are not copied into every UI.

The current Windows Tauri build is the first client. The accessible Smart POS demo account is still unavailable, so its authenticated screens and exact behavior cannot be treated as a complete specification. Existing route and bundle inventory is recorded in `PRODUCT_PLAN.md` and `PRODUCT_RESEARCH.md`.

## Architecture decision

| Option | Strength | Cost | Decision |
| --- | --- | --- | --- |
| Rust application services + Tauri desktop + platform-native clients + versioned API | Keeps one authoritative service boundary and desktop continuity while allowing native mobile workflows | Requires API extraction and separate native UI/release pipelines | Use Tauri for the current desktop client; build web, Android, Apple and HarmonyOS as first-class platform clients |
| React web client and TypeScript serverless API on Vercel | Fast web delivery and a familiar serverless stack | Duplicates Rust business rules unless a shared protocol/WASM boundary is introduced | Rejected as the system-of-record API; keep TypeScript in the client and publish OpenAPI from the Rust service |
| Supabase direct data access from every client | Fast prototype, hosted Postgres, Auth and Realtime in one service | Requires careful RLS, grants, offline sync and provider-specific coupling | Use only for features whose access model is proven by migrations and tenant-isolation review |

**Selected direction:** React/TypeScript clients consume the same OpenAPI contract. Rust owns domain invariants and local storage today. Start the cloud API as a separately deployable Rust HTTP service, then deploy it on Vercel's supported Rust Functions runtime or a Rust container host after a latency, execution-duration, background-job, and connection-pooling spike. Vercel can also host the browser app. Keep provider-neutral Postgres migrations; choose Neon or Supabase only after the sync and auth design is testable. Vercel documents official Rust and Node.js function runtimes; Neon documents pooled connections for serverless clients; Supabase requires explicit grants and row-level policies for exposed data.

### Platform-specific client decision (30 October 2026)

Build distinct experiences for each platform rather than placing one generic interface inside every platform shell. Reuse the OpenAPI contract, domain vocabulary, accessibility requirements, design tokens, icons and contract fixtures; each client owns its navigation, layout, gestures, system integration and hardware adapters.

| Target | Product UI and implementation | Platform details to honor |
| --- | --- | --- |
| Windows desktop | Keep the current React/Tauri client for cashier and back office while it is the supported Windows client. | Windows title/menu conventions, WebView2, system print, secure credential storage, keyboard/scanner input, touch, customer-display windows, installer/update and Windows security boundaries. |
| macOS desktop | Initially ship the desktop client with macOS-specific Tauri shell behavior; retain a native SwiftUI/AppKit shell as a separately gated option if POS hardware or HIG review requires it. | macOS menus/shortcuts, title bar and window behavior, Keychain/passkeys, sandbox/signing/notarization, system print, multi-display and supported scanner APIs. |
| Linux desktop | Initially use the desktop client with distro-aware packaging and Linux device adapters; support only validated distributions. | GTK/WebKitGTK integration, desktop portals, permissions, package updates, printer service, keyboard navigation and kiosk/touch behavior. |
| Web | Create a browser-native React/TypeScript app. It is a first-class management and cashier product, not the desktop Tauri shell deployed as a website. | Responsive routes, browser session security, CSRF protection, keyboard/touch behavior, accessibility, tenant switching, safe export/download, explicit sync state and browser-only capability disclosure. |
| Android | Build `apps/android` in Kotlin with Jetpack Compose and Material 3 adaptive layouts. | Phone, tablet, foldable and handheld scanner layouts; system back/navigation; Credential Manager/passkeys; camera and enterprise scanner APIs; secure storage, notifications, NFC/payment/printer SDK integration only after device certification. |
| iOS/iPadOS | Build `apps/ios` in Swift with SwiftUI. Adapt the iPhone and iPad register/management layouts rather than scaling a web view. | Human Interface Guidelines, Dynamic Type, VoiceOver, Face ID/passkeys via AuthenticationServices, camera scanning, secure Keychain, share sheet, iPad keyboard/window behavior, accessory and payment SDK validation, signing and App Store distribution. |
| HarmonyOS NEXT | Build `apps/harmony` in ArkTS with ArkUI as a separate native client. | HarmonyOS navigation/components, device/size adaptation, platform secure credential APIs, camera/scanner integrations, distribution requirements and vendor SDK availability. Use the versioned HTTPS API unless an approved native service bridge is proven. |

The first mobile slices are online-first and focused on the most useful work on that device: Android stock lookup/count/receiving and handheld checkout; iPhone manager approvals, staff/branch oversight and stock count; iPad cashier checkout after payment/printer/scanner support is certified; HarmonyOS inventory and operations before expanding to register hardware. These are the delivery order, not permanent role limitations. As each operation gains safe offline semantics, add it consistently to the approved clients. Never claim a mobile client is a complete till before checkout, payment, printer, shift, offline and recovery paths are implemented and tested on hardware.

The platform-native choice is supported by current platform documentation: Jetpack Compose is Android's recommended native UI toolkit and provides adaptive layouts; SwiftUI is Apple's declarative framework across iOS, iPadOS and macOS; Huawei identifies ArkTS/ArkUI as HarmonyOS app technologies. References: [Android Jetpack Compose](https://developer.android.com/compose), [Apple SwiftUI](https://developer.apple.com/swiftui/), [Huawei HarmonyOS NEXT development](https://developer.huawei.com/consumer/en/harmonyos/develop/), [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Web client and API delivery plan

### Web client

1. Create `apps/web` with React, TypeScript, Vite, and the shared Advanced POS tokens/components. Keep `apps/desktop` as a separate shell; no Tauri imports may enter shared UI packages.
2. Add an HTTP adapter implementing the existing `posApi` use cases. The desktop adapter continues to call Tauri commands; the web adapter calls `/api/v1`.
3. Build responsive management pages first: overview, products/stock, sales, customers/suppliers, purchases, expenses, reports, staff, store settings, and audit history. Build a touch-focused cashier view after device-sized validation.
4. Add a browser offline queue only after idempotency and conflict rules are documented. A queued sale must show `Pending sync` until server acknowledgement; never claim that an unconfirmed sale is synced.
5. Deploy the static/client front end to Vercel. Keep API, background sync workers, webhooks and durable jobs independently deployable so the product is not coupled to one hosting runtime.

### API contract and service

1. Define OpenAPI v1 request/response schemas and stable IDs before moving a screen to the network adapter.
2. Use HTTPS and short-lived access tokens with refresh rotation. Every operation derives tenant, store, and user scope from the verified server session; clients do not submit trusted `business_id` values.
3. Model endpoints around workflows: `/auth`, `/stores`, `/catalog/products`, `/inventory/movements`, `/sales`, `/sales/{id}/refunds`, `/purchases`, `/customers`, `/suppliers`, `/expenses`, `/reports`, `/users`, `/settings`, `/sync/changes`, and `/health`.
4. Require idempotency keys for checkout, refund, purchase receipt, and outbox sync. Keep the database transaction as the boundary for sale, tender, stock movements, audit event, and idempotency result.
5. Add tenant isolation at both the service boundary and database policy layer. If Supabase is selected, every exposed table needs explicit grants and operation-specific RLS policies; tests must prove cross-tenant reads and writes fail. Never ship a service-role key to a client.
6. Keep provider secrets, payment credentials, tax integrations, webhook signing keys, and refund authority on the server. Payment processing is a provider integration; the client must not store raw card details.
7. Add sync cursors, tombstones, conflict policy, exponential retry, dead-letter review, audit visibility, backup/restore, and migration rollback/forward checks before multi-store production sync.

## Client and operating system roadmap

| Client | Implementation | Main role | Readiness gate |
| --- | --- | --- | --- |
| Windows desktop | Tauri 2, React/TypeScript, Rust core, SQLite | Primary cashier and back office | x64 installer upgrade/migration checks; Windows 10/11; touch, keyboard and mouse; WebView2; tested scanner, printer, drawer and customer display profiles |
| macOS desktop | Same Tauri UI and Rust core; macOS-specific device adapters where required | Cashier and management | Build/sign/notarize on macOS; Apple Silicon first, Intel only if demand justifies it; test printer/scanner SDKs and second display |
| Linux desktop | Same Tauri UI and Rust core; distribution-specific packaging | Cost-conscious shops and kiosks | Validate supported Debian/Ubuntu releases and WebKitGTK dependencies; test permissions, USB access, printer services and package updates |
| Web | React/TypeScript client on Vercel; HTTPS API; browser storage for an explicitly limited offline queue | Owner/admin reporting, remote management, and web register where hardware allows | Browser permission model, server-side authorization, tenant-isolation review, sync recovery and keyboard/touch flows |
| Android phone/tablet | Native Kotlin/Compose client in `apps/android`; typed generated OpenAPI client; native scanner, camera, printer, NFC and payment adapters | Handheld sales, stock count, receiving and mobile checkout | Test rugged terminals and at least one phone/tablet; adapt navigation, panes and font scale; validate permissions, secure storage and every required peripheral SDK |
| iOS/iPadOS | Native Swift/SwiftUI client in `apps/ios`; typed OpenAPI client; native secure-auth, share, camera and accessory adapters | Manager access, stocktaking and customer lookup on iPhone; register and checkout on iPad | Xcode signing and App Store packaging; test iPhone plus iPad; use native Dynamic Type, VoiceOver, navigation and accessory paths; iPad second-display support only where the system/device supports it |
| HarmonyOS NEXT | Separate ArkTS/ArkUI client in `apps/harmony` using the same OpenAPI contract and HarmonyOS design components | Mobile inventory and store operations, then register only when terminal hardware support is proven | DevEco SDK/device access; validate local secure storage, camera/scanner, distribution policy, network behavior and printer/payment SDK access before parity claims |

Tauri remains the chosen Windows desktop shell and the initial macOS/Linux desktop option. The customer display feature must use desktop window/monitor capabilities only where supported; mobile screens get native in-app customer views and must not pretend to be an external customer display. Android adaptive layouts must respond to window size and font scale. HarmonyOS NEXT remains a separate ArkTS/ArkUI client rather than a Tauri build.

## POS device and accessibility matrix

| Device class | Interface behavior | Hardware path |
| --- | --- | --- |
| All-in-one and mini PC with touch | Large targets, scan-first catalog, no hover-only actions, optional cashier full screen | HID keyboard wedge first; validated USB/serial adapters for vendor-specific scanner SDKs |
| Standard desktop/laptop and touch monitor | Resizable management layout; F2 checkout; Ctrl/Cmd +/- zoom; configurable cashier full screen | Keyboard, mouse, USB/network printer, drawer kick through a supported printer |
| Tablet / foldable | Reflowed navigation, compact cart/catalog panes, portrait and landscape, touch and stylus | Camera barcode scanning where supported; Bluetooth/USB/network printer only after device profile validation |
| Phone / handheld scanner | Single-task register and scan/count/receive flows; large controls; system font-scale support | Camera/HID/enterprise scanner APIs; supported Bluetooth or network printer adapters |
| Customer-facing monitor | Separate sanitized projection of basket, discount, total, payment success and an admin-authored idle message | Extended desktop monitor; admin chooses target monitor and full-screen preference; cashier window remains independent |
| Receipt/label printer and cash drawer | Printer status and clear recovery; duplicate-safe receipt action | Capability profiles for ESC/POS, OS spooler, USB, serial, Bluetooth, network, and printer-triggered drawer; never claim universal device support |

## Visual test environment strategy

Use Docker Desktop to run local API, PostgreSQL, queue/webhook simulators and reproducible test fixtures. It can also host browser automation for responsive web layouts and browser interaction tests. It cannot visually validate all native operating systems: Docker Desktop runs containers in a Linux VM except for native Windows containers, and containers are not full GUI OS installations. Keep platform tests on native runners or platform simulators: Windows desktop on Windows; macOS and iOS/iPadOS on a Mac with Xcode; Android on Android Studio emulators and representative physical hardware; HarmonyOS in DevEco Studio and target devices; Linux on supported native distributions/VMs. Use real scanners, printers, drawers and customer displays for hardware acceptance.

References: [Docker Desktop](https://docs.docker.com/desktop/), [Docker platform FAQ](https://docs.docker.com/faqs/platform/), [Apple simulated/physical device testing](https://developer.apple.com/documentation/Xcode/running-your-app-on-simulated-or-physical-devices), [Android Emulator](https://developer.android.com/studio/run/emulator), [Android build-managed devices](https://developer.android.com/studio/test/managed-devices), [Huawei DevEco Studio](https://developer.huawei.com/consumer/en/deveco-studio).

### Accessibility and distraction controls

- Provide application zoom from 80% through 160%, plus operating-system/browser zoom shortcuts where supported; preserve layout and allow reset to 100%.
- Respect OS font scaling and high-contrast settings. Keep focus outlines, screen-reader names, keyboard paths, and touch targets at least 44 CSS px in touch mode.
- Admin controls include `cashier opens directly at register`, `cashier starts full screen`, `customer display enabled`, selected display, customer-display full screen, and a short idle greeting.
- Cashier full screen is a preference, not a kiosk lock: F11 and a visible Exit full screen control always restore window mode. Supervisors/admins can change the setting.
- Customer display shows only approved product names, quantities, prices, totals, payment status, and greeting. It does not show staff identity, account balances, private customer details, or management navigation.
- If no second display exists, keep customer display windowed and let the admin move it or choose another screen; do not full-screen over the active cashier display.

## Phases and acceptance

1. **Desktop control slice (current):** admin preferences, cashier auto-register/full screen, zoom, display selection, secondary window, live basket projection; debug workspace and role accounts exercise the flow.
2. **Core retail completeness:** refunds/voids, till open/close, staff management, permissions, reports, printer profiles, backups, audit log, role-based navigation and manager approvals.
3. **Device abstraction:** scanner, receipt printer, cash drawer, label printer, customer screen adapters; hardware compatibility table with model/firmware/tested OS and reconnect/error behavior.
4. **API and Postgres spike:** OpenAPI contract, Rust API prototype, idempotent checkout, tenant enforcement, migration from SQLite export, Neon/Supabase evaluation; no production cloud selection until sync recovery is proved.
5. **Web client:** management first, then responsive web cashier and bounded offline queue; Vercel previews and production with secrets server-side.
6. **macOS and Linux:** build packages from the shared desktop code; certify platform-specific devices and upgrade paths.
7. **Android/iOS:** build the native Kotlin/Compose and Swift/SwiftUI apps from `apps/android` and `apps/ios`; implement secure auth, adaptive screens and certified hardware paths against the shared API; add bounded offline workflows only after sync and accounting recovery pass.
8. **HarmonyOS:** build the native ArkTS/ArkUI app from `apps/harmony` consuming OpenAPI, then validate local secure storage, offline recovery and vendor device SDKs on DevEco and target hardware.
9. **Release readiness:** security and tenant isolation, backup/restore, sync replay, financial reconciliation, accessibility, device matrix and recovery verification across supported clients.

## Current boundaries

The running code is an early local product build, not a complete POS suite or a 1:1 Smart POS replacement. Current card/mobile-money choices record the tender type locally and do not contact a processor. This roadmap makes no claim that untested printers, scanners, payment devices, OS versions, app stores, or hosting free tiers are supported. Provider plans, quotas and policies change and must be rechecked before deployment.

## Current implementation notes

- The cashier's order ticket is pinned and scroll-independent at desktop/tablet widths while basket and payment details scroll inside their own regions. Mobile switches between catalog and ticket panels. Desktop/tablet/mobile visual verification remains open.
- Store admins can save unsupported bank/provider and terminal requests locally and copy a structured request bundle without store or payment-account identifiers. The request is labelled `local_only`; it is not visible to the developer until the cloud support API and operator inbox are deployed and acknowledge delivery. Admins are told to review free-text details before sharing.
- Reports include a store-scoped period profit/loss summary using sale-time cost snapshots, refund-date reversals, and recorded expenses. Gross and operating profit are withheld when period sale/return units have missing cost snapshots; UI visual review remains open.

## Official platform references

- [Tauri prerequisites and supported desktop development environments](https://v2.tauri.app/start/prerequisites/)
- [Tauri multi-window API](https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/)
- [Tauri window and monitor APIs](https://v2.tauri.app/reference/javascript/api/namespacewindow/)
- [Tauri window zoom hotkeys](https://v2.tauri.app/reference/config/#zoomhotkeysenabled)
- [Vercel Functions runtimes](https://vercel.com/docs/functions/runtimes)
- [Supabase Postgres row-level security](https://supabase.com/docs/guides/database/postgres/row-level-security)
- [Neon connection pooling](https://neon.com/docs/connect/connection-pooling)
- [Android adaptive apps](https://developer.android.com/develop/adaptive-apps/guides/get-started-with-adaptive-apps)
- [Huawei ArkTS](https://developer.huawei.com/consumer/en/arkts/)

## Additional workflow roadmap from the POS video transcript

The user supplied a transcript of the [Quicklogic POS demo](https://www.youtube.com/watch?v=x3Kli-OIPRk). It expands the roadmap with the following workflow groups:

1. **Fast checkout:** explicit keyboard map (including an F12 complete-sale shortcut with safe guards), product sorting, price lookup, saved sales drafts that restore to the register, quotations, customer quick-add, and multiple tender types.
2. **Returns and cash control:** linked returns with refund method, void/discount approvals, cashier shifts, shift history, opening/closing cash, X snapshots, and Z close reports that reconcile and close once.
3. **Management reporting:** sales by date/store/cashier/tender, expense report, stock alerts and movement, margin/profit-and-loss only when cost and expense data are complete, plus CSV/PDF export.
4. **Catalog and customer accounts:** variants, categories, brands, tags, barcode/label workflows, customer credit limits only after receivables and collection accounting exist.
5. **Store and hardware setup:** business identity, address/contact, currency/language/tax configuration, receipt header/footer and paper width/template, printer/scanner/customer-display profiles, backup and cloud-sync controls.
6. **People and security:** separate cashier, manager, and owner/administrator workspaces; per-action permissions, staff shifts/PINs, approval flows and audit history.

Local Reports read store-scoped sales, net partial and full refunds into period totals and tender movement, export complete paginated CSVs, and show a cost-aware P&L summary that withholds profit when item costs are missing. The activity log pages, searches, and exports safe audit metadata under `reports:read`; it intentionally omits raw event details. Drawer open/movements, X snapshots, counted Z close and closed history are implemented. Checkout stores cost/tax/discount snapshots; linked partial refunds and full-sale voids restore stock and reconcile selected payout tenders. The native Rust suite passes 35 tests at its recorded milestone; frontend build, lint, and Rust formatting pass. The isolated desktop checkpoint was removed during the 2026-11-01 generated-output cleanup; rebuild with `tools/cargo-build.ps1`. Staff accounts can be deactivated/restored with self/last-admin safeguards. Live authenticated cashier/report/audit/drawer/backup/staff workflow review remains pending. Receipt actions open the OS print dialog or prepare email/WhatsApp drafts for a staff member to review; they do not auto-send or prove that a printer or channel is connected.

Ticket discount approval is implemented locally: an active approver with `sales:discount:approve` reviews the ticket quote and authenticates with their own PIN/password without replacing the cashier session. Approval is ticket-, store-, requester-, and amount-bound, expires after 15 minutes, is invalidated when the basket/price/customer/discount changes, and can be consumed only once inside checkout's transaction. Approval attempts are rate-limited. An unapproved/expired discount is excluded from the cashier/customer-display amount due. Three Rust regression tests cover quote changes, expiry, approver access changes, and one-time consumption. The full Rust suite (35 tests), desktop build, frontend build, lint, formatting, and migration smoke checks pass. Authenticated UI review remains open.

## Shared branch inventory and transfers

The business is the tenant; branches own inventory balances; register stations are children of a branch and share that balance. The cloud API is the authority for cross-device availability and transactional holds/transfers. Expose branch availability by canonical company product with on-hand, reservations, outbound commitments, available-to-promise, observed-at, and freshness fields. Return `unknown`/stale for offline caches and disable authoritative holds or transfers until connectivity returns. API commands enforce tenant and branch scope, role permission, idempotency and expected revisions.

Implement canonical catalog mapping and owner resolution for ambiguous legacy branch SKU/barcode rows before branch search. Then deliver, in order: cashier discovery and branch contact/redirect; expiring customer pickup reservation with atomic allocation/release; internal transfer request with source acceptance, dispatch, in-transit state, partial destination receipt, discrepancy resolution and paired stock movements; notifications, admin policies and consolidated reporting. Stations can be offered as pickup/contact counters but do not have separate quantities. Cross-branch payment, revenue ownership and tax/return settlement remain disabled until explicitly designed.

Full domain contract, lifecycle and acceptance criteria: `.planning/arena/branch-stock-routing/cross-judge.md` and `.planning/arena/branch-stock-routing/grounding.md`.

## Staff access and station licensing

One station is one registered POS terminal in a branch, not one employee account. Allow any number of named staff accounts subject to tenant policy; several cashiers can take turns signing in or switching to their profile on the same station, and all sales, cash movements, refunds and work periods must retain the acting person's identity. Only one active cashier/temporary-cashier actor may transact at a station at a time. Owners and managers can access the register for read/review and ticket preparation; they can transact only with their own explicit temporary-cashier mode enabled. A station in another branch or a second physical terminal consumes another active station slot. The owner account can manage all branch and station assignments according to plan capacity.

The desktop local station registry currently identifies tills for attribution and lets an installation select its station; it does not yet enforce paid capacity or coordinate a terminal lease across computers. For the cloud implementation, keep employee memberships and station activation as separate records, count active station public keys against the tenant's `stations.active` entitlement in one database transaction, and grant the active actor a short lease. Every sale/drawer mutation must validate that lease transactionally; profile switch/logout releases it, heartbeat renews it, and expiry permits audited recovery after a client crash. Offline terminals can enforce their own local actor session using a signed lease, but must disclose that the server cannot prevent another disconnected installation from acting concurrently. Never bill per named staff account as a substitute for station capacity.
