# Advanced POS API foundation

This is an independently runnable Rust HTTP service with hosted identity, station, catalog, inventory and an initial signed checkout and cash-drawer slice. It exposes process liveness, a stateless sale-preview calculation, optional ZITADEL access-token introspection, RLS-backed tenant membership lookup, branch/station permission snapshots, owner-authorized station activation/revocation, and cashier-only station lease acquire/heartbeat/release. Activation is transactionally capacity-limited and requires Ed25519 proof of device-key possession. Owner revocation records a reason, cancels pending challenges and terminates any active cashier lease. The hosted catalog supports authorized product creation and branch/register reads, station-authorized cross-branch availability, and idempotent manager stock adjustments. `POST .../sales` uses server-owned prices/tax and atomically records configured manual tenders, immutable receipt snapshots, stock movements and audit under a fresh station signature. Cash requires an open station shift and is included in the drawer X/Z reconciliation. Station users can search their RLS-visible completed sales and reprint immutable receipts; branch sales readers have paged reports and receipt detail. Walk-in customers can be attached after checkout without changing the sale. Configured external tenders are cashier-confirmed only; provider authorization/capture is not implemented. The calculation endpoint still trusts caller-supplied values and is only a preview. The API is not yet a complete production POS backend.

## Run locally

From the repository root:

```powershell
$env:POS_API_BIND = '127.0.0.1:8787'
$env:POS_API_ALLOWED_ORIGINS = 'http://localhost:5173'
pwsh -File tools/cargo-build.ps1 -Command run -CargoArgs @('-p', 'pos-api')
```

### Local Docker development stack

From the repository root, use `docker compose up --build` to start PostgreSQL 16, apply the embedded migrations, grant the API runtime role only the identity/bootstrap, station activation, cashier lease, catalog, inventory and audit columns currently required, and start the API. The database and API ports bind to loopback (`55432` and `8787`). The database uses a named volume; `docker compose down -v` deletes that local database.

The compose file uses visibly local-only example passwords when the corresponding `POS_API_DEV_MIGRATION_PASSWORD`, `POS_API_DEV_RUNTIME_PASSWORD` and `POS_API_DEV_OPERATOR_PASSWORD` variables are unset. Override them in a local, ignored `.env` file before starting the stack. Never use these defaults or this compose configuration in a deployed environment. Both API containers are debug builds; insecure Docker database overrides are accepted only in debug builds and only for the Compose service hostname `postgres`. Release builds reject the override and still require `sslmode=verify-full` for hosted database hosts. PostgreSQL and both APIs bind to loopback for local use.

The Compose stack starts `operator-api` as a separate executable on port `8788`, with its own `POS_OPERATOR_OIDC_*` settings and `pos_operator_runtime` database login. Queue endpoints remain unavailable until a separate operator IdP client and provisioned operator account are configured. When operator OIDC is enabled, configure `POS_OPERATOR_FORBIDDEN_AUDIENCES` and `POS_OPERATOR_FORBIDDEN_CLIENT_IDS` with the tenant API audience and clients; startup rejects overlapping values. Require passkeys/MFA in the operator IdP policy before enabling access; the current introspection response does not prove which factor the person used. The separate operator web client is in `apps/operator-web`; run it on `http://127.0.0.1:5174` and configure its `VITE_OPERATOR_*` settings independently from the tenant app. The disposable PostgreSQL verifier covers active, inactive and unprovisioned operator identities, tenant/operator token separation, revisions, idempotency and tenant-visible progress through separate restricted database logins. The web app builds and runs locally, but browser visual acceptance could not be performed because local browser use is disabled in the current Work settings. Production IdP policy and deployment readiness remain unverified. Local Compose fallback credentials are for disposable development only.

Operator accounts can be provisioned and maintained with the offline CLI after the operator migrations. Set `POS_OPERATOR_PROVISIONING_DATABASE_URL` to the separate `pos_operator_provisioner` credential and `POS_OPERATOR_PROVISIONING_ACTOR` to the audit actor label, then run `operator-api provision <issuer-origin> <oidc-subject> <display-name> <reason>`, `operator-api set-display-name <operator-uuid> <display-name> <reason>`, or `operator-api set-status <operator-uuid> <active|suspended|deactivated> <reason>`. Provisioning creates the support role only; display names and status changes are recorded with append-only provisioning events. The support runtime receives only active display-name directory entries and cannot read issuer/subject fields from the directory table. The provisioning credential has no `BYPASSRLS` and cannot read tenant tables. The actor label is operator-supplied for audit context, not an IdP-verified identity.

Tenant support requests may opt in to sharing only the client app version and one allowlisted platform label. The server records the consent time. The request contract rejects unconsented diagnostic values and arbitrary fields; it does not collect logs, browser user-agent strings, device identifiers, local paths or business records. Operators see these fields on the internal request inbox; tenant staff can see what was submitted with their request.

The migration command runs as the local migration role before runtime grants are provisioned. API requests use the separate non-superuser, non-`BYPASSRLS` role. OIDC is not configured by default, so authenticated identity routes fail closed until a developer configures a local provider and provisions an identity. No fictitious tenant or login is seeded. The stack currently does not include queue/webhook simulators or browser screenshot automation because those service contracts are not implemented; it does not emulate Windows, macOS, Android, iOS or HarmonyOS UI.

Useful commands:

```powershell
docker compose up --build
docker compose logs -f api
docker compose down
docker compose down -v # removes the local PostgreSQL data volume
```

To run the restricted-role PostgreSQL migration/RLS verifier instead of the persistent developer stack, use `./tools/verify-api-postgres-migrations.ps1`. It creates and removes its own disposable database container.

`POS_API_BIND` defaults to loopback at `127.0.0.1:8787`. `POS_API_ALLOWED_ORIGINS` is an optional comma-separated list of exact HTTP(S) origins without paths; omitted means browser cross-origin access is disabled. TLS termination, deployment CORS, rate limits, redacted observability and secret-manager integration are not configured here.

Optional identity verification uses these server-only settings together: `POS_API_OIDC_ISSUER`, `POS_API_OIDC_INTROSPECTION_URL`, `POS_API_OIDC_API_AUDIENCE`, `POS_API_OIDC_ALLOWED_CLIENT_IDS` (comma-separated), and `POS_API_OIDC_KEY_FILE` (the ZITADEL application private-key JSON file). If any setting is present, all are required. The key file is loaded once at startup; never place it in a client, installer, source tree or normal logs. The configured issuer and introspection URL must share an HTTPS origin; HTTP is accepted only for loopback development. The API signs a short-lived `private_key_jwt` client assertion for each introspection request. Introspection validates opaque and JWT access tokens and their revocation status. `GET /v1/auth/identity` matches the verified `(issuer, subject)` exactly to a pre-provisioned internal identity. `GET /v1/me/tenants` returns its active memberships and RLS-filtered branch/station assignments. Branch and station permission endpoints resolve built-in server role permissions for one assigned resource; custom roles fail closed. These snapshots guide client navigation; each implemented product or inventory mutation independently checks active membership, resource scope and its server-owned permission. Identity is never linked or created by email.

Identity, membership, station activation, cashier leases, catalog and stock adjustment routes require the separate server-only `POS_API_DATABASE_URL` runtime credential. When configured, startup opens a bounded pool (maximum 8 connections, 3-second acquisition timeout) and applies a 5-second PostgreSQL statement timeout; it does not run migrations. Keep this credential distinct from `POS_API_MIGRATION_DATABASE_URL`, and use a non-owner/non-superuser role without `BYPASSRLS`. The API enforces `sslmode=verify-full` for non-loopback database hosts; loopback is allowed for local development. Without a runtime URL or identity provider, protected endpoints fail closed with `503`. Unknown identities/resources return `404`; inactive identities return `403`; invalid tokens return `401`; database errors return `503`. The disposable PostgreSQL 16 verifier has passed using the restricted runtime role, including catalog and inventory mutations; production provider/database deployment remains unverified.

Apply the embedded database migrations as a separate deployment step using a dedicated migration-only PostgreSQL credential:

```powershell
$env:POS_API_MIGRATION_DATABASE_URL = 'postgres://migration_user:secret@localhost/advanced_pos?sslmode=verify-full'
pwsh -File tools/cargo-build.ps1 -Command run -CargoArgs @('-p', 'pos-api', '--', 'migrate')
Remove-Item Env:POS_API_MIGRATION_DATABASE_URL
```

The API does not migrate automatically at startup. Do not put the migration URL in a client, installer, source file, or long-lived runtime configuration. Use a restricted runtime role for API queries. See [`migrations/postgres/README.md`](migrations/postgres/README.md) for the required RLS and tenant-isolation gates.

Check liveness:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/healthz
```

Calculate a preview:

```powershell
$body = @{ lines = @(@{ quantity = 2; unitPriceMinor = 1250 }, @{ quantity = 1; unitPriceMinor = 500 }); discountMinor = 100; taxRateBps = 1500 } | ConvertTo-Json -Depth 5
Invoke-RestMethod http://127.0.0.1:8787/v1/sale-calculations -Method Post -ContentType 'application/json' -Body $body
```

The `/v1/sale-calculations` endpoint uses only submitted values; submitted prices and tax are not checked against a business catalog, and its response is only a preview. Hosted checkout is a separate signed, station-scoped route that uses server catalog prices and tax and commits sales, tenders, stock movements and audit records transactionally.

The wire contract is in [`openapi.yaml`](openapi.yaml). Hosted workflows currently include branch catalog reads/creation/editing/archive, stock adjustment, cross-branch availability and transfer lifecycle, revision-checked branch contact and weekly-hours settings, payment-option configuration, signed station checkout, drawer open/movements/X/Z reports, branch sales summaries and historical receipt detail/reprint, station/branch customer search with bounded paging and management, checkout-time and post-sale customer association, a manager supplier directory, owner/admin staff profile images and directory, branch purchase-order drafts with revision-checked editing, placement/cancellation and idempotent partial/final receipts, plus supplier invoice matching and manual settlement. Receiving posts stock movements, actual unit-cost snapshots and audit events transactionally. These use tenant/branch/station authorization and database RLS; use the migration guide and implementation tracker for exact contracts and verification scope. Processor capture/refunds, durable offline sync, subscriptions and broader operator tooling remain open. The separate operator service supports payment request queueing, revision-aware tenant progress, private request history and due dates, a general tenant support inbox, and a separate issue Kanban with typed links, self-assignment, internal schedules and private history. Tenant-facing support responses contain only that business's public status and progress; issue details and operator notes remain private. Production IdP policy, notifications, team directory/assignment and browser acceptance remain unverified.

Branch-local archive and restore are implemented through PATCH on a branch product. They do not delete product identity, change stock, or hide the product from other branches; the API enforces manager permission and atomically records an audit event.

Branch managers can also update selling price, tax rate and stock alert levels at a branch using an expected catalog revision. Stale edits return a conflict; stock quantity changes remain restricted to audited adjustment or fulfillment workflows.
