# Candidate B: protected use-case endpoint behind explicit runtime ports

## Problem

The selected multi-client architecture needs a runnable, versioned HTTP contract, but there is no cloud identity, PostgreSQL schema, API crate, or reusable desktop application service yet. The desktop `checkout_sale` command makes the local authority boundary concrete: it derives a local staff session, opens an immediate SQLite transaction, reloads and validates a register ticket, recomputes prices/tax/discount/payment, writes stock, cash, audit and accounting records, and commits atomically. `pos-core` currently supplies only pure sale arithmetic. The first service must therefore establish an honest network boundary without reusing local command models or pretending that a cloud principal exists.

## Usage (caller's view)

The example client imports only generated/versioned wire DTOs and uses ordinary HTTPS. These are intended call sites once the identity and persistence adapters are supplied; they are not runnable against the current repository.

```ts
import type { ProductPage } from "@advanced-pos/api-v1";

const page: ProductPage = await api.get("/v1/catalog/products?limit=40");
```

```ts
const page = await api.get<ProductPage>("/v1/catalog/products?cursor=eyJ...&limit=40");
renderProducts(page.items);
```

```ts
const result = await api.post<SaleResult>("/v1/sales", {
  idempotencyKey: crypto.randomUUID(),
  lines: [{ productId: selectedId, quantity: 2 }],
  payment: { method: "cash", amountTenderedMinor: 5000 },
});
showReceipt(result.receiptNumber, result.totalMinor, result.changeDueMinor);
```

The shared API client attaches a cloud bearer credential only after the separately planned cloud authentication flow exists. The server, never the request payload, determines the business and user scope. The sale call is shown to specify the eventual caller contract; sales are deliberately outside the first implementation slice.

## Shape

### First vertical slice

Implement a protected, read-only `GET /v1/catalog/products` endpoint in a new `apps/api` Rust binary. It proves versioning, request parsing, authenticated tenant scope, stable DTO serialization and application-to-storage adaptation with a small policy surface. A missing or invalid principal returns `401`; an authenticated principal lacking catalog-read permission returns `403`; storage failure returns a non-sensitive `500`. `GET /healthz` is a separate process-liveness response with no business or configuration data.

The service starts with a `PrincipalResolver` and `CatalogReader` contract. The endpoint is not exposed with an allow-all or hard-coded tenant implementation. Local development can run health immediately; the protected endpoint returns unavailable until a real identity and tenant-scoped storage adapter is configured. Do not seed demo business data into the service or call local desktop accounts cloud identities. This deliberately bounds the first deployable increment while keeping the meaningful protected call honest.

### Public and application types

```rust
// apps/api/src/http/v1/catalog.rs
#[derive(Deserialize)]
pub struct ProductListQuery {
    pub cursor: Option<String>,
    pub limit: Option<u16>, // default 40, maximum 100
}

#[derive(Serialize)]
pub struct ProductPage {
    pub items: Vec<ProductSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct ProductSummary {
    pub id: String,
    pub sku: String,
    pub name: String,
    pub price_minor: i64,
    pub currency: String,
    pub active: bool,
}

// apps/api/src/application/catalog.rs
pub struct ActorContext {
    pub business_id: BusinessId, // populated from verified server-side identity/session
    pub user_id: UserId,
    pub permissions: PermissionSet,
}

pub struct ProductPageQuery {
    pub cursor: Option<ProductCursor>,
    pub limit: NonZeroU16,
}

pub trait PrincipalResolver: Send + Sync {
    async fn resolve(&self, bearer: &str) -> Result<ActorContext, AuthFailure>;
}

pub trait CatalogReader: Send + Sync {
    async fn list_products(
        &self,
        actor: &ActorContext,
        query: ProductPageQuery,
    ) -> Result<ProductPage, CatalogFailure>;
}

pub struct CatalogService<R> { reader: R }
impl<R: CatalogReader> CatalogService<R> {
    pub async fn list_products(
        &self,
        actor: &ActorContext,
        query: ProductPageQuery,
    ) -> Result<ProductPage, CatalogFailure>; // permission check + delegated scoped read
}
```

The real implementation should make `BusinessId` and `UserId` validated opaque newtypes. `ProductPage` and `ProductSummary` above are application read models independent from both HTTP and SQL; a private HTTP mapper serializes them into `v1` DTOs. The SQL adapter must query by `actor.business_id` on every request and must not accept a client-supplied business/store ID. A later schema may add branch/store constraints; this slice does not assert a current hosted data model.

### Module map

```text
apps/api/
  Cargo.toml
  src/
    main.rs                 # config + adapter wiring; no business rules
    config.rs               # bind address, allowed origins, secret references
    http/
      mod.rs                # router + safe error mapping
      health.rs             # non-sensitive liveness only
      v1/mod.rs             # /v1 router
      v1/catalog.rs         # wire DTOs, query validation, status mapping
    application/
      mod.rs
      catalog.rs            # permission and use-case policy
      identity.rs           # ActorContext and PrincipalResolver contract
    adapters/
      mod.rs
      auth/                  # future verified cloud session implementation
      catalog/               # future tenant-scoped SQL implementation
```

The router owns protocol concerns; `CatalogService` owns permission/use-case policy; the storage adapter owns query translation and tenant predicates. The public surface is one catalog operation plus liveness. This keeps the call chain short and hides query/storage representation from clients. In the first slice the explicit adapter ports expose only what the service needs, rather than a generic repository or desktop command facade.

### Invariants and non-goals

- Wire DTOs are stable `/v1` contracts and are not imported from `pos-core` or desktop `models.rs`.
- Authentication produces `ActorContext` only after verification by a real cloud auth adapter. A request body/header cannot choose tenant scope.
- Authorization is checked server-side for each use case; tenant scoping is enforced in the adapter query as a second boundary.
- No sale, refund, stock mutation, subscription/device entitlement or audit write is exposed by the first implementation.
- No local SQLite, desktop keyring, local passkey or staff profile becomes a cloud credential.
- No PostgreSQL provider, schema or hosted database is claimed to exist. The port records the seam to implement after that choice.
- Health output is fixed and non-sensitive; it does not report secret/config/database details.

## Rationale

This shape is API-first without starting by publishing a low-value health-only service as the vertical slice. Catalog listing is a meaningful browser/CLI consumer operation and a compact proof of auth context, authorization, tenant filtering and DTO isolation. It refuses to simulate missing cloud capabilities: `healthz` can demonstrate the process, while the protected route remains unavailable until its real adapters exist. This follows the chosen architecture's server authority and its warning that local SQLite identities cannot stand in for hosted accounts.

An eventual sale endpoint must be a server application use case that recomputes price, tax, discount approval, stock, payment and accounting inside a server transaction; it must not be a wrapper around `checkout_sale` or accept a client-computed total as authority. Its stable idempotency key must be bound server-side to a canonical request payload and tenant scope. That work is too wide and dependent on unresolved payment/accounting schema for this initial protected read slice.

## Tradeoffs accepted

- We accept that the first binary can answer health but cannot serve catalog data until real auth and storage adapters are selected, in exchange for never shipping a fake tenant identity or demo store as cloud authority.
- We accept a narrow `PrincipalResolver`/`CatalogReader` port pair in exchange for keeping framework and storage concerns private and independently replaceable.
- We accept product summary duplication in API DTOs in exchange for stable cross-platform wire contracts that do not leak local domain/database structs.
- We accept deferring sale workflows in exchange for not defining money, payment, tax, inventory, audit and idempotency semantics without their server data model.

## Alternatives considered

1. **Public capability/health-only service.** This can be independently runnable immediately and safely, but its only consumer call proves liveness rather than a tenant-aware product workflow. It exposes less to callers, but also hides less application complexity and does not validate authorization scope.
2. **POST `/v1/sales` as the first endpoint.** This is the most meaningful business mutation, but it requires authoritative hosted catalog, tenant auth, stock, cash shifts/payment configuration, discount approvals, accounting, audit and transaction/idempotency semantics. It would create a broad set of invented assumptions before the unresolved cloud foundation.
3. **Expose desktop commands over HTTP or share desktop `models.rs`.** This hides little and leaks Tauri/local session/SQLite decisions into every API client. It cannot provide server-side tenant authority and is rejected.

## Open questions and risks

- Which cloud authentication/session provider will issue the bearer credential, and what verified claims are sufficient to derive business and user scope?
- Should catalog visibility be business-wide or branch/store scoped in v1, and how will business-wide product identity map to local per-store catalogs?
- Which PostgreSQL host/migration tool will implement the tenant predicate and backup/restore expectations?
- Is the first API runtime intended for one process with pooled SQL connections, and what deployment/configuration contract should local Docker provide?
- Should product prices in the shared catalog response be represented in the business's configured currency once that model exists, or should the endpoint omit price until currency semantics are established?

## Next implementation step

Add `apps/api` to the Cargo workspace with `/healthz`, router/config/error mapping, DTO and port sketches, and a fail-closed protected route; then select and implement verified cloud identity plus tenant-scoped storage before enabling catalog responses.

## Intended files and commands

Candidate artifact only:

- `.planning/clients/api-v1-candidates/candidate-b/CANDIDATE.md`

No production files or Cargo manifests are changed by this candidate. After synthesis, the planned implementation files are the `apps/api/` module map above and the workspace root `Cargo.toml`. Likely commands after implementation: `cargo check -p advanced-pos-api`, `cargo run -p advanced-pos-api`, `curl http://127.0.0.1:<port>/healthz`, then an authenticated catalog request once real adapters are configured.
