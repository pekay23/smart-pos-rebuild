# Candidate: Native clients with a shared Rust domain engine

Status: independently authored synthesis candidate for comparison, not yet selected.

## Decision

Use a native UI on every client and a Rust domain/application engine for rules that must agree across clients. Publish that engine as `pos-domain` with no Tauri, WebView, SQLite, or HTTP dependencies. The Tauri desktop app and a new Rust API service call it directly. Android and Apple clients call a stable C ABI generated through UniFFI. HarmonyOS uses a typed OpenAPI client because there is no established Rust FFI path in this repository. The web app also uses OpenAPI and never embeds the desktop shell.

The UI stacks are React/TypeScript for web, React/Tauri for Windows/macOS/Linux desktop (with platform-specific shell and hardware adapters), Kotlin/Jetpack Compose for Android, Swift/SwiftUI for iOS/iPadOS, and ArkTS/ArkUI for HarmonyOS NEXT. Native apps own their navigation, accessibility, secure credential storage, font scaling, scanner/camera, notifications, and system sharing. Shared behavior comes from contract fixtures and engine use cases, not copied screens.

## Caller use and public modules

```rust
let command = CheckoutCommand::try_from(validated_wire_request)?;
let result = checkout.execute(actor, command, &mut unit_of_work)?;
```

```kotlin
val outcome = PosEngine.checkout(localSession, checkoutDraft)
```

```swift
let outcome = try await api.checkout(draft, idempotencyKey: draft.operationID)
```

Proposed crates and interfaces:

```text
crates/pos-domain       IDs, Money, validated quantities, tax/discount/refund rules
crates/pos-application  Checkout, refund, receiving, shift, drawer, report use cases
crates/pos-contract     OpenAPI DTO fixtures and stable error/status vocabulary
crates/pos-ffi          narrow UniFFI facade; opaque engine/session handles, no SQL types
apps/api                auth/scope, HTTP DTO validation, Postgres unit-of-work, workers
apps/desktop/src-tauri  Tauri shell, SQLite adapter, local device session, native hardware
apps/android            Compose navigation/features, Credential Manager, Android adapters
apps/ios                 SwiftUI navigation/features, AuthenticationServices, iOS adapters
apps/harmony             ArkUI features, platform services, OpenAPI-backed data layer
apps/web                browser-native features, OpenAPI client, bounded local drafts
```

`ActorContext` is created by the host after authenticating a cloud or local session. `CheckoutCommand` holds store and station IDs as untrusted requested scope until the host authorizes them. `CheckoutResult` distinguishes acknowledged, provisional, rejected and reconciliation-needed states. `UnitOfWork` commits sale, tenders, stock ledger, audit and idempotency result atomically. `IdempotencyKey` binds a store, command kind and canonical payload hash.

Domain behavior is expressed as pure or repository-backed use cases. Repositories expose transactions and named read/write operations, not arbitrary SQL. Implement separate SQLite and Postgres adapters; never pretend their locks, date functions or pagination semantics are interchangeable. Server sessions and local sessions are distinct types. Transport DTOs are converted at the boundary; domain structs are not serialized directly.

## Current extraction needed

Today `posApi` calls Tauri directly. Most command handlers in `apps/desktop/src-tauri/src` open `AppState`, read a mutable desktop session and issue SQLite statements themselves. `pos-core` currently exports only sale-total allocation helpers. The engine architecture therefore requires moving complete vertical slices (first catalog read, then online checkout) out of Tauri files, separating authorization context from `AppState`, and adding explicit repository/transaction traits. A shared type crate alone will not create a reusable engine.

## Runtime and offline policy

Cloud actions go to the Rust API, where the server rechecks staff permissions, store membership, subscription/device entitlements, totals, stock and idempotency inside a transaction. Postgres is authoritative for cloud businesses. Desktop and Android/Apple clients may host encrypted local SQLite for approved offline workflows, using the same Rust use case and an outbox committed in the same transaction as each provisional operation. The server still decides cross-device acceptance. Mobile starts with cached read-only views and local drafts; offline checkout is enabled only after device leases, stock conflict, receipt numbering, and cash/ledger rejection workflows are proven. Browser starts with no offline sales.

An offline operation cannot mutate shared stock authoritatively, use cloud payment credentials, or show as settled. It receives a station-scoped provisional receipt. The engine retains the operation and server response for reconciliation; it never retries under a new ID. If storage, clock, device lease, or authorization is invalid, checkout fails closed.

## Platform-specific delivery map

| Target | UI / native services | Initial slice |
| --- | --- | --- |
| Windows/macOS/Linux | Existing Tauri/React shell tailored per OS; Rust engine linked directly; per-OS hardware plugins | Keep current SQLite checkout, extract sale read/write and demonstrate one handler using the engine |
| Web | React responsive workspace; browser-first routes, keyboard access, IndexedDB only for explicit drafts | Secure login, overview/catalog/sales reports against authenticated OpenAPI API |
| Android | Kotlin/Compose with adaptive navigation and Material 3; Credential Manager; camera/HID scanner; printer/payment SDK adapters | Inventory lookup, receiving and online checkout; test phone and tablet; add offline only after sync proof |
| iOS/iPadOS | SwiftUI idiomatic navigation; AuthenticationServices; camera scan, system share/print support | Manager dashboard and stock count on iPhone; iPad register/checkout after accessory certification |
| HarmonyOS NEXT | ArkTS/ArkUI and Huawei-distributed design; native camera/barcode and authorized SDK adapters | Online inventory and operations against OpenAPI, then device-specific POS validation |

Build, sign, distribute and visually inspect each native client in its own supported environment. A Compose preview does not verify a rugged terminal. A SwiftUI build needs macOS/Xcode. A HarmonyOS client needs DevEco Studio/device validation. No target receives a parity claim solely because `pos-domain` compiles.

## Rollout and gates

1. Split the current crate into `pos-domain` and `pos-application` without changing behavior; add fixture-based rules tests.
2. Extract one desktop vertical slice through a repository transaction. Keep Tauri commands as thin adapters; verify current local databases migrate in place.
3. Add a Rust API around the application service with authentication, tenant resolution, OpenAPI DTOs, Postgres transactions, idempotency and cross-tenant denial checks. Vercel hosts the web static app; the API and durable jobs remain a separately operated Rust service.
4. Build the web management client and measure real API authorization, accessibility and responsive behavior. Keep cloud authentication separate from legacy local credentials.
5. Generate UniFFI bindings, prove cryptographic/storage dependencies and SQLite migration on Android and Apple platforms, then add native clients and their hardware adapters.
6. Build HarmonyOS as an API client, use contract fixtures for parity, and ship only after DevEco and hardware validation.
7. Enable offline work one operation at a time after crash/replay, lost acknowledgement, expired cursor/device, stock collision, backup restore and financial reconciliation acceptance.

## Alternatives considered

- API-only native clients with duplicated local validation (Candidate 1) are a smaller first server milestone and reduce FFI risk. They win if mobile stays online-first permanently. They lose for a full POS offline workflow because offline rules would either be weak/duplicated or require a second engine later. This candidate adds FFI and release burden to preserve one local decision engine.
- One Tauri/React mobile app saves UI effort but weakens native navigation and hardware integration and fails the user’s request for custom platform experiences. It remains a possible time-boxed device compatibility spike, not the shipped mobile UI.
- One cloud database accessed directly from all clients lowers API work but exposes too much policy to UI clients and duplicates workflow transactions. It loses because stock, permissions, refund and audit invariants need one controlled service boundary.

## Tradeoffs and risks

- We accept extracting and maintaining Rust FFI for Android/iOS in exchange for shared offline business rules and parity. If FFI or native packaging becomes a repeated source of failure, keep server online-first and reassess before enabling mobile offline.
- We accept that web and HarmonyOS cannot run the same local Rust module in this plan. The server remains authoritative, and contract/behavior fixtures police client previews; no client decides final tax, stock, refunds or entitlements alone.
- We accept separate web/native UI implementations to make each app feel local and integrate with platform capabilities.
- We accept staged feature parity: the first mobile release is not a full till until secure offline state and certified peripherals exist.
- The main unresolved risks are license/update distribution, provider-specific terminal SDKs, online-service hosting and data region, offline negative-stock/cash-rejection policy, and Apple/Harmony app distribution requirements.
