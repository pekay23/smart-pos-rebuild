# Candidate A: normalized durable tickets

## Caller usage

```ts
// RegisterPage opens the store's recoverable work, or starts a new ticket on first edit.
const tickets = await posApi.listTickets(); // working and held, creator + last editor
const ticket = await posApi.restoreTicket(ticketId); // returns current version + catalog warnings
const saved = await posApi.autosaveTicket({ id: ticket.id, expectedVersion: ticket.version, draft });
await posApi.holdTicket({ id: saved.id, expectedVersion: saved.version });
const receipt = await posApi.checkout({ ticketId: saved.id, expectedVersion: saved.version,
  paymentMethod, amountTenderedMinor });
const customer = await posApi.saveCustomer({ name, phone, email, address: null, notes: null });
await posApi.attachSaleCustomer({ saleId: receipt.id, customerId: customer.id });
const nextUser = await posApi.switchStaff({ userId: selectedId, pin });
onSessionChanged(nextUser);
```

Search filters existing active catalog products by name, SKU, barcode and category; Enter on an exact barcode/SKU adds that item, and a tile click adds it. Quantity changes, remove-selected and clear-all mutate the draft, not stock. Clear-all requires confirmation; selective removal leaves other lines untouched. A hold flushes pending edits, changes status and opens an empty workspace. A recovery list exposes both working and held tickets across the store with creator, last editor and update time. On switching staff, flush the current ticket first or stop and show the save error; the authenticated successor can restore it. On restart, select a recoverable working ticket or held ticket; never silently replace one with another.

## Type and persistence sketch

```ts
type TicketStatus = 'working' | 'held' | 'completed' | 'discarded';
type TicketLine = { productId: string; quantity: number };
type TicketDraft = { lines: TicketLine[]; customerId: string | null; discountMinor: number;
  paymentMethod: PaymentMethod; amountTenderedMinor: number };
type Ticket = { id: string; status: TicketStatus; version: number; createdAt: string;
  updatedAt: string; creator: StaffRef; lastEditor: StaffRef; draft: TicketDraft;
  warnings: TicketWarning[]; saleId: string | null };
type TicketWarning = { productId: string; kind: 'inactive' | 'missing' | 'stock_changed' | 'price_changed' };
type VersionedTicket = { id: string; expectedVersion: number };
type StaffProfile = { userId: string; fullName: string; roleName: string };
```

SQLite migration creates `tickets(id, store_id, creator_user_id, last_editor_user_id, status, version, customer_id, discount_minor, payment_method, amount_tendered_minor, created_at, updated_at, completed_at, sale_id)` and `ticket_lines(ticket_id, product_id, quantity, position, observed_price_minor)` with primary key `(ticket_id, product_id)`, positive quantity and foreign keys. Add store/status/update index, status checks, unique non-null `sale_id`; keep `sale_id` and completed row for idempotent receipt recovery. Captured price is diagnostic only: checkout uses current catalog prices, makes changes visible and requires cashier review. Draft writes validate same-store references, quantities and money, but do not reserve inventory. A missing or inactive product remains visible as a warning and must be corrected before checkout. `users.pin_hash` is nullable; it contains a salted Argon2 PHC string, never a PIN. A persisted per-profile attempt/lockout record prevents a restart clearing throttling.

## Command signatures and transaction rules

```rust
fn list_tickets(state: State<AppState>) -> Result<Vec<TicketSummary>, Error>;
fn restore_ticket(id: String, state: State<AppState>) -> Result<Ticket, Error>;
fn autosave_ticket(input: SaveTicketInput, state: State<AppState>) -> Result<Ticket, Error>;
fn hold_ticket(input: VersionedTicket, state: State<AppState>) -> Result<Ticket, Error>;
fn discard_ticket(input: VersionedTicket, state: State<AppState>) -> Result<(), Error>;
// Existing checkout_sale is extended; it remains the sole sale/checkout stock movement writer.
fn checkout_sale(input: TicketCheckoutInput, state: State<AppState>) -> Result<SaleReceipt, Error>;
fn attach_sale_customer(input: AttachSaleCustomerInput, state: State<AppState>) -> Result<(), Error>;
fn list_switchable_staff(state: State<AppState>) -> Result<Vec<StaffProfile>, Error>;
fn switch_staff(input: SwitchStaffInput, state: State<AppState>) -> Result<SessionUser, Error>;
fn set_staff_pin(input: SetStaffPinInput, state: State<AppState>) -> Result<(), Error>;
```

All ticket commands derive store and actor from `active_session(..., "pos:use")`, never client-supplied identity. Autosave creates a working ticket on first meaningful change; subsequent writes replace header and lines in one immediate transaction, compare version and increment it. Client serializes/debounces saves per ticket, exposes unsaved/error state, and flushes before hold, switch, navigation or checkout. A version conflict returns the latest server version and requires explicit reload or reconciliation; never overwrite a colleague's edit. Empty working drafts can be discarded explicitly. Held restore changes status to working with the same version guard; any authorized staff member in the store can perform it, preserving original creator and recording last editor.

Checkout flushes edits, then `checkout_sale` locks and reads the persisted ticket/version inside its existing immediate transaction, validates current customer, products, prices, stock and payment, writes sale, payment and stock movements, and marks the ticket completed with sale ID in the *same* transaction. An already completed ticket returns its existing receipt on retry; stale version, invalid stock, changed prices or payment failure leaves the ticket recoverable and creates no partial sale. UI does not clear until success; after an ambiguous timeout/restart it queries the ticket before retrying, preventing a duplicate sale. Receipt reconstruction must be supported from persisted sale data. Customer display follows the restored working cart and displays paid only after confirmed checkout.

Quick-create uses existing `save_customer` and its `customers:write` authorization; select the returned ID on the draft. Post-sale attachment checks `pos:use` and `customers:write`, validates sale/customer store scope, permits only a previously unlinked sale (same ID is idempotent; changes need a separate policy), and updates `sales.customer_id` plus `add_audit_event` with actor, sale and old/new customer IDs in one transaction. It does not rewrite totals or inventory.

Profile switch requires an existing authenticated session, same business/store, active target with configured PIN and `pos:use`. Verify the target's salted Argon2 hash in Rust, apply persisted per-target and device-wide throttling, use generic failures, and audit successes/failures without secrets. Construct permissions from the current role and replace `AppState.active_session` only after successful verification. PIN enrollment/reset is an explicit authenticated, permission-gated workflow with confirmation and no plaintext logging; accounts without a PIN use password sign-in until provisioned. Serialize switching with commands so an in-flight operation cannot complete under a principal the UI believes has changed; update `MainApplication` session state only after success and clear stale privileged view data.

## Module map

- `apps/desktop/src-tauri/src/db.rs`: versioned migration, ticket/line constraints, nullable PIN hash and rate-limit storage. Preserve existing databases.
- `apps/desktop/src-tauri/src/commands.rs`: extend `checkout_sale`, `active_session`, `login`-adjacent authentication and customer audit patterns; ticket commands can delegate to a cohesive ticket repository. Register commands in the Tauri invoke handler.
- `apps/desktop/src-tauri/src/models.rs` and `apps/desktop/src/lib/models.ts`: matching serialized ticket, error, PIN and receipt shapes. `apps/desktop/src/lib/api.ts` exposes typed invokes.
- `apps/desktop/src/pages/RegisterPage.tsx`: draft editing, recovery/hold, conflict and save state, customer quick-create, checkout and receipt attachment. `App.tsx`/`components/AppShell.tsx` propagate the returned session and re-evaluate permissions; `customerDisplay.ts` publishes restored ticket state.
- `apps/desktop/src/styles.css` and shell styles: cashier workspace fills the available viewport with `height: 100dvh`, a grid/flex chain with `min-height: 0`, fixed header/actions, and independently scrollable catalog and ticket bodies (`overflow: auto`). Account for zoom, narrow widths, keyboard focus and dialogs; avoid trapping controls below the viewport.

## Rationale and tradeoffs

`RegisterPage` currently owns an ephemeral `cart` and `completeSale` calls `posApi.checkout`; `get_products` is store-scoped, `save_customer` already audits changes, and Rust `active_session` checks the real principal. Normalized rows permit safe store-scoped listing, constraints and recovery without treating a serialized client object as authority. Single-transaction completion and persistent sale linkage handle a lost response after commit. This design adds migration and conflict-handling complexity, and autosave can lose edits since the most recent acknowledged write on sudden power loss; the UI must make save state visible. Handoff is store-wide only within the same local SQLite database: no cross-device synchronization is implied.

## Alternative

Store one serialized ticket JSON document per store with a version. It simplifies persistence but weakens line-level constraints and querying and increases migration and validation burden. It still needs the same checkout transaction, identity checks and conflict protocol.

## Risks and verification

- Stock and prices may change while held; refresh catalog on restore and immediately before checkout, show discrepancies, require review, and enforce again in the transaction.
- Exercise save/hold/restart/restore, cross-user handoff, selective/all clear, version conflicts, failed payment, crash before and after commit, and repeated checkout with one sale and one set of stock movements. Test same-store isolation and customer attachment audit/rollback.
- Test PIN setup, incorrect PIN throttling across restart, inactive/wrong-store user, failed switch retaining the original principal, and permission changes after successful switch. Low-entropy PINs remain vulnerable to offline attack if the database is stolen despite Argon2.
- Verify fresh and upgraded databases, customer display state, keyboard barcode scanning, zoom and short/narrow viewports in the actual desktop WebView.
