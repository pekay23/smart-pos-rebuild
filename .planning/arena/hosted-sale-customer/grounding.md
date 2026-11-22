# Hosted post-sale customer attachment

## Objective

Let a cashier associate a walk-in customer with a completed hosted sale after checkout, including creating the customer's name and phone/email at that point. It must not edit sale prices, tenders, stock, cashier identity, or receipt number.

## Current system

- Local desktop already has `attach_sale_customer`; its tests limit links to the cashier's own store sale and reject invalid/replaced links.
- Hosted checkout accepts `customerId` before commit and stores it on `hosted_sales`; the sale/report and checkout responses already serialize it.
- Hosted customer rows are tenant-wide and read under branch/station access RLS. Cashiers can create/read active customers at their assigned station; manager roles can read and manage tenant customers through their branch grant. A valid customer may have been created at a different branch in the same tenant.
- Hosted sales are forced-RLS. Current grants allow only `SELECT` and `INSERT`; the sale header is otherwise immutable. The current RLS policies expose a sale to owner/admin/manager grants scoped to its branch/station and to its cashier when that cashier owns the row.
- `HostedRegisterPanel` retains `lastSale` in memory and displays a print receipt. It already has a station customer search/create flow for the next checkout; that form is tied to the live cart and should not be reused to silently mutate receipt state.
- The API uses `authorize_catalog_scope` for tenant/branch/station checks, DB transactions set transaction-local `pos.*` context, and successful customer/supplier mutations write `audit_events` atomically.

## Type and module sketch

```rust
struct AttachSaleCustomerInput { customer_id: String }

sales::attach_customer(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    sale_id: &str,
    input: AttachSaleCustomerInput,
) -> Result<CommittedSale, SaleError>

async fn attach_hosted_sale_customer_endpoint(...) -> Response
```

The caller supplies a customer ID from the authorized station customer search or the result of station customer creation. The sales domain transaction locks the sale, verifies it is unlinked or already linked to this same customer, validates the customer is active and visible in this branch/station, updates only `customer_id`, appends one audit event, and commits. A unique single link means a retried identical request is a no-op; an attempt to replace a link conflicts. A new migration grants only `UPDATE(customer_id)` and adds forced-RLS `USING`/`WITH CHECK` policies. A cashier can link only their own sale; owner/admin/manager can link an unassigned sale within their existing scope.

The web API gets `attachHostedSaleCustomer`. The receipt UI gets its own post-sale customer search and create form, with independent selected-customer state; on success, it updates the receipt customer label and permits reprint. Linked sales display the linked customer without an edit action.

## Required verification

- Restricted PostgreSQL HTTP integration: cashier attaches an active customer to own sale; exact same-customer retry succeeds without a second audit event; another customer cannot replace it; another cashier cannot attach; manager can attach an unlinked sale in scope; cross-tenant and archived-customer cases fail; an active customer created at another branch in the same tenant remains linkable where station policy allows; sale total/tender and stock remain unchanged.
- API unit suite, migration idempotency verifier, Rust formatting, hosted web production build, OpenAPI YAML and all references.
- Browser visual review remains dependent on Work settings that currently disable local browser control.

## Arena rubric

1. Preserves financial sale immutability and allows only customer metadata to change.
2. Enforces cashier ownership, manager scope, tenant/branch/station boundaries in both API and RLS.
3. Is safe on retry and produces at most one audit event.
4. Supports a usable cashier flow after checkout, including creating a customer with contact details.
5. Keeps the public API and implementation surface small and verifiable.

## Phase checklist

- [x] Ground current desktop, hosted checkout, customer, authorization and RLS behavior.
- [x] Compare independent designs and challenge the chosen domain/security tradeoffs.
- [ ] Implement migration, API, web client and receipt workflow.
- [ ] Run relevant tests/verifiers and record evidence in the implementation tracker.
