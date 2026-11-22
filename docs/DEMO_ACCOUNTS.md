# Local development demo accounts

These accounts are available only in a Tauri development build. From the repository root, run `tools/cargo-build.ps1 -Command tauri-dev`, then choose **Create demo workspace** on the first-run screen. The action is limited to an empty local database and creates a sample Accra Wood Supplies Ltd store with inventory, a supplier, a customer, an opening delivery, and one expense.

| Role | Username | Password | Shift PIN |
| --- | --- | --- | --- |
| Owner | `owner` | `OwnerDemo2026!` | `135790` |
| Administrator | `admin` | `AdminDemo2026!` | `864209` |
| Manager | `manager` | `ManagerDemo2026!` | `246802` |
| Cashier | `cashier` | `CashierDemo2026!` | `357913` |

The seed command and its credentials are not compiled into optimized production builds. This sample workspace is for local review only; do not use the demo credentials for a real store.

Sample products include scanner-ready demo barcodes: Plywood 18 mm `2000000000010`, MDF sheet 12 mm `2000000000027`, and Pine timber 2 × 4 `2000000000034`. These local demo values can be used with keyboard-wedge scanners and are not real product identifiers.
