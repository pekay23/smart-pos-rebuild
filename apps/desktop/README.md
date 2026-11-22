# Advanced POS Desktop

Tauri + React desktop client for the offline-first POS application.

## Development

```powershell
npm install
tools/cargo-build.ps1 -Command tauri-dev
```

Run the command from the repository root. It starts Vite and the Tauri window together, so frontend edits hot-reload while the app is running. Native Rust changes are rebuilt by Tauri's development watcher. The shared build wrapper counts the run and clears managed Cargo output after every third build/run. In a new empty development data directory, choose **Create demo workspace** to load isolated review accounts; see [`DEMO_ACCOUNTS.md`](../../docs/DEMO_ACCOUNTS.md).

## Build

```powershell
npm run build
npm run lint
```

The account/work page contains **App updates**. Routine direct-distribution desktop updates require a usable delta chain, download only delta packages, reconstruct the target locally, and apply to the existing installation after the user chooses to restart. Missing or invalid patches fail closed; the in-app updater does not fall back to downloading the full app package. A one-time full migration is needed for the legacy MSI. Set `ADVANCED_POS_UPDATE_FEED_URL` at build time to the HTTPS feed root containing Velopack `releases.<channel>.json`. Production feed hosting, signed desktop packaging, release automation, and migration validation from the current MSI are not configured yet. See [the release and update plan](../../docs/RELEASE_UPDATE_PLAN.md).

## License

AGPL-3.0-or-later. See [LICENSE](../../LICENSE).
