# Store, tax, and receipt settings

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This file retains store, tax, and receipt-setting detail.

## Implemented

- Owners/admins with `settings:write` can edit the business name, current store name, address, phone, currency, and sales tax.
- Currency remains business-wide and locks after any branch has products or financial activity. Existing minor-unit prices and journals therefore cannot be silently relabelled.
- Sales tax is per store. Migration 0024 copies the legacy business-wide tax into each existing store before removing the old key. Checkout and discount approval read the active store's value.
- Profile changes are audited. Updated names and currency replace the current session labels immediately; receipt tax totals retain their checkout snapshots.
- Store preferences include custom receipt header/footer and 58 mm or 80 mm paper width. Migration 0025 adds the settings with a default footer and 80 mm width. Receipt screens and copy/email/WhatsApp drafts use the saved header/footer; print output uses the selected roll width through the OS print dialog.

## Verification

- Rust coverage validates currency and tax input bounds, blocks currency changes across all branches after catalog/financial activity, preserves legacy tax through migration 0024, and verifies migration 0025 defaults.
- Full Rust suite: 58 tests pass.
- Frontend production build passes. Lint exits successfully with the existing warnings in Products, People, and Register.
- Rust formatting and the isolated Windows desktop executable build pass.
- Authenticated settings/receipt UI review, 58/80 mm physical printing, and printer-model driver behavior remain open.

## Remaining work

Store currency is still business-wide; multi-currency branches require explicit ledger currency and conversion rules. Printing depends on an OS printer driver; direct printer discovery, cash-drawer kick commands, and supported-device certification are not implemented.

