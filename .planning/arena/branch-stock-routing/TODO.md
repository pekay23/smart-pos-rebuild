# Branch stock routing design work

- [x] Ground existing business, store/branch, register station, product and stock movement ownership.
- [x] Sketch candidate A: explicit transfer request and fulfillment lifecycle.
- [x] Sketch candidate B: availability discovery plus customer reservation/pickup workflow.
- [x] Cross-judge and select a design for one coherent tenant workflow.
- [x] Add the agreed workflow and infrastructure dependency to the product plan, platform plan and implementation tracker.
- [x] Verify architecture constraint: the current local SQLite installation cannot truthfully serve live stock or reserve/transfer stock across independent devices; no misleading local-only feature was enabled.
- [x] Implement tenant-wide canonical product identities, shared live availability API, and hosted inter-branch transfer request/accept/reject/dispatch/receipt lifecycle.
- [x] Implement hosted cashier transfer requests plus branch-scoped manager/owner transfer queue; API authorization, stale revisions, idempotent retries, and short receipt discrepancies were verified in restricted PostgreSQL integration.
- [x] Add web controls for adjusting accepted/dispatched/received quantities. Short receipts require an explanation and close as discrepancies.
- [ ] Add post-receipt discrepancy resolution and incremental receipt sessions; perform authenticated browser validation across cashier, manager and owner roles.

## Hosted progress

- [x] Cross-branch cashier lookup returns a live database snapshot with a server-generated UTC `observedAt`; the UI displays the localized check time and says the result is neither a reservation nor a transfer.
- [x] Add PostgreSQL transfer, line and append-only event schema with source/destination branch RLS, transfer permission baselines and restricted-runtime column grants.
- [x] Implement branch-manager accept/reject, source dispatch, and destination receipt endpoints. Dispatch and receipt update stock with matching movements; each action checks expected revision, supports exact idempotent replay, and writes its event and audit record transactionally. Short receipt is recorded as a discrepancy and credits only the received amount.
- [x] Run restricted PostgreSQL HTTP integration for request/list and the transition lifecycle. It verified owner management permissions, accept and exact replay, source dispatch and exact dispatch replay, short destination receipt recorded as a discrepancy, destination/source stock totals (11/1), two matching movements, request/transition event counts, restricted column grants, RLS and pooled context reset. The test also confirms requests themselves do not move stock.
- [x] Expose the transfer lifecycle in the hosted web workspace. Cashiers can submit a quantity request from availability lookup to a stocked branch; managers/owners can accept/reject, dispatch, and receive from a permission-gated queue. Web TypeScript and production build pass.
- [ ] Add branch contact/operating-hours data, available-to-promise reservations, customer pickup holds, incremental receipt sessions, discrepancy resolution, and owner/admin workflows. Keep the lookup timestamp as the freshness boundary; never label a cached/offline result live.
