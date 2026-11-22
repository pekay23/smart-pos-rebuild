# Documentation index

This folder is the canonical home for project documentation. Planning and
product documents live here, with links to app-specific guides and retained
design notes under .planning/.

## Product and planning

- [LICENSES.md](LICENSES.md) - license research and choice (AGPLv3)
- [RENAME_TO_TALLIXPOS.md](RENAME_TO_TALLIXPOS.md) - planned product rename checklist
- [IMPLEMENTATION_TRACKER.md](IMPLEMENTATION_TRACKER.md) - single checklist to resume implementation
- [RELEASE_UPDATE_PLAN.md](RELEASE_UPDATE_PLAN.md) - release pipeline and cross-platform update design
- [IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md)
- [PLAN.md](PLAN.md)
- [PRODUCT_PLAN.md](PRODUCT_PLAN.md)
- [PRODUCT_PLATFORM_PLAN.md](PRODUCT_PLATFORM_PLAN.md)
- [PRODUCT_COMMERCIAL_PLATFORM_PLAN.md](PRODUCT_COMMERCIAL_PLATFORM_PLAN.md)
- [PRODUCT_RESEARCH.md](PRODUCT_RESEARCH.md)
- [CASHIER_WORKFLOW_DESIGN.md](CASHIER_WORKFLOW_DESIGN.md)
- [DEMO_ACCOUNTS.md](DEMO_ACCOUNTS.md)
- [Customer receivables design](../.planning/customer-receivables.md)

## Planning workspace

- .planning/ - feature design notes, client/arena/inventory/security notes, and the
  vendored toolchain (gitignored).

## API

- [apps/api/README.md](../apps/api/README.md)
- [apps/api/openapi.yaml](../apps/api/openapi.yaml)
- [apps/api/migrations/postgres/README.md](../apps/api/migrations/postgres/README.md)

## Desktop

- [apps/desktop/README.md](../apps/desktop/README.md)

## Secrets and local-only files

Secrets are never committed. Copy .env.example to .env (gitignored) and fill
in locally. The ZITADEL private-key JSON referenced by POS_API_OIDC_KEY_FILE
must live outside the repository or in a gitignored path. See .gitignore.
