# Release and client update plan

Status: web deployment detection and update checks are implemented. Desktop Velopack check/download/stage/restart integration and an immutable, CLI-pinned desktop package-generation script are implemented. **Routine desktop updates download only delta patches, reconstruct the new package locally, and update the existing installation in place. The updater fails closed if a delta is unavailable, invalid, or cannot be applied; it never silently downloads the full application package.** Release packaging optimizes deltas for size and refuses routine releases without a generated delta. A full setup package is reserved for first install or a one-time migration/recovery process explicitly provided outside the in-app updater. Production feed hosting, signed package publishing, migration validation, store accounts, and the release operator console remain open.

## Distribution by client

| Client | Release channel | Update experience |
| --- | --- | --- |
| Web app | Vercel deployment | The app polls a cache-busted, `no-store` `build-info.json` every five minutes and when the tab returns to the foreground. It offers **Refresh now** or **Later**; reload loads the deployed bundle. |
| Windows/macOS/Linux desktop, direct distribution | Velopack | Publish channel-specific full base packages and generated delta packages over HTTPS. Routine client downloads require an available delta chain and stage only changed-file patches; the app applies them in place after a user-selected restart. Any missing, invalid, or unappliable delta stops the update without downloading the full application package. |
| macOS App Store | Mac App Store | Submit versions through App Store Connect; the Store owns installation and update delivery. Do not use a separate updater for App Store builds. |
| Android / Google Play | Google Play | Use Play Core flexible updates for background download and an in-app restart when convenient; reserve immediate updates for critical changes. Google Play controls delivery and installation. |
| Android outside Google Play | Approved device/vendor channel | Use the target store or managed enterprise channel. Direct APK update behavior must follow Android signing, install-source, and device-owner policy; do not silently install packages. |
| iOS/iPadOS App Store or TestFlight | App Store Connect / TestFlight | Submit each binary update for Apple review/distribution. The app may show version/release notes and open its App Store listing; it must not download/install a replacement executable itself. |
| HarmonyOS | AppGallery | Use AppGallery's native update manager/dialog. Publish and stage releases in AppGallery Connect; store delivery handles the actual install. |

The direct desktop client uses Velopack's Rust feed and apply machinery with a delta-only downloader. Delta packages carry changed-file data and are reconstructed before apply. The existing app installation is updated in place and its setup installer is not rerun. Missing base packages, missing delta chains, checksum errors, or patch failures stop the update; the in-app updater never falls back to downloading the full application package. First installs and one-time legacy MSI migration/recovery packages are delivered separately. Store-distributed clients use store-managed updates under the same app identity rather than a separate reinstall flow. The repository contains a Tauri desktop client and hosted web client; native Android, iOS, and HarmonyOS projects do not exist yet.

## Release workflow to build

1. Developer creates a versioned release draft with channel (`demo`, `beta`, `stable`), platform/architecture, release notes, minimum supported version, rollout percentage, and optional required-update deadline.
2. CI uses the immutable Velopack package ID `PoweredSolutions.AdvancedPOS` across Windows, macOS and Linux, with a release channel embedded in each package. Restore the prior release from the feed before packaging. The repository-pinned Windows packager (`tools/package-desktop-update.ps1`) pins `vpk` to the exact Rust updater version, explicitly uses `BestSize` delta generation, refuses an ordinary release if it cannot find a prior full package, and verifies that a delta is emitted. Sign executables/updater/installers with platform-native identities and record checksums and provenance.
3. Release service validates signatures, checksums, SemVer, artifact/platform compatibility, and release permissions before publishing immutable artifacts to object storage/CDN. The database stores metadata and audit history, never installer binaries or signing secrets.
4. Start with an internal ring, then configurable tenant/device cohorts. Pause a rollout immediately if health signals or support reports cross thresholds. Rollback means publish a higher version that fixes the issue; clients must not accept unsigned downgrades.
5. Client update checks return only the channel/platform-appropriate release. For an already migrated desktop installation, normal releases download an available delta chain and update the current installation in place; they must not prompt customers to download or run the full setup installer again. The desktop updater fails closed on missing or broken patches and never requests the full application package. Any recovery package must be an explicit support/admin action outside the in-app routine update flow. Updates are optional by default. A minimum-version policy can block new sign-ins/sync after a published deadline, with a safe grace path for cashiers to finish an open ticket/register close. Never interrupt a committed sale.
6. Store clients call their store APIs/SDKs. Track submission, review, phased rollout, availability, and rollback in the operator console, but do not mirror store binaries into an in-app installer.
7. Web deployment stamps each build with a unique commit/build ID. The current client can continue operating until the user refreshes; deployment monitoring must not discard a cart or unsaved form.

## Security and operational requirements

- Keep platform code-signing identities, Apple certificates/profiles, Android upload keys, and Huawei credentials out of source control, tenant settings, client bundles, and ordinary operator notes. Use separate identities per platform/channel, protected backups, rotation/recovery procedures, and audited release permissions.
- Host feeds and assets over HTTPS with immutable paths; validate SHA-256 checksums and platform code signatures. Velopack's feed checksums are integrity checks, not a substitute for code signing or protected publishing credentials.
- Require HTTPS, immutable versioned artifact URLs, platform signature checks, SHA-256 inventory, malware scanning, SBOM/provenance, access-controlled drafts, two-person review for stable/required releases, and append-only publish/pause/rollback audit events.
- Release metadata is public and contains no tenant, user, device identifier, or secrets. Device entitlement checks remain server-side; update manifests do not grant subscriptions or features.
- Separate app release updates from business-data/schema migrations. Local database migrations must be backward-compatible where possible, transactional, backed up, and tested against supported rollback paths before a mandatory app version is announced.
- Enforce channel selection server-side and rate-limit update checks. Cache metadata briefly and make web build info explicitly no-store.
- Never force an update during checkout. For critical security fixes, explain the deadline and allow a safe transaction/register-close boundary.

## Implemented client contract

### Web

`apps/web` builds a `build-info.json` asset containing version and build ID. A no-store Vercel header and cache-busted request allow an already-open tab to detect a newer deployment. The update banner supports refresh-now and 15-minute reminder behavior.

### Desktop

The account/work page invokes native Tauri commands to check, download, and apply updates. Download progress is sent from the updater worker to an accessible progress bar. The Rust runtime uses Velopack's `HttpSource`, configured at build time through `ADVANCED_POS_UPDATE_FEED_URL`; only HTTPS feed URLs are accepted. The host must provide `releases.<channel>.json` and its immutable full/delta packages. The client downloads only the delta chain, verifies package size and SHA-1/SHA-256 checksums, and reconstructs the target package locally using Velopack's patch command. Missing base packages, missing delta chains, download/checksum failures, or reconstruction failures stop the operation and do not fetch the full application package. The user explicitly restarts to apply. Updates replace changed files in place and do not rerun the app's setup installer.

For Windows release packaging, restore the repository's pinned tool with `dotnet tool restore`, set `ADVANCED_POS_UPDATE_FEED_URL` to the HTTPS channel feed while building the Tauri app, then run `tools/package-desktop-update.ps1` with the Tauri release directory, executable name, version, HTTPS feed URL and output directory. The script downloads the previous channel release first and refuses to produce an ordinary release without a delta. Use `-AllowInitialRelease` only for the first channel package or the one-time migration package. Upload the complete updated feed and referenced assets atomically; retain older delta assets while clients may still need them. The script packages a release but does not sign or publish it.

The current MSI installation is not managed by Velopack. Moving existing users to this update mechanism requires one full migration installer; this is a one-time transition, not the recurring release process. Keep the Tauri app-data identifier stable to preserve local business data, and validate the migration on a copy of an installed MSI. After migration, ordinary releases generate and publish size-optimized deltas and update the existing installation in place. Although Velopack's stock downloader can fall back to a full update package, Advanced POS uses its delta-only download path; patch failures stop the update without requesting the full package. Keep clients reasonably current to reduce the number of deltas that must be applied.

Development builds and installations without a configured feed show an unavailable state. Production feed hosting, code signing, release automation, and MSI migration validation are not configured yet; this integration does not claim production updates are already published.

## Remaining implementation checklist

- [ ] Create the operator release console and authenticated release-management API (draft, artifact metadata, publish, staged rollout, pause, retire, audit).
- [ ] Provision protected CI signing and platform store credentials; document recovery and rotation.
- [x] Add a Windows desktop release packager pinned to the updater's Velopack CLI; it restores the prior feed release, generates/verifies a delta for regular releases, and blocks same-version artifact replacement. PowerShell AST parsing and a local two-version fixture drill passed. The pinned CLI emitted a 36,009-byte 1.0.1 delta against a 4,593,979-byte full package; applying the delta reconstructed a package with the same SHA-256 as the target. Missing and truncated deltas failed without creating output packages. Artifact names include the release channel, and the packager checks the correct channel-qualified names.
- [x] Run the pinned CLI against two isolated local fixture versions; verify the generated channel-qualified delta is smaller and reconstructs a byte-identical target package. The deterministic drill is `tools/verify-desktop-delta-packaging.ps1`; it also verifies missing and truncated patch rejection without a reconstructed output.
- [x] Build two real Windows Velopack versions, install v1 into an isolated temporary directory, reconstruct v2 from the real v1 package plus v2 delta, then apply it in place with Velopack `Update.exe`. The installed version advanced, a business-data sentinel survived, and the setup install log did not change. This proves real package delta reconstruction/application and data preservation, but does not exercise the app's Rust HTTP downloader.
- [ ] Run the app's updater against a trusted HTTPS test feed; prove it downloads only the size-optimized delta, applies after restart, preserves app data, and fails closed on interrupted/corrupt/missing patches without launching setup or requesting the full app package. The local self-signed HTTPS fixture could not be trusted by Velopack's Rust HTTP client, which uses bundled WebPKI roots instead of the Windows trust store.
- [ ] Add an update acceptance drill that confirms ordinary releases never launch setup or download the full app, including patch corruption and missing-delta cases; separately document and measure the one-time MSI migration/recovery package.
- [ ] Build and validate Velopack desktop packages for macOS and Linux; configure the HTTPS feed, platform signing, delta generation, checksum-failure, rollback-safe database migration, and cashier boundary behavior.
- [ ] Build a one-time MSI-to-Velopack migration package preserving existing app data and validate it against a copied MSI installation.
- [ ] Add native Android, iOS, and HarmonyOS applications, then integrate Play Core, App Store Connect/TestFlight, and AppGallery update APIs respectively.
- [ ] Add release cohorts, minimum-supported-version policy, security release deadlines, telemetry/health gates, notifications, and tenant-visible release notes.
- [ ] Configure Vercel production deployment and verify no-store build-info behavior behind the production CDN.
- [ ] Run signed release drills on clean devices and document tenant recovery/support procedures.

## Official platform references

- [Velopack Rust quick start](https://docs.velopack.io/getting-started/rust) — Rust startup integration and update check/download/apply API.
- [Velopack delta packages](https://docs.velopack.io/packaging/deltas) — delta generation, catch-up, selection heuristics, and full-package fallback.
- [Velopack deployment CLI](https://docs.velopack.io/distributing/deploy-cli) — download the prior release, package the new version, and upload generated assets.
- [Velopack integration overview](https://docs.velopack.io/integrating/overview) — feed, download, apply, and restart behavior.
- [Velopack code signing](https://docs.velopack.io/packaging/signing) — sign desktop executables, Velopack updater binaries, and installers before distribution.
- [Google Play in-app updates](https://developer.android.com/guide/playcore/in-app-updates) — flexible and immediate update flows and Play-managed install/restart.
- [Apple App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/) — rules on downloading executable code, App Store review, TestFlight, and Mac App Store distribution/update requirements.
- [Huawei HarmonyOS AppGallery update manager](https://developer.huawei.com/consumer/en/doc/harmonyos-references/store-updatemanager/) — HarmonyOS update checks and store-managed update prompts.
- [Huawei AppGallery Connect](https://developer.huawei.com/consumer/en/doc/app/agc-help-introduction-0000002270860209/) — app submission, release, phased rollout and rollback tools.
