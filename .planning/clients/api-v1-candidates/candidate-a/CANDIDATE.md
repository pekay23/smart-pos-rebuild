# Candidate A: stateless sale calculation service

**Status:** architecture proposal only; no service or client code exists yet.

## Caller first

Start the proposed service from the repository root:

```powershell
$env:POS_API_BIND = '127.0.0.1:8787'
cargo run -p pos-api
```

From any browser-native client, CLI, or test harness, request a versioned calculation:

```sh
curl -sS http://127.0.0.1:8787/v1/sale-calculations \
  -H 'content-type: application/json' \
  -d '{"lines":[{"quantity":2,"unitPriceMinor":1250},{"quantity":1,"unitPriceMinor":500}],"discountMinor":100,"taxRateBps":1500}'
```

Example response:

```json
{
  "data": {
    "subtotalMinor": 3000,
    "discountMinor": 100,
    "taxMinor": 435,
    "totalMinor": 3335,
    "lines": [
      {"lineSubtotalMinor":2500,"discountMinor":83,"taxMinor":363},
      {"lineSubtotalMinor":500,"discountMinor":17,"taxMinor":72}
    ]
  }
}
```

This endpoint is a calculation helper. It computes only from values submitted in that request. It does not read catalog prices, tax settings, user identity, or tenant records, and its result is not an authoritative quote, checkout, tax determination, or persisted sale. The client must not treat caller-supplied prices or tax rate as trusted business configuration.

## Concrete call sites

1. **Desktop register ticket preview:** replace duplicated preview arithmetic in the register UI with a call through a small HTTP adapter. Keep the existing Tauri `checkout_sale` command as the only current sale-commit path; this service does not authorize or persist desktop checkout.
2. **Future browser management/cashier client:** use the same request/response contract for a basket preview without linking a browser build to Tauri. A later authenticated catalog/checkout API is required before this becomes a live tenant workflow.
3. **Contract and compatibility harness:** CLI/contract tests can exercise the exact JSON and rounding behavior without starting the desktop shell. This exposes shared arithmetic to platform clients while `pos-core` remains the calculation authority.

## Wire contract (`/v1`)

- `GET /healthz` returns only `{ "status": "ok" }`; no environment, secrets, database status, or host details.
- `POST /v1/sale-calculations` accepts JSON below and returns `200` with `{ "data": SaleCalculationResponse }`.
- `400` is used for malformed JSON or invalid calculation input; return a stable `{ "error": { "code", "message" } }` envelope. Do not expose Rust debug text or internal errors.
- Limit request body size and number of lines (proposed initial limit: 500). Reject unknown or out-of-range inputs deterministically. Version-breaking wire changes require `/v2`; additive optional response fields may be added within `/v1`.
- Money is integer minor units. Quantity and all values are signed 64-bit JSON integers; API serialization must reject values outside the JSON safe integer range for browser consumers, or adopt decimal strings before any such range can occur. The initial implementation should enforce JavaScript safe integers for all returned amounts.

## Rust types and signatures

Transport DTOs live in the service crate and do not reuse local SQLite models or expose the `pos-core` structs directly:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaleCalculationRequest {
    lines: Vec<PricedLineInput>,
    discount_minor: i64,
    tax_rate_bps: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PricedLineInput {
    quantity: i64,
    unit_price_minor: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaleCalculationResponse {
    subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    lines: Vec<LineAllocationResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LineAllocationResponse {
    line_subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
}

fn calculate_sale(
    request: SaleCalculationRequest,
) -> Result<SaleCalculationResponse, ApiError>;
async fn post_sale_calculation(
    Json(request): Json<SaleCalculationRequest>,
) -> Result<Json<ApiEnvelope<SaleCalculationResponse>>, ApiError>;
```

`calculate_sale` maps input into `pos_core::PricedLine`, calls both `pos_core::calculate_sale_totals` and `pos_core::allocate_sale_components`, then maps the results to transport DTOs. The source domain signature today is `calculate_sale_totals(&[PricedLine], discount_minor: i64, tax_rate_bps: i64) -> Result<SaleTotals, CalculationError>`; preserve its checked arithmetic and rounding behavior. Map known validation failures to stable public error codes without leaking internal details.

## Module map

Proposed new workspace member `apps/api` (package/binary `pos-api`):

```text
apps/api/
  Cargo.toml                 # axum, tokio, serde, tracing, pos-core
  src/main.rs                # config, tracing, bind, graceful shutdown
  src/lib.rs                 # router assembly for unit/contract tests
  src/config.rs              # bind address and request limits; validated defaults
  src/http.rs                # health and /v1 routes, JSON limits, CORS policy
  src/sale_calculations.rs   # DTOs, mapping to/from pos-core, handler
  src/errors.rs              # stable error envelope/status mapping
```

Root workspace change: add `apps/api` to `Cargo.toml` members. No edits to `apps/desktop/src-tauri`, its SQLite schema, `pos-core` behavior, or production tracker are part of this candidate. The HTTP crate depends on `pos-core`; it must not depend on the Tauri crate.

## Authentication and tenant boundary

This first endpoint reads and writes no tenant/business data, so it has no tenant principal and requires no tenant cookie or token. It must not invent a tenant ID, claim the local installation account is a cloud identity, or imply that a successful calculation proves authorization. Health metadata is similarly public and non-sensitive.

Before adding any data-bearing endpoint, establish a separate cloud identity/session design and authentication middleware that constructs a server-derived `ActorContext` (user, business, store/branch scope, permissions, session ID). Every repository query and command must take this context and constrain tenant scope server-side; callers must never select tenant scope by supplying arbitrary business/store IDs. Add cross-tenant denial tests before exposing those routes. Prefer same-site secure browser sessions with CSRF protection for the future web client; native/CLI token strategy and refresh rotation remain decisions. Do not reuse desktop SQLite login/passkey artifacts as cloud credentials. TLS termination, deployment CORS allowlist, rate limiting, request size limits, and redacted structured logs belong in the runnable service/deployment boundary.

## Rationale

- Meets the independently runnable service and `/v1` contract goals with a feature users can actually call from desktop, browser, and CLI code.
- Reuses the only current platform-independent sale logic (`pos-core`) instead of copying arithmetic into HTTP handlers or TypeScript.
- Provides a bounded first slice without pretending cloud identity, durable tenant data, PostgreSQL, or sale persistence already exist.
- Creates an executable Rust boundary for OpenAPI generation and future platform clients. The HTTP contract becomes the candidate for an OpenAPI document before public client adoption.

## Alternatives and tradeoffs

| Alternative | Benefit | Cost / reason not selected |
|---|---|---|
| Health-only service | Smallest startup proof | Too little product value and does not exercise versioned domain DTOs or `pos-core`. Keep health as a secondary route only. |
| Static configured bearer token plus seeded in-memory tenant/catalog and product-list endpoint | Can demonstrate protected reads and a richer UI call | A single configured secret is not a real tenant identity/session system; in-memory catalog is misleading as POS data. Adds false confidence around authentication and business authority. Defer until explicit dev-only fixtures and the production auth contract are designed. |
| Implement Postgres + tenant auth + catalog and checkout immediately | Closest to eventual multi-user product | Too many unresolved decisions (identity provider, schema, branch scope, session/CSRF, idempotency, migrations, deployment) and current local use cases are not yet extracted. High chance of duplicating desktop behavior or claiming incomplete security. |
| Expose `checkout_sale` over HTTP using local SQLite | Reuses mature local workflow | Couples independently deployable API to desktop database/session assumptions and exposes local installation identity as cloud identity. Rejected. |
| Move mobile/desktop logic behind Rust FFI | Shared calculations offline | Does not produce an independently runnable HTTP service; mobile native extraction/security/reconciliation cost is not justified for this online-first slice. |

Tradeoff: this endpoint is not tenant-authenticated because it has no tenant data. It advances the API boundary and shared arithmetic, but does not satisfy the future cloud-auth milestone by itself. Avoid expanding the no-auth surface to catalog, reports, customers, or mutations.

## Open questions

- Should OpenAPI be generated from Axum DTOs (e.g. `utoipa`) or checked in as the authoritative contract with schema conformance tests?
- Which tax-rate and currency policy will supply authoritative configuration after authentication? Current `pos-core` accepts an integer basis-point rate but the endpoint cannot validate merchant/jurisdiction policy.
- Should browser-safe amount handling use JS-safe integers or decimal-string wire values from day one? Start with explicit safe-integer bounds; revisit before larger ledgers or currencies.
- Which cloud identity provider/session design, cookie policy, recovery, and tenant membership lifecycle will be approved before data-bearing API work?
- Should the endpoint retain pure totals plus line allocations, or should line allocation remain local until a consumer needs it?
- What deployment origins and rate limits are supported for local desktop, localhost browser development, and production clients?

## Exact intended implementation files and commands

If this candidate is selected, the bounded first implementation should add/change only:

```text
Cargo.toml
apps/api/Cargo.toml
apps/api/src/main.rs
apps/api/src/lib.rs
apps/api/src/config.rs
apps/api/src/http.rs
apps/api/src/sale_calculations.rs
apps/api/src/errors.rs
apps/api/openapi.yaml                 # if OpenAPI-first is selected
```

Proposed commands from the repo root:

```powershell
cargo check -p pos-api
cargo test -p pos-api
cargo run -p pos-api
curl.exe -i http://127.0.0.1:8787/healthz
curl.exe -i -X POST http://127.0.0.1:8787/v1/sale-calculations -H "content-type: application/json" --data-binary "{`"lines`": [{`"quantity`": 2, `"unitPriceMinor`": 1250}], `"discountMinor`": 0, `"taxRateBps`": 0}"
```

Tests should cover versioned JSON shape, totals and per-line allocation parity with `pos-core`, validation/overflow, stable error responses, body/line limits, health redaction, and that no tenant-data route is mounted. These are proposed verification commands, not run evidence.

## Next implementation step

Select or reject this candidate in the parent architecture decision. If selected, implement the service and tests above, then add a small desktop HTTP adapter for the ticket preview behind an explicit capability flag; do not reroute checkout mutations. Update the tracker only after build and CLI/browser calls pass. Keep tenant-data endpoints blocked on an approved cloud auth/session design, server-derived actor context, persistence choice, and cross-tenant authorization tests.
