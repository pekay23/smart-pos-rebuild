# Cashier design comparison

I read both candidate documents completely. This review uses the supplied repository facts and judges the designs as written, rather than treating either design as verified implementation.

Choose Candidate B's durable workspace and intent-based caller as the base, with the reductions below. Its strongest advantage is that hold, resume and checkout each perform one atomic transition. Candidate A supplies the better store and employee handoff policy. B should adopt that policy without adding a second persistence model.

| Criterion | A | B | Evidence and consequence |
| --- | --- | --- | --- |
| Recovery after restart and payment reply loss | 4 | 5 | A atomically completes a persisted ticket and retains its sale ID, which is sufficient for duplicate-sale prevention. Its debounced saves leave a larger unsaved interval, and restore does not identify one active workspace. B acknowledges each basket edit only after persistence and atomically replaces the active basket when checkout commits. Its attempt lookup precedes revision rejection, which correctly handles a reply lost after commit. |
| Sale, stock, customer integrity and authorization | 4 | 4 | Both preserve `checkout_sale` as the atomic sale and stock authority and derive actor/store in Rust. B explicitly addresses cashiers lacking `sales:read` and limits receipt linkage, but leaves the exact recent-receipt predicate unsettled. A's post-sale attachment permits any unlinked sale in the store when the actor has `pos:use` and `customers:write`; it needs a narrower receipt policy. Both need an explicit price-review acknowledgement contract. |
| Store and cashier handoff fit | 5 | 4 | A explicitly lists store-wide working and held tickets with creator and last editor. B preserves the workspace across staff switches and records the checkout actor correctly, but adds terminal ownership despite the current product having one local database. Neither delivers cross-device sharing, and neither should claim it. |
| Zoom and layout usability | 3 | 5 | A establishes bounded flex/grid sizing and independent scrolling but leaves small-view payment behavior vague. B specifies reachable total/payment controls, short-view payment drawer, narrow-view catalog/ticket tabs, scrolling dialogs, focus behavior and concrete zoom checks. Those details directly address cashier controls disappearing at zoom. |
| Smallest coherent caller and maintainability for this MVP | 3 | 3 | A exposes save, restore, hold, discard and flush choreography to callers and requires a conflict policy for each. B gives callers a better action interface, but its complete design adds a terminal identity, document migrations, attempt rotation, fingerprints, contact union, principal generations and several modules. B's seven commands conceal substantial machinery. Neither earns a high simplicity score as written. |
| Total | 19 | 21 | The totals are close. The recommendation rests on atomic workspace transitions and fewer frontend lifecycle decisions. |

## Keep from B

- One Rust-owned register service that accepts validated actions. React should not assemble persisted workspace documents or coordinate separate hold/save/activate calls.
- Persist acknowledged basket mutations immediately through a serial controller. Keep pending/error state visible. Dirty contact or money form text may remain local until an explicit commit, provided navigation and payment await that commit.
- Atomic hold/resume with an explicit empty-or-swap rule. A failed transition must preserve the active and held baskets.
- Complete checkout and consume the active draft inside the existing sale transaction. Resolve a completed checkout identity before rejecting stale draft revision. Return the original receipt on retry without a second payment or stock movement.
- Treat card/mobile-money selection as recording an externally confirmed payment. Neither candidate supplies provider reconciliation or a guarantee against charging an external provider twice.
- Resolve an exact scanned code to a real catalog item and require selection when it matches multiple products. This improves on A's unqualified exact-code behavior.
- B's bounded viewport, narrow-screen tabs and short-screen payment behavior. Validate them in the actual desktop WebView with many cart lines and dialogs open.

## Graft from A

- Make the recovery workspace belong to the store in this local database and remain available to authorized successor staff. Carry creator, last editor and timestamps on active and held tickets. Do not hide a predecessor's unfinished sale behind employee ownership.
- Preserve durable completed-ticket-to-sale linkage for recovery. This can coexist with B's atomic active-ticket replacement without maintaining two different checkout identity schemes.
- Keep existing `save_customer` as the ordinary quick-create path unless transactional inline contact creation is required by the chosen UI. Reuse its validation and audit logic if checkout also creates customers. Do not implement two drifting customer creation policies.
- Keep missing/inactive product lines visible with actionable warnings until the cashier removes or corrects them. Never silently drop them during restore.

## Reduce or reject before implementation

1. Remove terminal UUID creation and terminal ownership from the initial design. A workspace keyed by the authenticated store in the existing local database meets the stated scope. Multiple windows, if supported, can contend on that same workspace revision. A future shared database or cross-device handoff requires an explicit product decision.
2. Use one stable checkout identity per draft lifecycle. Ticket ID plus a persisted successful sale mapping can provide idempotency here. Do not rotate a separate attempt ID after every cart edit unless a demonstrated requirement needs it. Completed retries must return their original receipt or reject mismatched input, never create a new sale. A cleared or replaced basket receives a new identity.
3. Specify the session-switch boundary, then implement the smallest backend synchronization that enforces it. Already admitted protected operations must finish under their original principal before replacement; queued requests from an outgoing principal must not inherit a successor's privileges. B's generation protocol is one solution, but should not become a general event system. A's instruction to serialize switching is too vague to implement safely without this contract.
4. Do not equate a JSON aggregate with fewer invariants. Keep schema versioning, bounded size and recovery on decode failure if using B's representation. A normalized repository can implement the same action interface later; callers must not depend on the document format. Reject B's suggestion that normalized tables inherently require caller coordination.
5. Keep customer capture proportional to the requested workflow. B's `DraftCustomer` variants and atomic customer creation add work beyond selecting or creating a customer through the existing API. If retained, include `customers:write` at checkout when creating the customer, not merely when editing the draft contact.
6. Do not expand PIN switching into employee administration beyond the minimum enrollment/reset flow. Both designs need real credential provisioning because there are currently no PIN fields or staff UI. Password-authenticated enrollment, hashed PINs, persisted throttling and fresh role loading are necessary parts of the feature; invented default PINs are unacceptable.

## Contracts still required

Both candidates say changed prices require review, but neither fully specifies how the backend recognizes acceptance. Define an observed quote or equivalent checked version. Checkout must reject an unseen price change, allow the cashier to accept the refreshed quote, and revalidate inside the sale transaction. A visible warning by itself does not establish this contract.

Define exactly which receipt a cashier can reopen or attach a customer to without `sales:read`. The current store's workspace-linked completed receipt is a practical starting point. Require `customers:write` for attachment and audit it. Do not introduce an undocumented time window or allow arbitrary store-sale IDs solely because the cashier has `pos:use`.

Recovery verification must cover a crash before commit, a commit followed by lost reply, repeated delivery, a fresh login after restart, failed save before switching, and a stale command racing with a profile switch. Assert one sale, one payment record and one set of stock movements after a successful retry. Also verify that clear-all leaves held work intact and that a failed resume/swap preserves both baskets.

The resulting design should have one register service, one frontend controller and the existing checkout authority. If implementation needs separate ticket, terminal, attempt and session coordinators, the simplification has failed.
