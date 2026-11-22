# Candidate B: durable terminal workspace

## Usage (caller's view)

The register restores one terminal workspace after sign-in. Every acknowledged edit is durable. Holding a ticket replaces the active ticket atomically; resuming requires an empty active ticket or an explicit swap. A ticket is a draft, never a sale or a stock reservation.

```ts
const register = useRegisterWorkspace(user); // Provider above routed pages in AppShell.
await register.act({ type: "addProduct", productId: catalogHit.id });
await register.act({ type: "removeLines", lineIds: selectedLineIds });
await register.act({ type: "clearActive" });
await register.act({ type: "holdActive", label: "Customer returning" });
await register.act({ type: "resumeHeld", ticketId, activeDisposition: "requireEmpty" });

// Enter resolves a real catalog product. Ambiguous codes require choosing a result.
const hits = catalog.resolveExactCode(scanText);
if (hits.length === 1) await register.act({ type: "addProduct", productId: hits[0].id });

await register.act({ type: "captureContact", contact: { name, phone, email } });
const outcome = await register.pay({ method: "cash", amountTenderedMinor });
// pay reuses the attempt ID after a transport failure; it never clears optimistically.
await sales.attachContact(outcome.receipt.id, { type: "existing", customerId });
const next = await staff.switchWithPin({ userId: selectedProfile.id, pin });
// App replaces SessionUser and permitted navigation from the backend response.
```

## Problem and grounded boundaries

`apps/desktop/src/pages/RegisterPage.tsx:12` currently owns its cart, tender, discount and selected customer in component state. `completeSale:68` clears after a successful `posApi.checkout`, but navigating away unmounts the register and loses drafts. `apps/desktop/src/components/AppShell.tsx:137` performs that conditional mount and already owns fullscreen/zoom behavior. The authenticated `SessionUser` enters AppShell as a prop, so a switch must update the owner in App as well as the Rust principal.

`apps/desktop/src/lib/api.ts:41` invokes `checkout_sale`; `apps/desktop/src-tauri/src/commands.rs:1048` authorizes `pos:use`, validates store-scoped customer/products, calculates totals through `crates/pos-core`, and writes the completed sale, items, stock decrements, movements, payment and audit event in one immediate SQLite transaction. Preserve that authority. Existing `get_products` returns the real store catalog, and RegisterPage already searches name/SKU/barcode and resolves exact SKU/barcode on Enter. The barcode index is not unique, so do not silently choose the first match.

`apps/desktop/src-tauri/src/db.rs:33` contains a database mutex and one in-memory `active_session`, reset on restart. Users have Argon2 password hashes and role permissions; there are no PIN fields or switch commands. `customers` is store scoped, and `sales.customer_id` is nullable. Cashiers retain `customers:write` but migration `0005_cashier_scope.sql` removes `sales:read`. A register receipt command must therefore authorize its own narrow use without granting cashiers full history.

## Shape

Persist a single JSON aggregate per store and terminal in `register_workspaces`. The aggregate embeds its active and held tickets. This is a coherent terminal workspace, not normalized draft rows and line tables. Use a database revision alongside a document schema version. The terminal ID is generated once and persisted by Rust in app data; commands derive store and terminal identity from trusted backend context.

```rust
struct RegisterWorkspace {
    schema_version: u16,
    active: DraftTicket,
    held: Vec<HeldTicket>,
    last_completed: Option<CompletedTicket>,
}
struct WorkspaceView { revision: u64, workspace: RegisterWorkspace, quote: TicketQuote }
struct DraftTicket {
    id: TicketId,
    created_by: UserId,
    lines: Vec<DraftLine>,
    customer: DraftCustomer,
    discount_minor: i64,
    tender: TenderDraft,
    attempt_id: CheckoutAttemptId,
}
struct DraftLine { id: LineId, product_id: ProductId, quantity: PositiveQuantity }
enum DraftCustomer { WalkIn, Existing(CustomerId), Contact(ContactDraft) }
struct ContactDraft { name: String, phone: Option<String>, email: Option<String> }
struct HeldTicket { ticket: DraftTicket, label: String, held_by: UserId, held_at: Timestamp }
struct CompletedTicket { ticket_id: TicketId, attempt_id: CheckoutAttemptId, sale_id: SaleId }
enum RegisterAction {
    AddProduct { product_id: ProductId },
    SetQuantity { line_id: LineId, quantity: PositiveQuantity },
    RemoveLines { line_ids: Vec<LineId> }, ClearActive,
    SetDiscount { minor: i64 }, SetTender { tender: TenderDraft },
    SelectCustomer { customer: DraftCustomer },
    HoldActive { label: String },
    ResumeHeld { ticket_id: TicketId, active_disposition: ActiveDisposition },
    DiscardHeld { ticket_id: TicketId },
}
enum ActiveDisposition { RequireEmpty, HoldAndSwap }
enum WorkspaceError { RevisionConflict(WorkspaceView), InvalidAction(String), Forbidden }
struct StaffProfile { id: UserId, full_name: String, role_name: String, has_pin: bool }
struct StaffPinInput { user_id: UserId, pin: SecretString }
enum SaleContactInput { Existing(CustomerId), New(ContactDraft) }
```

Rust constructs `PositiveQuantity` only after boundary validation. Product names, stock and prices come from the catalog or a backend quote and are not persisted as authoritative draft prices. A resumed ticket is repriced; show a changed-price/availability message before payment. No customer profile is required for a walk-in. Quick contact capture persists the contact in the draft, then checkout creates a customer and links the sale in the same transaction. This prevents retries from creating duplicate profiles.

### Signatures

```rust
// Tauri commands; bodies intentionally omitted for this design.
fn get_register_workspace(state: State<AppState>) -> Result<WorkspaceView, CommandError>;
fn edit_register_workspace(expected_revision: u64, action: RegisterAction,
    state: State<AppState>) -> Result<WorkspaceView, CommandError>;
fn checkout_sale(input: WorkspaceCheckoutInput,
    state: State<AppState>) -> Result<CheckoutOutcome, CommandError>;
fn attach_sale_contact(sale_id: SaleId, input: SaleContactInput,
    state: State<AppState>) -> Result<Customer, CommandError>;
fn list_switchable_staff(state: State<AppState>) -> Result<Vec<StaffProfile>, CommandError>;
fn switch_staff_pin(input: StaffPinInput,
    state: State<AppState>) -> Result<SessionUser, CommandError>;
fn save_staff_pin(input: SetStaffPinInput,
    state: State<AppState>) -> Result<(), CommandError>;

struct WorkspaceCheckoutInput {
    expected_revision: u64,
    ticket_id: TicketId,
    attempt_id: CheckoutAttemptId,
    tender: TenderDraft,
}
struct CheckoutOutcome { receipt: SaleReceipt, workspace: WorkspaceView }
```

`edit_register_workspace` owns schema decoding, edit validation, stock availability hints, quote calculation, revision checking and atomic replacement. It requires `pos:use`; contact edits additionally require `customers:write`. It accepts intent, never an arbitrary client-supplied workspace. Merge duplicate product additions into one line. Remove-selected verifies every line belongs to the active ticket. Clear resets lines, contact, discount and tender together, rotates the draft/attempt identities and leaves held tickets alone.

Each edit performs compare-and-swap on the workspace revision in SQLite. One frontend controller serializes actions and exposes saving/error state. The UI may display pending edits, but only acknowledged state is advertised as saved. Do not silently drop failed writes or debounce committed line edits. Money/contact typing stays in local form state until Apply/blur; navigating with a dirty form commits and awaits its action first. A crash can lose unsubmitted typing, but cannot lose acknowledged basket edits.

`checkout_sale` consumes the authoritative active ticket. In its existing immediate transaction, it checks the authenticated actor, revision and active ticket ID, revalidates catalog/stock/customer/tender, resolves quick contact, and records the sale. Add a unique `(store_id, attempt_id)` record mapping attempt to sale. In that same transaction it replaces the active ticket with an empty one, records `last_completed`, and increments the workspace revision. Check an existing successful attempt before rejecting an old workspace revision, after authorizing its store and terminal. Repeated delivery returns the original receipt without moving stock twice. Store a request fingerprint and reject a reused attempt ID with different tender/ticket contents.

Validation/payment failure rolls back and preserves the draft. A lost reply after commit is recovered by loading the workspace or retrying the same attempt ID. An edited ticket rotates its attempt ID, so a revision cannot accidentally reuse an old completed attempt. A revision conflict refreshes and asks the UI to reapply the single pending intent; never overwrite a newer aggregate. Existing card/mobile-money selection records an externally confirmed payment; it is not a payment-provider integration.

### Module map

| Boundary | Responsibility |
| --- | --- |
| `apps/desktop/src-tauri/src/register.rs` | Aggregate types, action reducer, revisioned persistence, backend quotes and held/resume semantics. SQLite details stay private. |
| `apps/desktop/src-tauri/src/commands.rs` | Existing checkout transaction extended for workspace checkout and idempotency; existing business commands retain permission checks. |
| `apps/desktop/src-tauri/src/staff.rs` | PIN enrollment/reset, profile listing, throttling, switch authorization and principal replacement. |
| `apps/desktop/src-tauri/src/models.rs` | Narrow serde input/output models. Persisted document representation remains private to register module. |
| `apps/desktop/src-tauri/migrations/0006_register_workspaces.sql` | Workspace document/revision, checkout-attempt uniqueness, nullable PIN hash and PIN lockout state. Explicit version migration in `db.rs`. |
| `apps/desktop/src-tauri/src/lib.rs` | Register new commands in both debug and release handlers. |
| `apps/desktop/src/lib/registerWorkspace.tsx` | Provider/controller, serial intent queue, quote projection and recovery status across page mounts. |
| `apps/desktop/src/lib/api.ts` | Invoke transport adaptation; no pricing or authorization policy. |
| `apps/desktop/src/pages/RegisterPage.tsx` | Catalog search/scanning, line selection, held drawer, contact/payment forms and receipt actions. |
| `apps/desktop/src/App.tsx`, `components/AppShell.tsx` | Replace session after PIN switch, invalidate principal-scoped queries, enforce permitted view and mount workspace provider. |
| `apps/desktop/src/styles.css` | Cashier viewport bounds, catalog/cart inner scrolling, responsive ticket/payment layout. |

### Staff security and switching

Profile switching is authentication in Rust. Staff select a profile belonging to the active business, enter that staff member's PIN, and receive a new `SessionUser` with role permissions loaded from the database. Never accept a requested role or permissions from React, and never reuse the outgoing user's privileges. Admins manage PINs using `users:write`; self enrollment/reset requires the staff member's current password. No default/shared PINs. Store separate salted Argon2 PIN hashes, apply per-user plus terminal-wide persisted cooldowns, avoid PIN logging, and invalidate disabled profiles immediately.

Use one documented lock order for auth mutations and protected operations: active-session guard, then database lock. A switch waits for an in-flight checkout, then atomically replaces the principal and emits a principal-generation change. Commands arriving from an old generation fail and reload the session. Do not hold the DB lock during expensive PIN hashing; verify against a loaded credential revision and recheck that revision/account status before replacement. This closes the current clone-session versus switch race without letting an old cashier command execute under a manager's newly selected role.

The workspace belongs to the terminal, not the outgoing staff member. Switching preserves drafts with creator attribution; the authenticated checkout actor becomes the sale cashier. Managers/admins can use the same register and their permitted management views. A cashier result immediately exits any management view. Signing out hides the workspace; restarting requires sign-in before restore, with no credentials or elevated session serialized into the aggregate.

### Contacts after sale

`attach_sale_contact` uses a store-scoped sale and either an existing same-store customer or new contact. It requires `customers:write` plus either the same cashier's recent receipt from this terminal with `pos:use`, or a manager/admin permission for history linkage. Grant no full `sales:read` to cashiers. Existing identical attachment is idempotent; changing an already-linked customer requires manager/admin authority and an explicit action. Create contact, update `sales.customer_id`, and record old/new linkage in one audited transaction. Never alter sale items, stock or payment. Receipt state updates from the response before email/WhatsApp draft actions use contact details.

### Viewport layout

Cashier register mode is a layout state, independent of OS fullscreen. Bound the shell to `height: 100dvh; overflow: hidden`, use a header row plus `minmax(0, 1fr)` content row, and put `min-height: 0` on every flex/grid ancestor. The catalog grid owns one vertical scroller. Ticket uses fixed heading/contact, `minmax(0, 1fr)` cart scroller, and payment footer. At short/high-zoom viewports payment details collapse into an accessible drawer whose own content scrolls, while the total and Take payment stay reachable. At narrow widths use catalog/ticket tabs inside the same bounded content region. Dialogs use `max-height: calc(100dvh - 2rem)` with internal scrolling, focus trapping and return focus. Verify at 80%, 100%, 125% and 160% zoom, including a short landscape viewport and many cart lines.

## Rationale

The terminal is the real unit of recovery. A workspace replacement atomically keeps the active ticket, held list and latest completed receipt consistent, per model-the-domain. React callers send a small set of register intents, and Rust hides persistence, stock-aware quoting and conflict resolution, per boundary-discipline. There is no caller choreography of save-line, save-ticket and set-active-ticket. Hold/resume therefore cannot strand a basket between tables or accidentally make two active tickets. The backend remains the only authority that creates a sale or moves stock.

The document stores product references and cashier input. Catalog and permissions remain separately authoritative. This avoids spreading product snapshots, pricing rules and role logic across the controller and storage format, per minimize-reader-load. Selected checkbox state, scan text and search filters remain ephemeral because they are presentation state, not a customer's recoverable ticket.

## Synthesis decision

Candidate B is a complete alternative for arena comparison. Its defining choice is one revisioned terminal-workspace aggregate with embedded drafts and an atomic checkout transition. The parent synthesis should choose it only if local terminal recovery and simple hold/resume operations matter more than cross-terminal ticket queries.

## Tradeoffs accepted

- We accept replacing a small JSON document for every committed edit in exchange for atomic hold/resume and one recovery read. Bound the document to 50 held tickets, 500 lines per ticket and validated text lengths; reject excess without truncating work.
- We accept schema-versioned Rust document migration in exchange for avoiding per-line draft tables and exposing their lifecycle to callers. A failed/unknown-version migration retains the original document and blocks edits with a recoverable error.
- We accept terminal-local held tickets in exchange for a coherent offline aggregate. Store-wide, cross-terminal handover would require a different ownership model, not an unguarded JSON copy.
- We accept backend round trips for acknowledged edits in exchange for durable restart recovery. The database is local; speculative display is allowed only with an honest saving/error indicator.

## Alternatives considered

A normalized `draft_tickets` plus `draft_lines` design makes store-wide search and cross-terminal handover straightforward. It exposes more lifecycle and transaction coordination unless wrapped in an equally capable register service. It loses here because this candidate treats the terminal's complete workspace as the recovery unit.

A React context plus localStorage snapshot hides navigation loss with little backend work, but exposes permission, catalog repricing, checkout reconciliation and multi-window coordination to JavaScript. It cannot atomically reconcile a committed sale with a cleared draft, so it loses on interface depth and failure recovery.

An append-only command/event log can rebuild the workspace and retain every edit. It adds event migration, replay and compaction rules to the persistence owner. The current desktop flow has no requirement that justifies that cost.

## Open questions and risks

- Does the product require staff to resume another terminal's held ticket? This candidate intentionally scopes recovery to one terminal; choose normalized store-wide ticket ownership if that is required now.
- Should cashier post-sale linkage expire after a fixed window such as the current business day? Default to the terminal's own recent completed receipt, with managers/admins handling older receipts.
- Does a short viewport need a persistent payment drawer or a single-pane ticket tab? Verify the actual cashier device and zoom settings before final layout selection.
- Can a held draft refer to a deleted product or customer after future catalog cleanup? Resolve references on restore and retain recoverable lines/contact until a cashier explicitly removes or relinks them.
- What is the operational recovery policy for a corrupted workspace document? Preserve the raw row, show a recovery error, and require explicit discard/export rather than silently creating an empty basket.

## Next implementation step

Build the Rust workspace migration and action reducer, then prove hold/resume, revision conflicts, rollback preservation and lost-reply checkout retries against a temporary SQLite database before wiring RegisterPage.
