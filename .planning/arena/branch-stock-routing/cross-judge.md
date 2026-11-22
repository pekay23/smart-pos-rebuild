# Cross-judge: branch stock routing

## Scoring

Scores are 1–5; higher is better.

| Criterion | Candidate A: transfer lifecycle | Candidate B: canonical catalog and pickup |
|---|---:|---:|
| Branch stock vs shared station stock | 5 | 5 |
| Honest live/stale/offline availability | 5 | 5 |
| Customer redirect/pickup and internal transfer | 4 | 3 |
| Audit, accounting, concurrency and idempotency | 5 | 4 |
| Practical phased path from current repository | 4 | 4 |
| **Total** | **23** | **21** |

## Ruling

Use Candidate A as the base. It specifies a complete inter-branch movement lifecycle, including acceptance, source dispatch, destination receipt, partial receipt, discrepancy handling, expected revisions, idempotency, tenant checks and the rule that stock in transit is not sellable at the destination. Those controls are essential for inventory integrity and reports.

Graft Candidate B's explicit separation between **customer pickup reservation** and **internal stock transfer**. A customer may simply visit a branch that has stock; moving inventory is unnecessary in that case. A reservation needs its own expiry and collection lifecycle. Do not encode both intents as one transfer status machine.

Also graft Candidate B's canonical company product identity and branch assortment mapping. Store-scoped SKU is unique only within a store, so name matching or implicit SKU matching can select the wrong product. Existing exact SKU/barcode matches may seed mappings only when unambiguous; ambiguous rows need owner resolution.

## Rejections and required refinements

- Reject Candidate B's placement of transfer as a vague later module without defining dispatch, in-transit custody, receipt and discrepancy accounting. Use Candidate A's transfer lifecycle for that work.
- Reject presenting any local snapshot as live availability. Current installations have separate local SQLite databases and no shared synchronization authority. Until the server-backed stock authority exists, a remote branch result must be marked stale/unknown and cannot reserve stock.
- Keep branch stock as the inventory balance. Register stations are collection/contact points within a branch and share that balance; they must not appear as independent stock locations.
- Defer cross-branch payment, sale ownership and tax settlement. In the first release the customer purchases at the branch they visit, or a branch accepts an expiring pickup reservation; cashier at another branch does not take payment for it.

## Recommended implementation order

1. Add company-wide canonical product identity and an owner mapping/reconciliation flow for existing store-specific products.
2. Build tenant-scoped server availability with `onHand`, reservations, outbound commitments, `availableToPromise`, branch contact details, `observedAt` and freshness. Enforce authorization on the service.
3. Add cashier branch discovery and a customer redirect action; show quantity and freshness and provide a refresh path.
4. Add expiring pickup reservations with atomic available-to-promise allocation, idempotency, audit and release on expiry/cancel.
5. Add explicit internal transfers with source acceptance, dispatch stock-out, transit status, destination receipt stock-in, partial receipt and discrepancy resolution.
6. Add admin settings, role permissions, event notifications and multi-branch operational reporting; test offline, concurrent request, duplicate retry and stale revision cases.

The current desktop SQLite application can prepare the UI contract and schema design, but it cannot safely implement authoritative cross-device stock checks or requests. The API/shared sync foundation is a prerequisite for enabling the cashier actions.
