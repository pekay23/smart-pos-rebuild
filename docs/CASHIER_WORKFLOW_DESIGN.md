# Cashier workflow architecture

> Product-wide continuation checklist: [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md). This document preserves the cashier-specific design and verification record.

## Work phases

- [x] Ground the current register, session, and data flow
- [x] Sketch two end-to-end architecture candidates and compare them
- [x] Select and record the implementation shape
- [x] Implement register persistence, customer capture, and staff switching
- [x] Review implementation friction and run compile, lint, and migration checks
- [x] Complete an interactive Windows smoke test of hold/resume, PIN switching, checkout, customer capture, and zoom

## Grounding

- `AppShell` owns the active `SessionUser`, view navigation, Tauri zoom, and cashier full-screen preferences. It mounts `RegisterPage` while the register view is active.
- `RegisterPage` currently owns cart, discount, payment method, tendered amount, and customer selection only in React state. Barcode/SKU input resolves an existing catalog item and adds it on Enter; product tiles add on click. Checkout calls the Rust `checkout_sale` command and only clears the cart after success, so a failed request leaves the current page state intact. Closing or unmounting the page loses that cart.
- The Rust backend persists business data in SQLite. `users` already supports multiple users per business, with a distinct Argon2 password hash and role relation. `AppState.active_session` is a single process-wide session; login verifies a password and replaces it. There is no staff profile-switch/PIN command or staff management screen yet.
- Customers are store-scoped records with name, email, phone, address, and notes. Cashiers already have `customers:write`, but the cashier UI has no create-customer action. Sales store a nullable `customer_id`, so a sale can be linked to a customer when the cashier creates the profile before checkout; post-sale attachment requires an explicit, audited operation.
- Cart drafts should remain separate from completed sales and stock movement. A draft/held basket does not reserve stock or count as a sale. The checkout transaction remains authoritative for stock validation and payment recording.
- The register currently has `min-height: calc(100vh - 160px)` and nested cart scrolling, but the outer workspace/page can still grow at increased WebView zoom. The register needs a height-bounded cashier mode, with catalog and ticket sections scrolling inside that viewport.
- `cash_shifts` exists in the initial SQLite schema but has no active close workflow. It does not automatically close the day or the register.
- Historical rationale is unavailable in this checkout: `git log` reports that the folder is not a Git repository, and no issue tracker or team-chat connector is configured. The current shape is understood from source and project plans, not from its original design discussion.

## Usage sketch

```text
Cashier adds or searches products, scans barcodes, removes selected lines or clears the basket.
Cashier holds the current basket; the next customer starts with an empty ticket.
Cashier reopens any held basket after a payment/network/device interruption and continues checkout.
Cashier creates a walk-in contact from the register and can attach it to a sale already recorded.
Staff selects their profile and confirms with their own PIN; backend replaces the active principal and its permissions.
```

## Shape

Use one Rust-owned, revisioned register workspace per store in the local database. It contains one active ticket and a bounded list of held tickets. Each cashier edit is an intent (`addProduct`, `removeLines`, `holdActive`, `resumeHeld`, `acceptCurrentPrices`), validated and committed by one backend operation. The React controller serializes actions and shows saving/error state; it does not persist an arbitrary workspace document.

The active and held tickets have stable ticket IDs, creator and last-editor attribution, customer selection, discount, payment method, tendered amount, and catalog references with the price accepted by the cashier. Restore refreshes the current catalog. If price changed, the backend returns a current quote; the cashier must explicitly accept it before checkout. Stock is never reserved by a draft and is revalidated during checkout.

`checkout_sale` consumes the active ticket in its existing immediate SQLite transaction. It uses the persisted ticket as the only checkout payload, validates accepted prices, stock, customer and tender, records the sale/payment/stock movements, and replaces the active ticket with a fresh empty one atomically. A nullable unique `sales.source_ticket_id` makes retries after a lost response return the original receipt without duplicate stock movement. Persist tendered and change amounts so the recovered receipt matches the original.

The workspace remains store-wide within this local database and survives staff switching; every ticket retains creator and last-editor names. This does not imply synchronization across separate devices. Existing customer save remains the one profile-creation path. Cashiers can attach a customer only to their own completed sale; staff with `sales:read` can attach within the store. Both paths validate store scope and write an audit event.

Add nullable `users.pin_hash`, a persisted attempt lockout, and safe staff-profile listing. PINs are salted Argon2 hashes. A switch verifies the target's credential in Rust, reloads the target's current role and permissions, then replaces the active session. Existing accounts without a PIN may switch with their password. Admins get a minimal team panel to create cashier, manager, and administrator accounts with unique usernames, strong passwords, and optional PINs.

In cashier mode, the shell and register occupy the available viewport. The catalog grid, ticket lines, and long dialogs scroll internally; the payment action stays reachable at high zoom and short heights. At narrow widths the product list and ticket become tabs so neither is pushed below the page.

## Synthesis decision

Candidate B is the base (cross-judge: 21/25 vs Candidate A's 19/25). Its action-based Rust boundary makes hold, resume and checkout single state transitions, and its viewport plan covers narrow and short screens. Graft Candidate A's store-wide ticket list and creator/last-editor attribution so a successor staff member can recover work. Keep A's persistent completed-ticket linkage, but use a unique source ticket ID on the sale for idempotent receipt recovery. Narrow cashier post-sale customer attachment to the cashier's own sale. Remove B's terminal UUID and event/attempt machinery; this app currently has one local database, and the ticket ID is enough for safe retry.

## Tradeoffs accepted

- We accept a versioned JSON workspace and explicit decode recovery in exchange for atomic active/held transitions through a small caller API.
- We accept optimistic revision conflicts when separate windows edit the same store workspace; conflicting work must reload or be held before editing.
- Held tickets remain local to this database. Cross-device sharing waits for the future API/database service.
- Basket changes save as acknowledged actions; unsent text in an open customer/discount/tender input may be lost on sudden power loss until committed.
- Card and mobile-money methods record a completed external payment only; this feature does not add payment-provider authorization or reconciliation.

## Alternatives considered

Candidate A's normalized ticket and ticket-line tables make list/search queries straightforward, but would add separate save/hold/activate lifecycle calls unless hidden behind the same action service. Candidate B's terminal-owned aggregate would require terminal identity and prevent successor staff from seeing work through a store-wide view. React localStorage would lose backend authority for permissions, quote checks and atomic checkout recovery.

## Open questions and risks

- A short numeric PIN is convenient but weaker than a password. Store only a salted Argon2 hash, rate-limit verification, and keep high-privilege settings actions behind their existing permission checks.
- No connected payment provider exists: a payment timeout after the external provider charged but before this app confirms remains a reconciliation workflow, even though the local ticket will survive.
- Workspace and held tickets are local to this device. The future API must add cross-device synchronization before staff on separate tills can share an in-progress customer ticket.

## Implementation and verification status

- Register actions, revision checks, persisted active and held tickets, checkout idempotency, customer capture/linking, and PIN/password staff switching are wired through Rust, SQLite, and the React register.
- Cashier mode bounds the application to the WebView viewport. Product results, cart lines, checkout controls, and dialogs scroll within their own panels; narrow layouts switch between the catalog and current ticket.
- `npm run build` passes. `npm run lint` exits successfully with existing state-in-effect warnings in `PeoplePage.tsx`, `ProductsPage.tsx`, and the register's intentional synchronization from the persisted ticket into its editable amount fields.
- `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check` passes. `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` compiles the desktop crate but currently reports zero automated tests. A SQLite smoke check created the fresh schema, applied the cashier column upgrades, and ran migration `0006_cashier_workflows.sql` twice successfully.
- An isolated Windows debug app with a separate application identifier and database completed a live UI smoke test: barcode/SKU entry added the matching product; the active basket held and resumed; the cashier switched to the demo manager with PIN `246802`, saw manager navigation, and switched back with cashier PIN `357913`; a walk-in customer with phone and identifying note was created and attached to the active ticket; selected-line removal and clearing the whole ticket both worked; a local cash checkout recorded one sale and reduced only the isolated demo database's MDF stock from 8 to 7; the receipt then created and linked another customer after checkout.
- The post-sale customer form initially rendered below the receipt backdrop. Its conditional backdrop now uses a higher stacking layer, and the live smoke test confirmed the form appears above the receipt and the linked name and phone then appear on the receipt.
- At enlarged WebView zoom, the cashier register remained within the app viewport without a document-level scroll; the checkout controls used their own internal scroll area. The manager navigation sidebar scrolls independently when its longer menu needs more room. The temporary debug instance was used for all UI checks; the installed release window and its data were not used.

## Follow-up work

Add automated Rust and frontend tests for ticket revisions, duplicate checkout retries, stock/price changes, role permissions, PIN lockout, customer attachment, and responsive zoom. Add shared ticket synchronization only with the future web API/database service; the current workspace and demo checkout are local to one device.
