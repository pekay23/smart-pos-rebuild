# Advanced POS

Retail point-of-sale software in active development. The Windows desktop client (Tauri + React + Rust + SQLite) is the most complete local POS. A browser client and Rust/PostgreSQL API now include hosted catalog, manual tender checkout, cash drawer and X/Z reporting, branch sales reports, stock availability and inter-branch transfer workflows. The hosted service is still a partial POS: provider payment capture, customers, refunds, offline sync, subscriptions and production operations are not complete. See the implementation tracker for verified status.

## Features

- Local desktop: store setup, staff profiles, role-based access, catalog, customers, suppliers, attendance, cash register, receipts, returns, purchasing and reports.
- Hosted web/API: OIDC-based tenant access; branch catalog and payment-method settings; station checkout with server-side stock/tender records; cash-drawer shifts and X/Z reports; branch sales reports; and cross-branch availability and stock transfers.
- Hardware and online payments depend on configured providers and tested devices; hosted tender records are manual and do not authorize or capture funds.

## Quick start

Requirements: Node.js, Rust 1.88+, Visual Studio C++ build tools, native Windows Perl, WebView2.

```powershell
cd apps\desktop
npm install
npm run tauri dev
```

The browser client is in `apps/web`. Configure its public OIDC client and API URL from `apps/web/.env.example`, then run `npm ci` and `npm run dev` there. Hosted features require a configured identity provider, provisioned tenant data and a running Rust API/PostgreSQL service; the client has no seeded hosted accounts.

Build checks:

```powershell
npm run build
npm run lint
pwsh -File tools/cargo-build.ps1 -Command check -CargoArgs @('-p', 'smart-pos-desktop')
```

Use `tools/cargo-build.ps1` for every local Cargo build/check/test/clippy/run command. It shares one root `target/` directory and automatically clears Rust/native build output at 21.5 GiB or after 100 Cargo commands, including interrupted runs. The size threshold is the primary limit; the command count is a fallback. At the last recorded 3.7 GiB workspace size, this puts the expected total near 24.3 GB, leaving about 0.7 GB below the 25 GB workspace cap for growth and measurement differences. Vite is configured to empty each app's `dist/` before rebuilding, so only the latest web and desktop frontend bundles remain.

## Documentation

- [Documentation index](docs/INDEX.md)
- [Product plan](docs/PRODUCT_PLAN.md)
- [Implementation tracker](docs/IMPLEMENTATION_TRACKER.md)
- [API service](apps/api/README.md)
- [Desktop app](apps/desktop/README.md)
- [Web client](apps/web/README.md)

## License

Licensed under the GNU Affero General Public License v3.0 or later. See [LICENSE](LICENSE). Network-facing builds provide source via `/source` and the repository link above.
