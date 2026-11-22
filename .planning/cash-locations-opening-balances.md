# Cash locations and opening balances

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This file retains cash-location design and its domain-specific verification history.

## Work sequence

- [x] Ground current chart, immutable journal, till-shift, and source cash-origin behavior.
- [x] Compare cash-location subledger and journal-ledger designs with accounting/operations council.
- [x] Decide migration and cutover rules; avoid duplicating historical operational postings.
- [x] Implement the safe aggregate cash opening balance entry for the current accounting model.
- [x] Implement tracked physical cash locations, atomic transfers, source disbursements, and till-count/Z disposition together.
- [x] Verify migration 28 on a foreign-key-enabled SQLite fixture, Rust compilation/tests, and the frontend production build.
- [x] Persist immutable variance review acknowledgements and show pending shift/Z count differences in Settings.
- [ ] Review the cash-location settings and cashier Z-close flows in the authenticated live UI.

## Aggregate opening-balance slice verification

- [x] Migration creates one immutable store-scoped opening record and enforces that it matches the same-store adjustment journal, owner identity, cash debit, and equity credit.
- [x] Rust tests cover balanced posting, immutable/unique record constraints, matching idempotent replay, changed-payload rejection, and rejection of a cutoff after prior cash-on-hand journal activity.
- [x] `cargo fmt --check`; 63 Rust library tests pass.
- [x] Frontend production build passes. Lint has no new warnings; three existing effect warnings remain in Products, People, and Register.
- [x] Native Windows executable was built to `target-review/debug/smart-pos-desktop.exe` at the recorded milestone (SHA-256 `184426620A47461807F39BB768BA4B1175D929EEF2CED6B7C5520BE6CAE1EE2E`); generated binaries were removed in the 2026-11-01 cleanup and can be rebuilt from source.
- [x] Migration 28 smoke test with foreign keys enabled confirms seeded register location, one-time openings, matched transfer events, and immutable ledger rows.
- [x] Rust library suite passes with the new cash-location migration test; frontend production build passes.
- [x] Historical Windows build: `target-review/debug/smart-pos-desktop.exe`, 30,055,936 bytes, SHA-256 `E77B7E5E99CB6D6B7A16070DE5640ED30A1D331D14C25CD689C75C16FFD15886`; generated binary removed during cleanup on 2026-11-01.
- [ ] Authenticated live screen review remains open because there is no usable demo login; do not claim full UI review from build output alone.

## Current system boundaries

- Migration 28 adds persistent register, safe, petty-cash, and other cash locations, each with append-only opening and movement events. Location balances derive from those events and reconcile against account 1000 without posting a second GL opening journal.
- Opening a register requires an initial tracked count. Its entered float is compared to the till balance; a difference becomes a pending variance event. Cash sales, refunds, cash expenses, supplier cash payments, manual paid-outs, and paired safe-to-till/till-to-safe transfers now update the location ledger in the same transaction as their source.
- Z close saves expected cash, counted cash, difference, closing sweep, and retained till amount. Count variance adjusts the physical balance through an immutable pending event; a sweep is atomic with its paired safe event. Historical close reports remain unchanged.
- Isolated desktop workflow verification on 2026-10-30 completed one full cash cycle: a zero opening float, GH₵65 sale, exact GH₵65 close count, and GH₵65 sweep to Main safe with GH₵0 retained. The test used a temporary unique Tauri identifier so its Windows profile database was separate from the regular NimbleTill profile.
- Historical expenses and supplier payments remain `legacy_unknown`, and old shifts remain unassigned. Migration 28 does not infer any historical location or float.
- The Settings page now provides location setup, one-time opening counts, a tracked-versus-GL reconciliation summary, and manual same-store transfers. Expense and supplier payment forms choose an initialized non-register cash location for outside-register cash.
- Hosted cash drops now require a cashier to choose an initialized branch safe or cash location. Migration 0047 records the till outflow and destination inflow in the same transaction. The restricted PostgreSQL verifier passed the route and paired-event scenario; authenticated visual review remains open.

## Design questions for the council

Compare two viable architectures: (A) a cash-location subledger with its own immutable opening/transfer/disbursement entries reconciled in aggregate to GL cash-on-hand, including how register shifts participate; (B) per-account opening journal batches and references from each expense/payment to a configured location. Specify a cutover strategy for a store with existing transactional journals, to prevent prior sales/expenses and new opening snapshots from being counted twice. Decide how safe-to-till transfers and Z close carry-forward should work.

## Council decision and current release boundary

The accounting architect, finance operations, and cashier operations reviewers agreed that the real cash-location ledger must include tills as persistent locations, safe-to-till and cash-drop transfers, cash refunds, expenses and supplier payments, shift counts, retained float, Z snapshots, and variance disposition. Migration 28 and the local desktop implementation now cover that coordinated slice. Opening a shift never adds another asset.

The current release instead adds one owner-attested opening balance for aggregate account `1000` per store, with an immutable adjustment journal against opening-balance equity. It requires a cutoff date and reason, is audited and retry-safe, and does not touch any drawer shift, cash movement, or historical journal. The backend rejects a cutoff if the store already has a cash-on-hand journal entry on or before it; the owner must choose a true ledger-start date before prior cash activity. Do not treat this aggregate entry as location accounting; incorrect entries require a separate authorized adjustment.

Cash-location cutover remains explicit: preserve old records as legacy; count each physical location once; never infer balances from historical shift floats or free-form source notes; keep historical Z reports immutable; and reconcile physical total against GL without silently creating a journal. At the next shift opening, count cash already in the till. A physical float movement between safe and till is one confirmed, atomic transfer. Close records expected, counted, variance, actual closing transfer, and retained cash separately. One isolated seeded-demo cash sale and Z close has now been exercised end to end. The original Smart POS remains unavailable without its own working account, and broader live reviews for other roles, workflows, devices, and reports remain open.

