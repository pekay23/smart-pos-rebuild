# Receivables domain capability candidate

## Usage: caller first

The cashier uses the existing signed sale endpoint. The request adds `onAccountMinor` (default `0`) and `expectedCreditTermRevision` (required when the amount is positive). A fully on-account sale has an empty `tenders` array. A mixed sale provides only money actually recorded as tender. The branch currency, customer credit limit, due days, invoice date and actor are derived on the server.

```json
{
  "customerId": "customer-uuid",
  "items": [{ "productId": "product-uuid", "quantity": 2 }],
  "tenders": [{ "paymentOptionId": "cash-option-uuid", "amountMinor": 4000, "reference": null }],
  "onAccountMinor": 6000,
  "expectedCreditTermRevision": 3
}
```

Inside the existing `sales::commit` transaction, after authenticating the station proof, resolving exact retries and pricing the basket, the caller persists the sale with its on-account amount and invokes one domain operation:

```rust
// Existing sale transaction owns station proof, sale, stock, tickets and commit.
let invoice = match settlement.credit_request() {
    Some(credit) => Some(receivables::issue_for_sale(
        &mut tx,
        sale_id,
        credit.expected_terms_revision(),
    ).await?),
    None => None,
};
sale_accounting::post_sale(&mut tx, tenant, branch, membership, sale_id).await?;
// Existing audit, proof consumption and ticket completion remain in this tx.
tx.commit().await?;
```

The response stores and returns `onAccountMinor`, `paidAtSaleMinor` and a nullable invoice summary (`id`, `originalAmountMinor`, `issuedAt`, `dueAt`). Receipt rendering uses these persisted values. Current outstanding balance is a separate receivables query because it can change after a later collection.

The cashier may preview `GET .../stations/{station}/customers/{customer}/credit-position`. This returns only terms revision, currency, limit, outstanding amount and available credit for the selected customer. The preview never reserves capacity; the issuing operation checks again under a lock.

## Grounding and problem

The current `sales::commit` owns one PostgreSQL transaction covering proof and lease validation, exact sale replay, shared-ticket recovery, pricing, stock movements, tender insertion, accounting, audit and proof consumption. `sale_accounting::post_sale` independently reads immutable sale and tender snapshots and currently requires tenders to equal the full sale total.

Migration 0066 creates currency-specific tenant-wide terms plus append-only revision events. Its update policy permits tenant-wide owner/admin/manager actors. Migration 0067 creates immutable invoices and the `hosted_sales.on_account_minor` field, with deferred source-sale matching. Invoice reads are limited to the current branch and, for cashiers, the actor's own sales. Those policies cannot directly support a cashier's tenant-wide exposure sum or management-only term-row lock.

The domain needs a bounded capability that can inspect the selected customer's full exposure and lock its terms while ordinary runtime SQL retains its current branch visibility. A service that only wraps a `SUM` would leave authorization, locks and insert ordering scattered in checkout. This candidate gives the receivables boundary ownership of issuing a valid receivable from an already-authorized source sale.

## Types and signatures

```rust
// sales.rs: parse DTO fields at the transport/domain boundary.
enum SettlementIntent {
    PaidInFull,
    OnAccount { amount_minor: PositiveMinor, expected_terms_revision: PositiveRevision },
}

// receivables.rs: domain values contain no HTTP or wire DTO types.
struct IssuedReceivable {
    id: Uuid,
    original_amount_minor: i64,
    issued_at: DateTime<Utc>,
    due_at: DateTime<Utc>,
}

enum ReceivableError {
    CustomerUnavailable,
    CreditDisabled,
    TermsChanged,
    LimitExceeded,
    SourceSaleMismatch,
    Database,
}

async fn issue_for_sale(
    tx: &mut Transaction<'_, Postgres>,
    sale_id: Uuid,
    expected_terms_revision: PositiveRevision,
) -> Result<IssuedReceivable, ReceivableError> {
    // Call the bounded SQL capability; map structured database errors.
    todo!("design sketch")
}
```

`issue_for_sale` owns invoice authorization, terms, exposure, due-date derivation and invoice creation. It neither begins nor commits a transaction. Sale checkout owns the larger sale transaction. This is a knowledge boundary, not a public load/validate/save sequence; callers cannot obtain an approval and accidentally use it in a later transaction. The one-operation interface hides enough policy to justify a module (boundary discipline and domain modeling).

## Database capability and RLS

Add a narrow `public.pos_issue_sale_receivable(sale_id, expected_revision)` function. It reads customer, currency, original amount, issued time and cashier from the source sale; callers cannot provide replacement values. It checks the current identity, tenant, branch, station, membership and active cashier lease against that sale before doing credit work. It inserts and returns the invoice in the caller's transaction.

The function uses `SECURITY DEFINER` owned by a dedicated `NOLOGIN NOBYPASSRLS NOINHERIT` role, with forced RLS still enabled. The owner receives only the columns and operations needed for the function. Policies targeted to this role allow the selected customer/currency exposure query and term-row lock; ordinary runtime invoice reads remain branch-scoped. Give the helper role the column UPDATE privilege and matching RLS required for `SELECT ... FOR UPDATE`, without granting term updates or the helper role itself to the runtime login.

Revoke `PUBLIC` execution as part of function creation. Pin `search_path` to `pg_catalog` and fully qualify application relations. Provision the dedicated role and transfer function ownership before granting execution to `pos_api_runtime`; do not leave a callable function owned by the migration superuser. Add the role to Docker provisioning and the disposable verifier's role setup. Remove direct runtime INSERT permission on receivable invoices: every invoice must go through the capability, while a credit sale without an invoice fails the existing deferred constraint.

A separate read-only credit-position function uses the same authorization scope and returns a bounded aggregate. It never returns cross-branch invoice IDs, branch details, receipt numbers or cashier names. A read-only preview function does not lock terms. No request may supply a setting that disables RLS or selects a different database role.

## Issue transaction

1. Checkout validates the external amount and revision shape, station lease and signature using the existing path. The signed raw request covers the new fields. It resolves exact sale/shared-ticket retries before evaluating current credit availability.
2. Pricing enforces checked integer arithmetic: `sum(actual_tenders) + on_account_minor == total_minor`, with `0 <= on_account_minor <= total_minor`. No synthetic credit payment option or tender is created. An empty tender list is valid only when the whole positive sale is on account.
3. Persist the sale with its on-account amount. `issue_for_sale` reads that authorized source and locks its customer row for share, then its exact `(tenant, customer, currency)` term row for update. The helper needs the corresponding narrow locking privileges and policies. Customer status and term revision are rechecked after locking.
4. Compute authoritative exposure across every branch of the tenant in that currency. At the current schema milestone it is the sum of original invoice balances; once collections/credit adjustments exist it is original invoices less valid allocations and adjustments. The same service owns this formula and its read model. Use a wide SQL aggregate and compare safely without overflowing bigint arithmetic.
5. Reject absent or zero terms, stale displayed revision, inactive/merged customer, and `exposure + requested_credit > limit`. Derive invoice snapshots from the locked term. Derive `issued_at` from the sale and `due_at` from that timestamp plus the configured number of UTC 24-hour days, so database session timezone does not change the result.
6. Insert exactly one invoice for the source sale, return its immutable summary, and append a receivable-issued audit event in the same transaction. Existing deferred guards check the sale/tender/invoice equality at commit. Add snapshot checks against the corresponding append-only term event and the derived due date.
7. Continue sale lines, stock/tenders and journal work using the existing transaction. Any error, including audit insertion or deferred validation, rolls back the sale, invoice, stock, tender, ticket completion and proof consumption together. PostgreSQL sequence gaps, if any receipt numbering uses a sequence, are acceptable; no committed record is partial.

Terms revisions, later collections, adjustments and merge handling must share the order `customer -> term -> affected invoices ordered by id`. The existing station lock remains the first lock in a station financial operation. Manager-only operations do not acquire station locks afterward. This serializes cross-branch sales against the same capacity and makes a terms reduction race resolve against a definite committed revision.

## Journal and existing feature boundaries

`sale_accounting::post_sale` reads `on_account_minor` from the saved sale and verifies tenders plus receivable equal total. It debits actual tender clearing accounts, debits `1100` Accounts Receivable for the unpaid amount, and preserves revenue/tax/COGS/inventory behavior. Add the `1100` name/type mapping to hosted accounting reports; otherwise the journal can be balanced but mislabelled or omitted. Fully paid sales follow the same path with zero AR and preserve historical behavior.

A later collection posts Dr actual recorded clearing / Cr Accounts Receivable and reduces exposure only through immutable allocations. Customer-facing receipts show paid-at-sale and amount due, without disclosing the credit limit or other branches' activity. A manual bank/terminal reference retains its existing unverified settlement meaning.

Before credit checkout becomes publicly available, existing refunds and customer merges need explicit handling. The current merge leaves source customer identifiers on historical records. Until a debt-aware consolidation exists, reject merging a source with outstanding receivables under the same customer/term locks; otherwise its debt can disappear from the target's credit check. Existing refund code must not refund the entire credit-sale amount as cash. Implement AR-first compensating adjustments before enabling credit-sale refunds; until that path is ready, reject those refunds with a specific error. These are rollout prerequisites, not evidence that the full receivables feature is complete.

## Retry and error contract

The existing signed idempotency key and raw-body hash remain the only sale idempotency authority. An exact replay returns the stored sale and its invoice even if terms were subsequently reduced or disabled. A changed on-account amount or displayed revision changes the signed payload and conflicts with an already-used key. `UNIQUE (tenant_id, sale_id)` prevents an extra invoice on the same sale. The helper may defensively return an existing matching source invoice without reissuing credit; an inconsistent source fails.

Shared-ticket replay needs an explicit receipt-summary read path: the 0067 invoice policy's own-cashier predicate can hide an invoice from a second cashier even when the existing shared-sale replay authorizes returning the sale. Extend that narrow, proof-validated completed-ticket replay to the invoice summary; do not broaden normal invoice listing or call the issue operation as the second cashier. Include this in restricted-role acceptance before wiring the new receipt fields.

Use structured database error metadata for credit-disabled, terms-changed and limit-exceeded errors, mapped by `receivables.rs` to domain errors and by the HTTP endpoint to stable 409 responses. Do not parse PostgreSQL prose. A failed credit issue leaves the financial challenge unconsumed because the transaction rolls back; retry still requires a challenge valid under the existing proof lifetime.

## Module map

| Location | Ownership |
|---|---|
| `sales.rs` | Sale DTO conversion, signed checkout, replay, whole-sale transaction and response snapshots |
| New `receivables.rs` | Credit-position query, issue-for-sale capability call, typed financial failures; later collections/adjustments |
| New PostgreSQL migration | Narrow function, helper-role policies, invoice snapshot guards and privileges |
| Docker role provisioning + `grant-runtime.sql` | Dedicated function owner, minimal privileges and runtime execution grant |
| `sale_accounting.rs` | One balanced sale journal with actual tender plus AR debit |
| Hosted accounting/report module | `1100` asset classification and currency-specific reconciliation |
| Restricted PostgreSQL tests | Concurrency, raw-role authorization, atomic rollback, replay and journal evidence |

## Verification criteria

- Two different branches/stations racing the same final credit capacity commit at most one sale; the loser leaves no stock, invoice, proof, ticket or journal mutation.
- A cashier can issue credit for their authorized sale but cannot alter terms, directly insert a fabricated invoice, call the helper for another cashier's sale, or list cross-branch invoices. Tenant and currency boundaries remain enforced with direct restricted-role SQL.
- Full credit, mixed tender/credit and fully paid sales reconcile exactly; zero/negative/overflow amounts, unknown customer, inactive customer, missing terms, stale revision and exhausted capacity are rejected.
- Exact retries return one sale, one invoice, one stock movement per line and one journal after limit changes; changed payloads conflict. Shared-ticket retry from another station preserves existing sale ownership and invoice identity.
- Audit/journal insertion failures and deferred-constraint failures roll everything back. Cash drawer inflow equals actual cash tender only.
- A terms change or customer archive racing checkout has a serial outcome; merge cannot hide open debt. Credit-sale refunds cannot pay out uncollected AR.
- Reports classify `1100` as an asset; the receipt's paid and on-account amounts sum to the original sale total and due dates remain stable across session timezones.
- Execute as the real restricted runtime role, not the migration role, and inspect helper ownership, PUBLIC revocation, FORCE RLS and absence of role membership escalation.

## Tradeoffs and rejected alternative

This candidate accepts a narrowly scoped PostgreSQL capability and its role-provisioning work in exchange for keeping cross-branch financial rows inaccessible to general runtime SQL. The Rust interface remains one domain command with no mutable approval token and no separate transaction.

Rejected: a pool-owning `ReceivablesService::approve_credit()` followed by `sales::commit()` in another transaction. Its apparent simplicity exposes a temporal contract to checkout: capacity can change between approval and sale, and rollback/retry requires reservations, expiry and compensation. A reservation subsystem could be justified for offline deferred authorization, but the current hosted synchronous checkout does not need it.

Also considered: putting a customer-scoped broad invoice SELECT policy directly on the runtime role and embedding all credit SQL in `sales::commit`. That is less provisioning work, but every caller must correctly set and clear scope, sum exposure, obtain cashier-compatible locking privileges, derive snapshots and avoid leaking rows. The bounded domain capability concentrates those rules and can be reused by collections without duplicating them.

## Synthesis status

Independent candidate for comparison; no product code changed. Next implementation step: add the bounded database issue capability and restricted-role tests for aggregate scope and locking, then wire it into the existing sale transaction and AR journal.
