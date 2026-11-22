# POS product research and platform direction

Research checked on 29 October 2026 against official vendor and framework documentation. This is a product-planning reference; it is not a claim of feature parity with these vendors.

## Useful patterns from established POS products

- **Fast, forgiving checkout:** Keep scan/search, quantity changes, discounts, customer assignment, tender, and receipt in one counter workflow. Support barcode scanners, held carts, clear void/return flows, and order notes. Shopify documents barcode search, held/draft orders, discounts, notes, receipts, returns, and exchanges in its [POS feature overview](https://www.shopify.com/pos/features).
- **Stock that follows transactions:** A sale should change available stock once, receiving should increase it once, and corrections should always record who changed what and why. Square describes sale-linked inventory counts and low-stock alerts in its [inventory tracking guide](https://squareup.com/help/us/en/article/7746-tracking-your-inventory-with-square-for-retail).
- **Practical buying workflows:** Receiving, purchase orders, supplier history, stock transfers, quick counts, reorder alerts, and order recommendations help prevent stockouts. Shopify lists receiving, transfers, quick counts, purchase orders, and low-stock alerts in its [inventory management features](https://www.shopify.com/pos/features). Lightspeed describes inventory performance, reorder recommendations, and scheduled reports in its [retail analytics overview](https://www.lightspeedhq.com/pos/retail/analytics/).
- **Staff-specific access:** Give each person a named account or PIN, store-scoped access, and only the permissions needed for their shift. Manager approval for discounts, refunds, and sensitive settings is a useful next step. Shopify explains role-based POS permissions and location assignments in its [staff-management guide](https://help.shopify.com/en/manual/sell-in-person/shopify-pos/staff-management/understanding-pos-staff-management).
- **Honest offline behavior:** Local checkout and inventory can work offline, while card processing and cloud features need explicit provider behavior and reconciliation. Square's [offline payments guide](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode) describes limits, risk, and later processing. Our current desktop app records tender type locally; it does not process a card or mobile-money payment.
- **Decision-focused reports:** Start with daily sales, cash/card/mobile-money totals, gross margin, top and low-stock items, sales by staff, refunds, and expenses. Add date/location/category filters and export before forecasting or AI recommendations.

## Priority for this product

### Counter attendant

1. Keep the register immediately available with a focused barcode/search field, readable product names, visible stock, large total, and clear tender/change steps.
2. Make common tasks keyboard-friendly and preserve a cart if the attendant needs to look up a customer or product.
3. Show only attendant actions; require a manager PIN/approval for restricted discounts, voids, refunds, and cash corrections.
4. Show an unmistakable saved/unsaved and online/offline state. Never label a tender as processed unless a payment provider confirms it.

### Manager and administrator

1. Provide a short opening/closing flow: opening cash, paid-in/out, counted cash, expected cash, variance reason, and close report.
2. Add linked refunds/exchanges, purchase orders and approvals, partial receiving, transfers, and count sheets with movement history.
3. Add staff invitations, role templates, PIN reset, action history, and sales by attendant.
4. Add tax configuration, receipt/printer setup, database backup/restore, and reports with CSV/PDF output.

### Business growth

After those store workflows are stable, add multi-location inventory, reorder recommendations, customer purchase history and loyalty, online pickup/delivery orders, scheduled reporting, cloud backup/sync, and mobile owner views. Keep external payment, SMS, AI, and accounting integrations behind explicit adapters and provider settings.

## Platform and language recommendation

- **Desktop first:** Keep the current React/TypeScript interface and Rust domain/backend behind Tauri commands. This fits the existing offline-first app and lets the same domain rules run in the native shell. Tauri describes a frontend-agnostic desktop/mobile toolkit with Rust and optional Swift/Kotlin logic in its [overview](https://v2.tauri.app/start/); its [frontend guide](https://v2.tauri.app/start/frontend/) recommends SPA/SSG-style frontends and a client/API boundary.
- **Windows, macOS, Linux:** Reuse the React UI and Rust domain crate. Add small OS-specific adapters only where device APIs require them: receipt printers, cash drawers, scanner configuration, secure storage, and updates. Keep platform calls behind a typed interface so most workflows stay shared.
- **Web:** Build a separate browser client against a versioned HTTPS API. Reuse design tokens, validation contracts, and domain concepts, but keep database access and authoritative permissions on the server. Vercel can host the web frontend; use provider-neutral PostgreSQL migrations with Neon or Supabase selected after comparing current requirements and limits.
- **API implementation:** Prefer a Rust API service if reusing the existing Rust domain rules is the priority; host that service on a container-capable provider and use Vercel for the frontend. If the team prioritizes Vercel-native serverless operations, a TypeScript API is also reasonable, but domain rules must be shared or kept equivalent with cross-boundary contract tests to avoid desktop/web behavior drifting.
- **Mobile:** First evaluate Tauri 2 with the shared UI. Add Swift or Kotlin plugins only for platform-specific hardware/payment capabilities that cannot be reliably handled by a shared frontend. Build fully native screens only if testing shows a concrete device integration or performance need that the shared app cannot meet.
- **Sync:** Keep local SQLite as the offline store, assign stable IDs, write transactions to an outbox, and reconcile with the server idempotently. The server owns tenant boundaries, permissions, payment status, and final conflict decisions.

## Working brand direction

**Provisional name:** Advanced POS  
**Positioning:** A clear, dependable counter and store-control system for small retail teams.  
**Tagline candidates:**

- Your shop, running smoothly.
- Sell fast. Stay in control.
- Every sale. In sync.

**Logo direction:** A compact “N” formed from a receipt edge and a forward-moving line, paired with a calm deep-green wordmark and a warm amber accent. Keep it legible on a receipt, app icon, and small cash-register screen.

Initial exact-name web searches did not surface an obvious POS product named Advanced POS. This is only a quick search result, not trademark, company-name, social-handle, or domain clearance. Check Ghana and target-market trademark registers and domain availability before adopting it publicly. Other names surfaced clear collisions or adjacent uses: Tillora is a billing/POS product, KasaFlow is already used for software concepts, Counterleaf is an existing product brand, and Storelane is used by retail/storefront products; avoid those directions.

## Quicklogic POS video reference

The user supplied a transcript of [Quicklogic's point-of-sale demo](https://www.youtube.com/watch?v=x3Kli-OIPRk). It names these useful workflow ideas: role-based login; shortcut-driven checkout; sorting, drafts and quotations; returns/refunds; customer profiles and credit limits; expenses; product price lookup; shifts; X/Z register reports; store/tax/printer/receipt settings; backup/cloud sync; variants and tags; staff permissions; and sales, expense, and profit-and-loss reports. These transcript details informed the roadmap. They are not independent verification of the vendor's product or an implementation claim about Advanced POS.

The Bubble [animated receipt example](https://gochain-plugins-demo-1.bubbleapps.io/version-test/animated_receipt) displayed a sample receipt with subtotal, tax, total, order ID, masked card number, date, and a completion animation. That page demonstrates a receipt animation only; it does not establish physical printer support or message delivery. Advanced POS now has a local receipt view, OS print handoff, and email/WhatsApp draft handoff, with staff review/send required.
