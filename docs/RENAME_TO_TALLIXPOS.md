# Planned rename: Advanced POS → TallixPOS (apply before commit/push)

Chosen product name: **TallixPOS** (coined from *tally* + *assist*).
Parent/developer identity remains **Powered Solutions**.

Status: **NOT YET APPLIED**. Other agents are actively using the codebase; apply this rename in a single coordinated pass before the initial GitHub commit/push.

## Repo / package identity

- GitHub repository name: `tallixpos` (or `tallixpos-pos` / `tallixpos-core` if obscuring).
- Root directory rename is optional; the remote repo name can differ from the local folder.
- `Cargo.toml` workspace: update descriptions that say "Advanced POS".
- `apps/desktop/src-tauri/Cargo.toml`: `name`, `description`.
- `apps/desktop/package.json`: `name` (currently `desktop`), `description` if added.
- `apps/api/Cargo.toml`: `description`.
- `apps/web` package metadata.

## App metadata

- `apps/desktop/src-tauri/tauri.conf.json`: `productName`, window `title`.
- Bundle identifier: decide whether to keep `com.amadutech.smartpos.local` to preserve existing local data, or change to a new identifier (data migration required).
- Brand mark letter in `AppShell.tsx` / `AuthFrame.tsx` (currently "A"; change to "T" if desired).

## User-facing strings

- Replace "Advanced POS" with "TallixPOS" in:
  - `apps/desktop/src/**` (AppShell, AuthFrame, pages, components, customer display).
  - `apps/web/src/**`.
  - `apps/api/src/**` (startup log, error messages, OpenAPI title).
  - `apps/api/openapi.yaml`.
- Keep "Powered Solutions" as the parent identity where it appears as developer/company.

## Documentation

- README titles and references.
- `docs/*.md` titles and body text.
- `IMPLEMENTATION_TRACKER.md` working-name notes (update provisional name and collision notes).
- `PRODUCT_RESEARCH.md` naming research.

## Assets

- Icons, splash screens, installer branding, receipt headers, customer display title.
- Any hardcoded "Advanced POS" in tests or seed data.

## Verification after rename

- `npm run build`, `npm run lint`.
- `cargo fmt --all -- --check`, `cargo check -p smart-pos-desktop`, `cargo check -p pos-api`, `cargo test`.
- Launch app; confirm first-run, sign-in, navigation, customer display, receipts, backups still work.
- Confirm local data path/identifier decision did not wipe existing profiles.

## Brand identity and logos (professional process)

The rename is not complete until the visual identity is rebuilt to professional standards. Do not just recolor or stretch the existing "A" mark.

1. **Brand strategy first**
   - Define positioning, personality, audience (local retail + multi-branch chains), and the trust/clarity promise.
   - Write a short brand brief: values, tone, differentiators, and how TallixPOS differs from "Advanced POS."

2. **Logo system, not a single logo**
   - Primary logo (horizontal), stacked/vertical, icon-only mark, wordmark, and monochrome/reversed versions.
   - Define clear space, minimum sizes, and misuse rules.

3. **Design exploration**
   - Sketch 20–40 concepts; select 3–5 directions; refine 2; finalize 1.
   - Test at small sizes (favicon, tray icon, receipt header) and on dark/light backgrounds.

4. **Typography and color**
   - Choose a primary typeface (and fallback) with licensing for commercial use.
   - Define a color palette with accessibility contrast (WCAG AA) and print-safe values (CMYK/spot).

5. **Deliverables**
   - Vector source: SVG (and AI/EPS if needed), plus PDF.
   - Raster exports: PNG (transparent), ICO, ICNS, and sized variants (16, 32, 48, 64, 128, 256, 512, 1024).
   - Favicon set and web manifest icons.
   - Brand guidelines document (PDF) with usage rules.

6. **Implementation in repo**
   - Replace `apps/desktop/src-tauri/icons/*` (icon.ico and any PNGs).
   - Update `brandMark` asset and `AppShell`/`AuthFrame` lockups.
   - Update web favicon/manifest icons and Open Graph image.
   - Update installer/MSI branding and splash screens.
   - Update receipt header logo and customer display branding.

7. **Quality checks**
   - Verify icons render in Windows taskbar, title bar, installer, and tray.
   - Verify no pixelation at 16×16 and 32×32.
   - Verify contrast and legibility on both light and dark themes.
   - Keep source files (SVG/AI) in a `brand/` folder (not in build output) and document them in `docs/`.

- All active agents finish their current tasks.
- A single owner performs the rename in one commit to avoid merge conflicts.