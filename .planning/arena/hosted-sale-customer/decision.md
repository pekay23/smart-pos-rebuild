# Decision: hosted post-sale customer attachment

## Chosen shape

Use an append-only `hosted_sale_customer_links` relation for customer associations recorded after checkout. Keep `hosted_sales.customer_id` as the checkout-time value. A sale can acquire at most one post-sale link, and only when the sale was completed and checkout did not already name a customer. Customer IDs and timestamps are retained in the association; there is no update or delete grant. Record one `sale_customer_linked` audit event in the same transaction.

Expose a station-scoped, idempotent `PUT .../stations/{station_id}/sales/{sale_id}/customer`. It returns `{saleId, customerId, customerName}`. The customer name is nullable on retry if the customer was later archived and the actor can no longer read that name. Same-customer retry succeeds without another audit event, even after archival. A different association conflicts. Customers remain tenant-wide, so an active customer created at another branch may be attached when visible through the target station's RLS; cross-tenant and archived-at-time-of-new-link customers fail.

The API authorizes `customers:write` for the requested station. Forced RLS repeats the active member, tenant/branch/station, sale status, cashier ownership or manager scope, and active visible-customer checks. Runtime grants are limited to the link table's required select/insert columns. The implementation takes a sale-scoped advisory transaction lock before checking sale and link rows, then inserts only if eligible. This avoids `SELECT FOR UPDATE` behavior under an UPDATE policy and gives identical retries a fresh read without mutating the sale row.

The completed receipt owns its separate customer search/create state and captures its target sale ID. The live cart's customer selection remains independent. After customer creation, retain the returned ID and retry only the link request. A bounded per-account pending reference may retain only scope, sale ID, receipt number, and selected customer ID; never phone/email. A successful async result updates the receipt only if the target sale is still selected. Durable sale history, name snapshots, fiscal reprints, and manager correction/reassignment remain distinct follow-up work.

## Synthesis record

Three independent designs were reviewed by domain/accounting, security/RLS, and cashier UX/API personas. The cross-judge recommends Candidate 1's append-only association as the base. Grafted Candidate 2's scope rules, ordinary-RLS-read retry handling, customer archival behavior, and least-privilege checks, plus Candidate 3's receipt-owned dialog, stable target, create-then-link retry and stale-result guard. Rejected sale-column mutation because it rewrites checkout-time customer data; rejected `SELECT FOR UPDATE` under a null-only update RLS policy; rejected an atomic create-and-link endpoint because the existing idempotent customer creation API plus a stable returned ID has a smaller recovery surface for this slice.

## Verification plan

Restricted PostgreSQL HTTP coverage will prove own-cashier success, manager-in-scope success, cashier/branch/station denial, cross-tenant denial, same-tenant cross-branch acceptance, active customer requirement, archived same-customer retry, different-customer conflict, one audit row after replay, and unchanged sale/tender/stock/drawer values. Also verify forced RLS and least-column grants on the new relation, 30-migration idempotency, API unit suite, Rust formatting, hosted web build and all OpenAPI references. Browser visual review remains unavailable under current Work settings.
