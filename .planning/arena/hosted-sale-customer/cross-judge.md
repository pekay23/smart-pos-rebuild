# Cross-judge: hosted post-sale customer attachment

## Rubric scores

Scores are 1–5, where 5 best satisfies the grounded requirement with the fewest correctness risks.

| Criterion | Candidate 1: append-only association | Candidate 2: guarded sale-column update | Candidate 3: receipt dialog + sale-column update |
|---|---:|---:|---:|
| Financial sale immutability | 5 | 3 | 3 |
| Cashier ownership and branch/station RLS | 4 | 5 | 3 |
| Retry safety and single audit event | 4 | 5 | 2 |
| Post-checkout create/search UX | 4 | 4 | 5 |
| Small, verifiable public surface | 3 | 5 | 4 |
| **Total** | **20** | **22** | **17** |

Candidate 2 has the smallest API/storage delta, but the rubric gives material weight to preserving the checkout record as written. The highest-confidence synthesis is Candidate 1's append-only association data model, with Candidate 2's RLS/authorization precision and Candidate 3's receipt-owned UX and recovery behavior.

## Findings

- **Candidate 1** best protects the checkout snapshot: it never adds runtime update authority to `hosted_sales`, and the unique `(tenant_id, sale_id)` association makes a later customer link explicit and one-way. Its main cost is a narrow extra table and an effective-customer projection. Its unique-conflict/re-read retry requires a fresh `READ COMMITTED` statement snapshot (or a sale-scoped transaction advisory lock) before deciding whether the winner attached the same customer. Add an explicit eligible sale-status rule and ensure that a hidden/cross-tenant sale is returned as not found, not as a conflict.
- **Candidate 2** has the strongest direct authorization sketch. Its ordinary SELECT under the existing read policy plus a sale-scoped advisory lock avoids the `SELECT ... FOR UPDATE`/RLS problem; guarded update and the `WITH CHECK` rule close the transition. However, this still changes the original hosted sale row and broadens the runtime's update surface. `customers:write` is an available scope permission; introducing `sales:customer:attach` is a useful future permission refinement but is not required to ship this slice.
- **Candidate 3** has the clearest caller and receipt workflow: pin the dialog to a captured receipt, keep it independent from the next cart, retain a created customer ID, retry only association, and guard against stale responses overwriting a newer receipt. Its proposed `SELECT ... FOR UPDATE` conflicts with its own UPDATE policy that exposes only rows whose `customer_id` is null. PostgreSQL locking reads require UPDATE access and apply UPDATE RLS, so a same-customer retry can disappear as a row-level-security-filtered result. Replace that with the concurrency/idempotency strategy selected for the base design.

## Synthesis decision

Use Candidate 1's new, forced-RLS `hosted_sale_customer_links` table and keep `hosted_sales.customer_id` as the original checkout-time association. Permit a post-sale link only when the sale was completed and had no checkout customer. Enforce one link per sale with a unique key; write the association and one audit event in one transaction; expose an identical retry as success without another audit event, including after the linked customer is archived. A different customer, ineligible sale, or disallowed role cannot replace the link.

Graft from Candidate 2: verify the tenant-wide customer model (a same-tenant customer created at another branch may be valid), require a customer active and visible for a new link, scope manager actions by the requested branch/station, and limit cashiers to their own sale. Keep `customers:write` for this first API rather than introducing a permission with no other caller. Use transaction-local identity/tenant/membership/branch/station context. Apply equivalent role and scope checks in the API and forced RLS, keep select/insert grants least-column, and never expose whether an out-of-scope sale exists.

Graft from Candidate 3: use a receipt-owned search/create form with a captured sale ID, distinct from the live cart customer selector; keep the created customer ID when association fails; retry only the link; scope pending browser recovery records to tenant, branch, station, and authenticated subject, with no phone/email; and apply an async result only to the matching receipt ID. Return a minimal canonical association with `customerName: string | null`; the customer can be archived after linking, so replay must not fail merely because the active-customer read is no longer allowed. A receipt name snapshot and durable cross-session receipt history/reprint are separate requirements and must not be claimed here.

Reject Candidate 2's `UPDATE(customer_id)` shape for this feature because the stored row is intended to preserve the checkout-time customer and the append-only association is a better audit/domain boundary. Reject Candidate 3's row-locking read under a null-only update policy. Do not rewrite the whole checkout/customer model around the new table; keep the existing checkout association and project the later link only where the receipt/API needs the effective customer. Treat manager correction/reassignment as a separately authorized append-only correction workflow, not an implicit replacement.

## Cross-branch and archived-customer behavior

Customers are tenant-wide, so an active customer created at another branch may be linked if the actor can see that customer under the target station's RLS context. A customer from another tenant must fail without disclosing sale/customer existence. An identical retry after archival succeeds from the durable association ID and may return `customerName: null`; a new association to an archived customer is denied.

## Verification implications

The restricted PostgreSQL verifier should assert forced RLS and least-column select/insert privileges, no runtime UPDATE/DELETE on the association table or sales table, correct tenant/branch/station policies, one audit row after exact retry, conflicting replacement denial, own-sale cashier success, other-cashier denial, manager-in-scope success, wrong-tenant customer denial, same-tenant cross-branch customer acceptance, archived-customer new-link denial, archived-customer retry success, and unchanged sale totals/tenders/stock/drawer. Also test concurrent same-customer and competing-customer requests. Web build checks the contract; browser acceptance remains separate.
