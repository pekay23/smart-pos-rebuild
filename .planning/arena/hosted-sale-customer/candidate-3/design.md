# Receipt-owned customer attachment

## Usage

The completed-receipt card shows the receipt number, total, Print, and Add customer. Add customer opens a dialog titled `Add customer to receipt R-123`. The dialog belongs to that receipt even if the cashier has begun another ticket. It shows the receipt time and total so the cashier can check the target before confirming.

The dialog has two modes. Find customer searches the station's existing customer API by name, phone, or email and ends with Link customer. Add customer collects name and optional phone/email, then offers Create and link to receipt R-123. That action uses the existing idempotent customer-creation API, retains the returned customer ID, and sends the association request. If only the second step fails, show `Customer saved. Retry link to receipt R-123.` Creating a customer does not select that customer for the next ticket.

```ts
const target = { saleId: receipt.id, receiptNumber: receipt.receiptNumber, stationId }
setCustomerDialog({ kind: 'choosing', target })

const customer = await createStationHostedCustomer({
  ...location,
  customer: { name, phone, email, notes: null },
  idempotencyKey: creationAttempt.key,
})
// Retain customer.id if the next request fails. Retry the link, not creation.
setCustomerDialog({ kind: 'ready', target, customer })

const attachment = await attachHostedSaleCustomer({
  ...location, saleId: target.saleId, customerId: customer.id,
})
setLastSale(current => current?.id === target.saleId
  ? { ...current, customerId: attachment.customerId }
  : current)
```

`PUT /v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales/{sale_id}/customer`

```json
{ "customerId": "customer-uuid" }
```

The server returns `200` with the canonical association after a new link or an identical retry. `customerName` is nullable because a customer may be archived between the original write and its retry; the link remains valid and the retry must still succeed.

```json
{
  "data": {
    "saleId": "sale-uuid",
    "customerId": "customer-uuid",
    "customerName": "Ama Mensah"
  }
}
```

The completed receipt then shows `Customer: Ama Mensah` and retains Print. It has no replace or remove action. Customer attachment success says `Ama Mensah was linked to receipt R-123.` Receipt-specific errors stay in the dialog, not in a message that could be mistaken for the next ticket's checkout status.

## Shape

Use one new association endpoint and the existing customer search/create endpoints. `PUT` makes repeated assignment to the same subordinate resource an explicit API property. The server serializes competing links by locking the sale row. It allows an unlinked completed sale to acquire one active visible customer, returns the same association for the same customer, and rejects a different customer with `409 sale_customer_already_linked`. Only the first successful change writes `sale_customer_linked` to the audit table.

The endpoint authorizes the existing `customers:write` permission at the requested station. RLS additionally limits the sale to its tenant, branch, station, and active member. Cashiers may attach only to their own completed sale. Owner/admin/manager grants may attach within their existing scope. Managers do not need a transacting cashier lease for this metadata action. The runtime receives only `UPDATE(customer_id)` on the sale header; it receives no new authority over amounts, status, tenders, lines, or stock.

```rust
// Parsed request type at the HTTP boundary.
struct AttachSaleCustomerInput { customer_id: String }

// Canonical result for both the first successful write and a no-op retry.
struct AttachedSaleCustomer {
    sale_id: String,
    customer_id: String,
    customer_name: Option<String>,
}

async fn attach_customer(
    pool: &PgPool,
    actor: AuthorizedStationActor<'_>,
    sale_id: Uuid,
    customer_id: Uuid,
) -> Result<AttachedSaleCustomer, AttachCustomerError> {
    // not implemented
    // Set transaction-local identity/tenant/member/branch/station context.
    // Read the authorized sale FOR UPDATE.
    // If its customer is the requested customer, return the existing result.
    // That no-op succeeds even if the customer was later archived; include its
    // name only when current customer RLS permits it.
    // If it has another customer or is not completed, return a conflict.
    // Read the active customer under customer RLS.
    // Update customer_id only; append one audit event; commit.
}
```

`AuthorizedStationActor` illustrates the trusted input the operation needs. Reuse the existing authorization result if introducing a new actor type would only wrap identical arguments. The interface should hide transaction context, RLS-sensitive queries, concurrency, and audit handling inside the sales domain operation, per boundary-discipline. The caller coordinates customer creation only because that existing operation is also useful independently.

The receipt dialog owns its target and its operation state. Keep that state outside the cart's `selectedCustomerId`, `customerAttempt`, and global checkout `busy` value. A discriminated state avoids showing a creation error while retrying a known customer's link.

```ts
type ReceiptTarget = Pick<HostedSale, 'id' | 'receiptNumber' | 'createdAt' | 'totalMinor' | 'currencyCode'>
type ReceiptCustomerState =
  | { kind: 'closed' }
  | { kind: 'choosing'; target: ReceiptTarget }
  | { kind: 'creating'; target: ReceiptTarget; attempt: CustomerCreationAttempt }
  | { kind: 'ready'; target: ReceiptTarget; customer: HostedCustomer }
  | { kind: 'linking'; target: ReceiptTarget; customer: HostedCustomer }
  | { kind: 'unconfirmed'; target: ReceiptTarget; customer: HostedCustomer; message: string }
  | { kind: 'linked'; target: ReceiptTarget; customerId: string; customerName: string | null }
```

The actual implementation can keep a smaller state union if creation fields live in a separate form component. It must still carry the target sale ID in every in-flight operation. Apply success to `lastSale` only when the IDs match. If another checkout replaced the card, show a receipt-specific success confirmation and leave the new card untouched.

## Retry and recovery

Customer creation already has an idempotency key. Keep the same key and exact input until the creation result is known. Once creation returns an ID, preserve that selected customer through link failures. A Retry link action repeats only the association request. Never clear the selected customer or silently issue another create on a transient attachment error.

Before sending the association request, save a bounded pending record containing tenant, branch, station, authenticated subject, sale ID, receipt number, and customer ID. This record needs no phone or email. Use the same account-scoped storage convention as the register draft and remove it after confirmed success or a definite permanent rejection. On refresh, the register can offer `Finish customer link for receipt R-123`; repeating the same PUT resolves an unknown result without a new receipt-read endpoint. Clear or hide the pending action after an identity/tenant/station change, and parse stored data as untrusted input.

A creation response lost before the customer ID is known is a different state. The creation key and original form data must remain available while the page stays open. If durable recovery across refresh is required, extend the existing creation API with an authenticated idempotency-result lookup, or use a single atomic create-and-link command. Storing customer phone/email in the shared terminal's persistent browser storage solely to recover this operation is a poor default.

There is a UX choice the grounding does not resolve: the current receipt is held only in memory, and the hosted client has no durable receipt lookup/reprint workflow. This slice can recover a pending association by sale/customer IDs, but it cannot promise complete receipt recovery after refresh. Keep receipt history/reprint on the roadmap and state this limitation in acceptance evidence.

## Module map

- `apps/api/src/sales.rs` owns the one-time link transaction and result type.
- `apps/api/src/lib.rs` owns the route, input parsing, existing station authorization, and stable error mapping.
- A new PostgreSQL migration adds the update policy; runtime grants expose `customer_id` only.
- `apps/web/src/api.ts` parses the association response and exposes `attachHostedSaleCustomer`.
- A small `ReceiptCustomerDialog` component owns target, search/create/select, retry, and field errors. `HostedRegisterPanel` opens it with the target receipt and applies the result only to that sale.
- OpenAPI and restricted PostgreSQL HTTP integration define and prove the contract.

## Alternatives considered

An atomic create-and-link endpoint accepts either an existing customer ID or new customer details and creates both records in one transaction. It hides the two-step creation/link recovery from callers and prevents an unlinked new customer after failure. It requires a larger request union, endpoint-specific idempotency storage, and customer creation inside a caller-owned transaction. I would choose it if the product requires a one-button Create and link flow that survives refresh without storing customer details locally. For this implementation, reusing the existing idempotent customer API and retaining the known ID gives a smaller change with clear partial-success handling.

A broad `PATCH sale` endpoint could attach a customer beside other future sale edits. It exposes a mutable sale resource and makes future additions likely to expand financial editing authority. A dedicated customer association resource is easier to authorize and verify.

Reusing the checkout customer selector is the smallest UI change, but its selected customer belongs to the new cart. It would make a user action depend on which sale is currently stored in `lastSale` and could change the next ticket during receipt work. Keep a separate receipt-owned form.

## Tradeoffs accepted

- We accept that creating a customer and attaching it use two transactions. The UI must preserve a successful creation result and offer retry of the link.
- We accept a narrow pending-link storage record in exchange for recovering association requests after reload without storing contact details.
- We accept that the canonical customer name can later change in the directory. This endpoint does not establish an immutable customer-name snapshot for fiscal receipts; that requires a separate receipt identity policy.

## Verification criteria

Use restricted PostgreSQL HTTP checks for ownership, branch/station boundaries, active-customer visibility, one new audit event, identical retry, conflicting replacement, competing requests, and unchanged sale/tender/stock values. In the browser, start a new ticket, open Add customer on the previous receipt, create/search and attach, and confirm the live cart's customer and items stay untouched. Also verify a stale response cannot modify a newer receipt card, and a failed link retries the same customer without creating a duplicate.

## Next implementation step

Build and verify the dedicated association transaction and endpoint, then add the receipt-owned dialog with explicit creation-success/link-failure recovery.
