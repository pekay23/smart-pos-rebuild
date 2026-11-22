# Client architecture cross-review

Reviewed both candidate documents end to end against the current grounding in `MULTI_CLIENT_ARCHITECTURE.md`. Scores run from 1, weakest, to 5, strongest. Higher burden scores mean less implementation and operational burden.

| Criterion | API-first | Shared engine | Reason |
| --- | --- | --- | --- |
| Platform-custom UI and capabilities | 5 | 5 | Both choose Compose, SwiftUI and ArkUI, preserve desktop adapters, and require actual device and accessory validation. Sharing Rust does not itself improve native UI fit. |
| Shared rules and financial invariants | 5 | 5 | Both extract Rust application services and make server transactions authoritative. Both bind idempotency to payloads and include sale, tender, stock and audit writes in one transaction. API-first already shares local desktop and server rules; it does not require copied mobile business logic. |
| Staged extraction from Tauri/SQLite | 4 | 3 | API-first starts with one catalog slice, then online checkout, and explicitly removes desktop session assumptions. Shared engine adds a crate split and mobile binding work before evidence that mobile needs an embedded engine. Both still face substantial SQL and transaction extraction. |
| Offline operation and recovery safety | 4 | 3 | API-first specifies ordered device sequences, tombstones, cursor reset without losing unsent work, dependent-operation failures and an operator review queue. Shared engine covers the main crash and replay gates but leaves more of that protocol unspecified. Neither resolves the financial policy for rejected cash sales yet. |
| Implementation and operational burden | 4 | 2 | Both require several UI projects, a Rust service, Postgres and native release pipelines. Shared engine additionally requires Android/Apple Rust packaging, bindings and local database migration support while those clients initially remain online-first. |
| Total | 22/25 | 18/25 | Scores assume the stated release gates are enforced. |

Choose API-first as the base. It fits the existing OpenAPI/service direction and gets a useful web client through a bounded extraction. It also preserves the option to reuse Rust rules locally. The real difference between these candidates is whether Android and Apple embed the engine now, not whether the product shares business rules at all.

Graft the shared-engine candidate's explicit `ActorContext`, distinct local/cloud session types, and transaction interface into the extracted application boundary. Keep transport DTOs separate. Defer `pos-ffi` until a required mobile offline workflow passes a small binding, secure-storage and migration proof on Android and Apple. A successful binding alone is insufficient to enable offline checkout.

The biggest failure case is a provisional cash sale rejected after goods leave the store. Shared calculations cannot undo the physical sale or decide the correct till, stock and ledger treatment. Before enabling offline checkout, require an agreed accounting policy and a recovery drill covering a stock collision, lost acknowledgement, replay and operator reconciliation without duplicate financial entries. This condition applies to both candidates.
