# Hosted customer merge contract

## Chosen behavior

Only an owner, administrator or manager with `customers:manage` at the branch can merge records. The user chooses the source duplicate and the surviving target explicitly. Both must be active and visible in the branch, and the request must include each current revision plus an idempotency key. There is no automatic match or email/phone based merge.

The source becomes `merged`, points to the active target, and cannot be edited, archived, restored or merged again. A target with incoming merge aliases cannot be archived; it can later be merged into another active profile. The target profile is otherwise unchanged; contact fields and notes are not silently overwritten. A later merge may use a prior target as its source, so clients follow the stored pointer to the current active survivor. Merge events are append-only and audited. Exact retries replay the original result; changed requests conflict.

## Tradeoff review

- **Accounting/audit:** keep checkout customer IDs and receipt snapshots unchanged; the merge is an alias and must not rewrite transaction history.
- **Privacy/data quality:** require a staff member to pick the survivor; matching names, phone numbers or email addresses alone must never merge personal records automatically.
- **Store operations:** make the action manager-only and irreversible in the directory, require a visible target confirmation, and prevent archiving a survivor while duplicate aliases depend on it.
- **Security:** enforce scope and revisions in the API and database, persist a replay key/hash, and keep the alias event append-only.

An LLM council integration was not available in this environment, so these are explicit review perspectives and recorded product decisions, not separate model votes.

Historical sale customer IDs, checkout-time names and printed receipt snapshots are not rewritten. The merge is a customer-directory alias, not a correction to accounting, sale attribution or customer consent. Do not use it to transfer balances or retroactively combine reports until a separately designed customer-ledger policy exists.

## Security and data invariants

- Tenant, branch, active membership and manager role are checked by the API and forced-RLS policies.
- The RLS insert policy matches the append-only event to the already-merged source, active target and exact recorded revisions; a deferred database constraint rejects any merged source without that matching event at commit.
- Both source and target use optimistic revision checks and are row-locked in stable ID order.
- A self-merge, inactive/archived target, stale revision, already merged source, cross-tenant target or changed retry conflicts.
- Merge history permits select/insert only for the restricted API runtime; update/delete are not granted.
- The original source profile remains available to managers in the archived/merged view, with its survivor ID. Cashiers cannot view merged-source rows through that view.
- Sale and receipt rows remain unchanged; the API integration re-reads a receipt after merging and compares its historical customer-name snapshot.

## Verification

Migration `0034_hosted_customer_merges.sql` enables the merge relation with forced RLS and narrow runtime grants. The hosted PostgreSQL integration verifies create, exact retry, changed-request conflict, cashier denial, merged listing and preserved receipt snapshot. The full restricted migration verifier checks repeated application and asserts RLS and append-only grants.

## Follow-up boundary

Customer-scoped lifetime history, account credit, loyalty balances, consent records and customer-data deletion/export need their own explicit merge semantics. The current merge only affects directory deduplication and future customer selection.
