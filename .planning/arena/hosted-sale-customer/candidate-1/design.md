# Candidate 1: immutable post-sale customer association

## Problem

A walk-in customer may give contact details after a sale is complete. The hosted sale header records the checkout request and financial snapshots, and the runtime currently cannot update it. Customer association is operational metadata that may be added later, but must remain attributable and must not rewrite what checkout recorded. Requests can be retried after a network failure or race with another staff member. The customer directory is tenant-wide, with visibility controlled by the actor's branch/station grant; it is not a set of branch-owned customer records.

## Usage (caller's view)

The register searches for an active customer or creates one using the existing station customer API. It then sends:

```http
PUT /v1/tenants/{tenant}/branches/{branch}/stations/{station}/sales/{sale}/customer
Content-Type: application/json

{"customerId":"<uuid>"}
```

The response supplies the effective customer ID and whether this call created the association. An exact repeat returns the same customer without a second audit event; a different customer returns a conflict. A cashier may act on an eligible sale they completed at an authorized station. An owner, administrator, or manager with the branch/station scope may attach to an eligible sale there.

```text
customer = searchStationCustomers(nameOrPhone).choose()
receipt = attachCustomerToSale(station, sale.id, customer.id)
showCustomerOnReceipt(receipt.customerId)

created = createStationCustomer({name, phone, email}, stableCreationKey)
receipt = attachCustomerToSale(station, sale.id, created.id)
// If attachment fails, keep created.id and the creation key for retry.

receipt = attachCustomerToSale(station, sale.id, customer.id)
// Same request after a lost response: same association, no duplicate event.
```

## Shape

Add a tenant-scoped `hosted_sale_customer_links` table for **post-checkout** associations. Its row contains `tenant_id`, `branch_id`, `sale_id`, `customer_id`, `linked_by_membership_id`, and `linked_at`; `(tenant_id, sale_id)` is unique. A composite foreign key to `hosted_sales(tenant_id, branch_id, id)` keeps the sale and branch aligned; tenant-qualified customer and membership foreign keys prevent cross-tenant links. The existing `hosted_sales.customer_id` remains the original checkout customer snapshot. It is not rewritten. The current effective association is `COALESCE(hosted_sales.customer_id, hosted_sale_customer_links.customer_id)` in sale projections. A link insert is legal only when the original checkout customer is null, so the two representations have distinct provenance and cannot compete.

```rust
struct AttachSaleCustomerInput { customer_id: String }
struct SaleCustomerAssociation {
    sale_id: String,
    customer_id: String,
    created: bool,
}
enum AttachCustomerError { InvalidInput, NotFound, Conflict, Database }

async fn attach_customer(
    pool: &PgPool, actor: SaleActorScope<'_>, sale_id: Uuid, customer_id: Uuid,
) -> Result<SaleCustomerAssociation, AttachCustomerError>;
```

The HTTP layer parses UUIDs and verifies `customers:write` at the requested station. The domain transaction sets the established transaction-local identity, tenant, membership, branch, and station context. It first resolves the visible sale and any existing effective customer. The same customer is an idempotent success even if that customer was archived after the original link; a different customer is a conflict. For a new link, it requires an active visible customer and an eligible sale status, inserts the unique link, and appends `sale_customer_linked` to `audit_events` in the same transaction. Use the unique constraint to serialize competing attachments: insert with conflict handling, then read the winner and compare under `READ COMMITTED`. Only the insert winner writes the audit event. Do not turn an authorization failure into a conflict that confirms another tenant's sale exists.

Force RLS on the link table. `SELECT` follows visibility of the parent sale. `INSERT WITH CHECK` requires the current active membership/identity, tenant, branch, sale station, actor, active visible customer, no checkout customer, and role scope: manager-level grants for their permitted branch/station, or the sale's own cashier with a qualifying cashier grant. Runtime privileges are restricted to needed `SELECT` and `INSERT` columns; do not grant `UPDATE` or `DELETE` on the link or `UPDATE(customer_id)` on `hosted_sales`. Audit writes and the link are atomic. The endpoint hides the transaction, scope checks, retry handling, and audit; callers provide only the sale and selected customer.

The register keeps customer-search/create state separate from the next cart. Once creation succeeds, retain the created ID when linking fails and offer **Retry linking**; retrying customer creation with a different key can otherwise create duplicates. Render an existing link without an edit button. Receipt reprints should clearly use the current association and should not imply an earlier printed receipt was retroactively changed. A separate versioned fiscal receipt is necessary before claiming historical receipt immutability in jurisdictions that require it.

## Synthesis decision

Pending comparison with the other candidates.

## Tradeoffs accepted

- We accept one additional narrow table and a read join in exchange for retaining checkout's original customer snapshot and an append-only, attributable later association.
- We accept a temporary one-way link in exchange for a small, auditable initial workflow. A future manager correction must be a separately authorized append-only correction with history; the local desktop already allows a sales reader to replace an existing customer, so an unconditional permanent ban would create a cross-platform behavior gap.
- We accept two requests when creating a walk-in customer, while retaining the returned customer ID across link retry. Customer creation is independently durable and must not be repeated after an uncertain response with a fresh key.
- We accept active-at-assignment validation using the transaction's database snapshot unless an explicit product rule requires serializing archival against attachment; that stronger guarantee needs a suitable lock/function design without granting cashier update rights on customer rows.

## Alternatives considered

- **Update `hosted_sales.customer_id` once.** A small endpoint and one row are easy for callers, but runtime `UPDATE` plus an update RLS policy changes the immutable checkout record. A `USING customer_id IS NULL` policy also filters already-linked rows from `SELECT FOR UPDATE`, so a same-customer retry cannot be handled by simply locking and reading through that policy. This option needs careful ordinary-read fallback and does not preserve the checkout-time customer snapshot by itself.
- **Move every checkout and post-checkout association to a new canonical link table.** This removes the `COALESCE` projection but requires a coordinated migration/backfill, rewrite of checkout inserts and replay paths, and removal or deprecation of the existing sale column and its RLS policy. It can be revisited as a deliberate schema migration; it expands the compatibility and accounting blast radius for this feature.

## Open questions and risks

- Should a future authorized manager be able to correct an incorrectly linked customer, as the existing desktop workflow allows? A one-way initial link must not be presented as the final correction policy.
- Is an immediate receipt-only control sufficient? `lastSale` is held only in memory, and the branch sales report denies cashiers. To satisfy later attachment after refresh or another sale, add a bounded station history/receipt lookup limited to the cashier's own eligible sales; managers need a scoped lookup as well.
- Should linking to a customer and archiving that customer be strictly serialized? A plain active-row check can race an archive under PostgreSQL's normal isolation; avoid asserting stronger semantics without a restricted-role concurrency test.
- Will fiscal receipt rules require the original issued document to retain its original customer details? Capture/version issued receipt data before calling regenerated output the original.

## Next implementation step

Add the immutable association migration and restricted-role integration cases, then wire effective-customer projection and the independent register flow.
