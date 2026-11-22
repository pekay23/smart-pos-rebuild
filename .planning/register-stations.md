# Register stations and installation assignment

> Consolidated continuation checklist: [`IMPLEMENTATION_TRACKER.md`](../docs/IMPLEMENTATION_TRACKER.md). This file retains register-station implementation and acceptance details.

## Decision

The local desktop app uses a generated installation UUID stored beside the SQLite database, outside portable business backups. It does not use a hardware fingerprint. A store has stable station keys and editable display names; transactions retain the station name at the time of recording so later renames do not rewrite historic reports.

The owner/admin can add and rename stations and assign the current installation in Settings. Cashiers do not submit arbitrary station IDs. Rust resolves the device assignment for checkout, cash drawer operations, and refunds. Cash sales and refunds use the assigned station's open drawer. A device cannot change stations while either its current or target station has an open drawer.

The finance and architecture council personas independently recommended a trusted backend-resolved device assignment, stable IDs separate from display names, historical snapshots, preserved legacy `main` station records, and explicit limits on cross-device sharing. The implementation follows those recommendations.

## Local verification completed

- Migration 0023 creates station records, a unique per-store local device assignment, sale/refund attribution, and station/report indexes.
- Existing drawer station keys are backfilled; legacy cash sales inherit their linked drawer station.
- New cash/non-cash sales and all refunds snapshot the station key and display name.
- Drawer shifts and saved Z reports snapshot the station display name; history and CSV show the label.
- Sales history can filter by station and shows the saved transaction label; full matching CSV export includes the register and uses that same filter.
- App installation identity is created once and remains separate from exported database contents.
- Portable backup restore can rebind a source installation's local station assignment to the destination installation when it resolves the active session.
- Full native Rust tests pass (58 total), including persisted station assignment and inactive-station rejection. Frontend TypeScript production build and lint pass; three pre-existing effect warnings remain in Products, People, and Register.
- Migration 0023 was applied to a legacy-shaped in-memory SQLite database; station backfills, schema version, and foreign-key checks passed.

## Boundaries and follow-up

Each local installation still has its own independent database. Two computers do not share stock, customers, shifts, or sales yet. Do not put SQLite on a shared drive. Multi-computer and branch station management requires the planned authenticated API or LAN coordinator, idempotent sync, conflict policy, and server-side ownership checks.

Before calling this slice fully verified, review station settings and drawer history through an authenticated app session, exercise portable backup restore on a different local installation, and run the updated full desktop executable. Station-specific aggregate profit and refund reports remain open; they need the same station attribution and accounting rules as sales history.

