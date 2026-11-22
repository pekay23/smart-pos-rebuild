# Candidate: one authoritative branch ticket row

## Caller usage

1. The register keeps its current browser-local active draft and held-ticket list. A cashier may continue to hold/resume local tickets offline exactly as today; those tickets are device-local and never imply a completed sale.
2. When online, the cashier may publish a local draft as a shared branch ticket. The client first obtains/renews its station lease, then calls `create`. The response supplies the canonical ticket ID and revision.
3. Other branch-authorized staff list shared tickets with `sales:read`. A manager/owner can inspect them but cannot claim, edit, release, or check them out. A cashier claims with the current station lease ID and token; claim succeeds only if unclaimed or reclaimable.
4. The claiming cashier loads the row and its revision into the register. Each save/claim/release is conditional on the expected revision and active lease. A stale station refreshes and resolves the conflict instead of overwriting.
5. On checkout, freeze the exact existing `CheckoutRequest` body and idempotency key into the shared row before calling the existing signed `sales:checkout` flow. Once that attempt is recorded, ticket contents and attempt are immutable. On timeout/unknown result, retry that exact body/key through the original station lease flow; never regenerate either. A confirmed sale marks the ticket completed with the sale ID in the same transaction boundary as sale commit, or via an idempotent finalization keyed by the same attempt if the current commit boundary cannot include it. The ticket remains a ticket record, not a sale record.
6. If a cashier releases the claim, or its lease expires/revokes, the row becomes claimable. Another cashier can claim at the latest revision and continue only when no uncertain checkout is in flight.

## Contract and module map

- **`apps/api/migrations/postgres/0064_shared_held_tickets.sql`** — add `branch_held_tickets`: UUID ID, tenant/branch, revision, status (`open | checkout_pending | completed | discarded`), label, JSONB draft payload, immutable nullable checkout body/key/fingerprint, optional completed sale ID, created/updated timestamps; claim lease ID and expiry are columns on this same row. Unique/foreign keys bind tenant, branch, ticket and station lease. Index by tenant/branch/status/updated time. Force RLS; grant the runtime role only needed SELECT/INSERT/UPDATE on the table and sequence (no DELETE, no owner bypass).
- **`apps/api/src/held_tickets.rs`** — typed domain and transactional SQL. Every transaction sets the existing `pos.identity_id`, `pos.tenant_id`, `pos.membership_id`, `pos.branch_id`, and station context. RLS limits tenant/branch visibility; endpoint authorization remains authoritative.
- **`apps/api/src/lib.rs`** — branch list/detail/create routes authorize `sales:read`; claim/save/release/discard/persist-attempt routes authorize `sales:checkout` and require current station ID, lease ID/token, and signed station proof as appropriate. Reuse the existing lease validator and `authorize_catalog_scope`; do not broaden sales permissions. List is read-only for managers/owners.
- **`apps/api/src/authorization.rs`** — retain existing role contract: owner/manager `sales:read`; cashier `sales:checkout`. Add no new broad capability implicitly.
- **`apps/web/src/api.ts`**, **`HostedRegisterPanel.tsx`** — typed client calls and explicit “This device” versus “Shared branch” lists. Retain localStorage draft key and recovery logic; online shared-ticket commands are additive. Never silently upload or hide a local-only ticket.

Suggested Rust API shapes:

```rust
struct HeldTicket { id: Uuid, revision: i64, branch_id: Uuid, status: TicketStatus,
    draft: TicketDraft, claim: Option<TicketClaim>, checkout_attempt: Option<ExactCheckoutAttempt> }
struct TicketClaim { lease_id: Uuid, station_id: Uuid, expires_at: DateTime<Utc> }
struct ExactCheckoutAttempt { body: CheckoutRequest, idempotency_key: Uuid, fingerprint: String }

async fn list(pool, actor, branch) -> Result<Vec<HeldTicket>, Error>; // sales:read
async fn create(pool, actor, branch, draft) -> Result<HeldTicket, Error>; // sales:checkout + lease
async fn claim(pool, actor, branch, id, revision, lease_id, lease_token) -> Result<HeldTicket, Error>;
async fn save(pool, actor, id, revision, lease_id, lease_token, draft) -> Result<HeldTicket, Error>;
async fn release(pool, actor, id, revision, lease_id, lease_token) -> Result<HeldTicket, Error>;
async fn record_attempt(pool, actor, id, revision, lease_id, lease_token,
    attempt: ExactCheckoutAttempt) -> Result<HeldTicket, Error>;
```

## Authority, concurrency, and recovery rules

The row is the single source of truth for shared ticket state and its active claim. Mutations run in a transaction with `SELECT … FOR UPDATE`, validate branch scope, current non-revoked lease token hash, lease station/actor, and claim lease ID, then compare-and-swap `revision`; increment revision once on success. Claim eligibility is an atomic conditional update where claim is empty, or its referenced lease is expired/revoked/released. Claim expiry is bounded by lease expiry; lease renewal may extend it only for the same lease. An old lease cannot mutate after a new lease claims. Release clears only a claim matching the supplied current lease. A pending exact checkout blocks draft edits, discard, and claim handoff until confirmed or explicitly reconciled from the idempotency result. Completed/discarded rows are terminal and retained for audit/duplicate protection.

Keep server JSON payload validation bounded and typed (known product/customer/payment IDs, positive quantities, size limits); never trust a client-supplied branch, actor, expiry, permission, sale, or lease status. Apply the existing signed station proof to writes and keep RLS forced with explicit runtime grants; RLS is defense in depth and not a substitute for route authorization.

## Alternatives and tradeoffs

- **Chosen: one row with revision and claim fields.** Smallest atomic boundary for draft + claim + attempt; row lock/CAS makes exclusive claim and stale-write handling straightforward. Tradeoff: frequent whole-row updates and claim fields share the ticket's write hotspot.
- **Separate claim/lease table.** Cleaner claim history and independent locking, but cross-row constraints/transactions make “draft revision plus exclusive current claimant” easier to get wrong. Choose only if audit requires multiple historical claims.
- **Append-only event log as authority.** Strong audit/replay, but projections and conflict resolution add operational complexity; disproportionate for a small editable register draft. A later audit stream can be added without changing the row as the authority.
- **Local-only coordination or last-write-wins cloud copy.** Preserves offline behavior but cannot prevent two stations from resuming and checking out the same ticket. Keep local-only as the offline fallback, not the shared authority.

