# Hosted customer attachment: security and RLS design

## Problem and grounded corrections

The feature adds customer metadata to a completed sale after payment. It must authorize the specific sale as well as the customer directory, preserve financial values, and behave safely when a network response is lost or two staff members act concurrently.

The current schema has forced RLS on `hosted_sales` and `hosted_customers`; the runtime can select/insert sale headers but cannot update them. Sale reads already allow scoped owner/admin/manager access and cashier ownership. Customers are tenant-wide, exposed through authorized branch/station context. A customer created through branch B can therefore be valid at branch A. The grounding's proposed “cross-branch/customer denial” assertion should mean wrong-tenant or invisible customer denial; denying a same-tenant customer solely for its creation branch would contradict the existing model.

## Usage

```http
PUT /v1/tenants/{tenant}/branches/{branch}/stations/{station}/sales/{sale}/customer
Authorization: Bearer ...
Content-Type: application/json

{"customerId":"..."}
```

```typescript
const customer = await createStationHostedCustomer({ ...scope, customer: details, idempotencyKey })
const link = await attachHostedSaleCustomer({ ...scope, saleId: receipt.id, customerId: customer.id })
// Set receipt metadata only after server confirmation. Keep the new cart independent.
```

The mutation's public result is deliberately narrow:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachSaleCustomerInput { customer_id: String }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaleCustomerLink { sale_id: String, customer_id: String }

enum AttachCustomerError { InvalidInput, NotFound, Conflict, Database }

async fn attach_customer(scope: AuthorizedStationScope, sale: SaleId, customer: CustomerId)
    -> Result<SaleCustomerLink, AttachCustomerError> {
    // not implemented: transaction-local scope, sale lock, read, guarded update, audit
}
```

The actual repository may keep its established positional service signature; the key boundary is that identity/membership/scope are server-authorized inputs and the client supplies only the target customer. Do not reuse `CommittedSale.replayed` to describe this metadata operation. A small link response avoids implying that sale creation has happened again and avoids making the browser replace receipt amounts with a loosely related response.

## Recommended shape

Keep `hosted_sales.customer_id` as the single authoritative association and permit one transition from null to a non-null ID. Add only `UPDATE(customer_id)` to the runtime grants, plus an explicit update policy. Keep all price, tender, status, cashier, lease, station and stock columns outside the runtime's new privileges.

Prefer a dedicated permission such as `sales:customer:attach`, granted to the current owner/admin/manager/cashier baseline roles. `customers:write` currently means customer creation and is a weak semantic gate for editing sale metadata. In either case, the database remains responsible for cashier ownership and scope. Manager access to this metadata action does not grant checkout permission or require temporarily becoming a cashier.

The service uses a sale-scoped transaction advisory lock, then an ordinary RLS `SELECT` constrained by the exact tenant, branch, station and sale ID. The read comes before the new customer's active-status check. If the visible completed sale already has the requested customer ID, return the link unchanged and do not emit another audit event. If it has a different customer, return conflict. If it is invisible, missing or not completed, return a uniform not-found result without disclosing its customer or cashier.

For a still-unlinked sale, verify that the requested customer is active and visible under this request's tenant/branch/station context. Execute a guarded `UPDATE ... WHERE customer_id IS NULL AND status='completed' RETURNING id, customer_id`, append `sale_customer_linked` with the current actor and sale/customer IDs, and commit. A failed audit insert rolls the update back. The database guard is retained even though the advisory lock serializes the application path.

Avoid relying on `SELECT ... FOR UPDATE` when the update policy's `USING` condition is `customer_id IS NULL`. Locking reads require update access, so that shape can hide already-linked rows and break exact retries. The ordinary read plus advisory lock avoids this interaction while the guarded update and RLS still enforce the transition. A restricted-role integration test must cover a second identical request.

The update policy should require all of the following in its `USING` expression: exact context tenant/branch/station; completed status; null customer; active current membership tied to `pos.identity_id`; and a covering owner/admin/manager grant or a covering cashier grant whose membership equals the sale's `cashier_membership_id`. Include active tenant and active branch/station eligibility or derive it through the existing RLS-visible parent joins. Its `WITH CHECK` should repeat the actor/scope/status rules and require a non-null active, visible customer belonging to the sale's tenant. Never omit `WITH CHECK`: PostgreSQL otherwise reuses `USING`, whose null-customer condition would reject every successful link.

Use transaction-local context only and do not accept identity, membership, tenant scope, role or actor overrides in the body. Current RLS context variables are set by the trusted API; RLS is a second query boundary, not a claim that arbitrary SQL running under the runtime credential cannot spoof those variables.

## Retry and concurrency behavior

- Same target customer twice: both requests return the same minimal link; exactly one mutation audit exists.
- Two different customers concurrently: one wins; the other conflicts and never replaces the winning link.
- Same-customer retry after archival: succeed from the already-authorized sale's persisted link before querying active customers. A valid successful operation must not become an apparent failure because its customer was later archived.
- Customer archival overlaps a new attachment: the update statement rechecks current customer eligibility through RLS. Subsequent archival is allowed and cannot remove the recorded link. Do not add a customer locking read that accidentally demands customer-update RLS from a cashier.
- Authentication or role removal between requests: fail under the current actor's permissions; idempotency does not override revocation.

## UI and privacy requirements

Give the completed receipt its own search/create form and state. Do not bind it to the live cart's customer selector. Persist the created customer's ID in the form immediately after successful customer creation, then retry attachment with that same ID if the second request fails. Otherwise a two-request create-and-attach flow can repeatedly create walk-in records after a lost attachment response.

Keep the customer-creation idempotency key stable across an uncertain response; changing details intentionally creates a new attempt. Show the receipt number and selected customer before confirmation because the one-time link is not editable by this endpoint. Do not promise email/WhatsApp delivery merely because a contact detail was entered. Do not log phone/email or include contact details in the attachment audit; the customer record owns those details.

The linked name displayed on the receipt should come from the confirmed selected customer and be retained in receipt state, rather than depending on whichever customer search result page is currently loaded. Current hosted sales do not persist a customer-name snapshot, and `lastSale` is only in memory. Historical receipt lookup/reprint and persistent historical names remain distinct gaps; this slice must not claim to resolve them.

## Alternative considered

An append-only `hosted_sale_customer_attachments` table keyed uniquely by `(tenant_id, sale_id)` can keep the original sale header fully immutable. It would store customer ID, name snapshot, actor and timestamp, with select/insert-only grants, forced RLS, and no update/delete. Reads would expose `COALESCE(hosted_sales.customer_id, attachment.customer_id)`. A unique constraint makes competing links mutually exclusive, and the attachment row itself provides a durable event.

That is the stronger choice if an immutable original customer snapshot and a full association event model are required now. It adds joins to every customer-aware sale/receipt read and creates a precedence rule between checkout associations and later associations. For the existing nullable field and one-time requirement, the guarded column update concentrates the behavior behind a smaller API while retaining financial immutability. Reconsider the table before supporting corrections or reassignment; do not broaden this endpoint to arbitrary replacement.

## Required adversarial verification

Use real restricted PostgreSQL HTTP tests for owner cashier success, same-target retry with one audit, conflicting replacement, two competing target customers, another cashier's sale hidden, manager scope, wrong branch/station path, wrong-tenant customer, active same-tenant customer created through another branch accepted, inactive customer rejected for a new link, same-target retry after archive, completed-status requirement, and pooled context reset. Verify the runtime cannot update sale totals/status/cashier/station or delete the sale. Check the sale total, tender rows, stock movements and drawer expected amount before/after linking.

The most important changes to the grounding contract are the narrow mutation result, stable created-customer ID during partial failure, ordinary read under an advisory lock, and correct tenant-wide customer semantics.
