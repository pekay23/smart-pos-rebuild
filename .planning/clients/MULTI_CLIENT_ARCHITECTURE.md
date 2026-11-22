# Multi-client architecture decision record

**Status:** initial API foundation implemented; tenant and client work remains  
**Owner:** Advanced POS implementation goal  
**Scope:** web, Windows/macOS/Linux desktop, Android, iOS/iPadOS, and HarmonyOS clients.

## Work phases

- [x] Ground the current desktop boundary and confirm which clients exist.
- [x] Compare materially different client/runtime architectures and review the candidates.
- [x] Select one contract, map client-specific modules, and capture the rejected alternatives.
- [x] Implement the first API vertical slice against the selected contract (`apps/api`, `/healthz`, `/v1/sale-calculations`, OpenAPI 3.1).
- [x] Verify the stateless API contract and its boundary; record platform gaps. The responsive web client and authenticated tenant authorization are not implemented.

## Current grounding

- `apps/desktop/src/lib/api.ts` is a large `posApi` facade, but each method calls a named Tauri command through `@tauri-apps/api/core`.
- `apps/desktop/src/pages/*` and `components/*` depend on that desktop-specific facade and are not yet a transport-neutral client layer.
- `apps/desktop/src-tauri/src/*` owns command handlers, SQLite state, and most domain operations together. `crates/pos-core` currently contains only a small Rust domain crate; the full desktop backend is not yet a reusable platform-independent engine.
- The Cargo workspace contains `crates/pos-core` and `apps/desktop/src-tauri`; there is no API service, web client, or native mobile project yet.
- `docs/PRODUCT_PLATFORM_PLAN.md` already establishes a versioned OpenAPI direction, a separately deployable Rust service, a Vercel-hosted web client, and hardware-specific platform adapters. The new user request adds a clear acceptance bar: platform clients should use their platform's UI and capability conventions rather than being generic wrappers.

## Candidate workspace

Each architecture candidate and its rationale will be stored in a separate subdirectory under `.planning/clients/candidates/`. The selected design, scorecard, grafts, and rejections will be recorded here before the candidate notes are removed or archived.

## Synthesis decision

Select **API-first with platform-specific clients** as the base, as recommended by the cross-judge at 22/25 versus 18/25 for embedding a shared Rust engine into mobile immediately. Build `apps/web`, `apps/android`, `apps/ios`, and `apps/harmony` as separate clients that use the versioned OpenAPI service. Keep the current React/Tauri desktop client and its local SQLite adapter while extracting backend use cases into reusable Rust application services.

Graft the shared-engine candidate's explicit `ActorContext`, separate cloud/local session types, transaction/unit-of-work boundary, stable idempotency key bound to a canonical payload, and provisional/rejected/reconciliation-needed operation status. Never expose domain database structs as transport DTOs. Keep server transaction authority over checkout, refund, stock, audit, role scope, and subscription/device entitlements.

Use Kotlin/Jetpack Compose for Android, Swift/SwiftUI for iOS/iPadOS, and ArkTS/ArkUI for HarmonyOS NEXT. The web client is browser-native React/TypeScript; desktop remains Tauri/React for now with OS-specific shell and hardware adapters. Reuse OpenAPI schemas, accessible design tokens and behavioral fixtures, not a forced shared mobile screen layer.

Start web and mobile online-first. Retain carefully bounded local-first desktop behavior. Defer Rust mobile FFI and offline mobile selling until a small native binding, secure storage, SQLite migration, device identity, sync replay and accounting recovery proof justifies the extra release burden. The largest shared failure case is a provisional cash sale rejected after goods leave the store; do not enable offline checkout until an accounting policy and recovery drill address till, stock and ledger reconciliation without duplicate entries.

Rejected or deferred: one generic Tauri/React app for all platforms (does not satisfy native interaction/hardware goals); direct client access to a shared database (does not centralize workflow authorization/transactions); immediate shared Rust FFI on mobile (high extraction/packaging burden before mobile offline workflows are proven). An initial mobile adapter spike may be used only if it helps measure device SDK feasibility; it is not a substitute for the approved native UI implementation.

Candidate and review artifacts: `candidates/api-first/CANDIDATE.md`, `candidates/shared-engine/CANDIDATE.md`, and `CROSS-JUDGE.md`. The shared-engine author did not produce a draft after repeated bounded prompts; the parent reconstructed that comparison candidate from the grounded requirements and had the judge score the saved artifact. The API-first candidate and cross-judge are independently authored. This synthesis is an architecture decision, not evidence that any web/mobile app or API has been implemented.

