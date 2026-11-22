// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use pos_core::{allocate_sale_components, PricedLine, SaleTotals};
use rusqlite::{params, Connection, TransactionBehavior};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
    sync::{Mutex, RwLock},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::models::SessionUser;

fn resolve_app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("ADVANCED_POS_DATA_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("ADVANCED_POS_DATA_DIR must be an absolute path.".to_owned());
        }
        return Ok(path);
    }

    app.path()
        .app_data_dir()
        .map_err(|error| format!("Could not locate the Smart POS data folder: {error}"))
}

fn ensure_cashier_start_preference(connection: &Connection) -> Result<(), String> {
    let has_column = {
        let mut statement = connection
            .prepare("PRAGMA table_info(store_preferences)")
            .map_err(|error| error.to_string())?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        columns
            .iter()
            .any(|column| column == "cashier_start_at_register")
    };
    if !has_column {
        connection
            .execute_batch(
                "ALTER TABLE store_preferences
                 ADD COLUMN cashier_start_at_register INTEGER NOT NULL DEFAULT 1
                 CHECK (cashier_start_at_register IN (0, 1))",
            )
            .map_err(|error| format!("Could not add cashier register preference: {error}"))?;
    }
    Ok(())
}

fn installation_id(data_dir: &std::path::Path) -> Result<String, String> {
    let identity_path = data_dir.join("installation-id");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&identity_path)
    {
        Ok(mut file) => {
            let id = Uuid::new_v4().to_string();
            file.write_all(id.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|error| {
                    format!("Could not save this register's local identity: {error}")
                })?;
            Ok(id)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let id = fs::read_to_string(&identity_path).map_err(|error| {
                format!("Could not read this register's local identity: {error}")
            })?;
            let id = id.trim();
            Uuid::parse_str(id)
                .map_err(|_| "This register's local identity is invalid. Restore its installation-id file before signing in.".to_owned())?;
            Ok(id.to_owned())
        }
        Err(error) => Err(format!(
            "Could not create this register's local identity: {error}"
        )),
    }
}

fn ensure_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
    alter_statement: &str,
) -> Result<(), String> {
    let has_column = {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .map_err(|error| error.to_string())?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        columns.iter().any(|column| column == column_name)
    };
    if !has_column {
        connection
            .execute_batch(alter_statement)
            .map_err(|error| format!("Could not add {column_name} to {table_name}: {error}"))?;
    }
    Ok(())
}

fn backfill_sale_line_components(connection: &mut Connection) -> Result<(), String> {
    let sales = {
        let mut statement = connection
            .prepare(
                "SELECT s.id,s.subtotal_minor,s.discount_minor,s.tax_minor,s.total_minor
                 FROM sales s
                 WHERE (COALESCE((SELECT SUM(si.line_discount_minor) FROM sale_items si WHERE si.sale_id=s.id),0) <> s.discount_minor
                     OR COALESCE((SELECT SUM(si.line_tax_minor) FROM sale_items si WHERE si.sale_id=s.id),0) <> s.tax_minor)
                   AND NOT EXISTS (SELECT 1 FROM refunds r WHERE r.sale_id=s.id)",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    if sales.is_empty() {
        return Ok(());
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    for (sale_id, subtotal, discount, tax, total) in sales {
        let lines = {
            let mut statement = tx
                .prepare("SELECT id,quantity,unit_price_minor FROM sale_items WHERE sale_id=?1 ORDER BY rowid")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([&sale_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        PricedLine {
                            quantity: row.get(1)?,
                            unit_price_minor: row.get(2)?,
                        },
                    ))
                })
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        let totals = SaleTotals {
            subtotal_minor: subtotal,
            discount_minor: discount,
            tax_minor: tax,
            total_minor: total,
        };
        let allocations = match allocate_sale_components(
            &lines.iter().map(|(_, line)| *line).collect::<Vec<_>>(),
            totals,
        ) {
            Ok(values) => values,
            Err(_) => continue,
        };
        for ((item_id, _), allocation) in lines.into_iter().zip(allocations) {
            tx.execute(
                "UPDATE sale_items SET line_discount_minor=?1,line_tax_minor=?2 WHERE id=?3 AND sale_id=?4",
                params![allocation.discount_minor, allocation.tax_minor, item_id, sale_id],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    tx.commit().map_err(|error| error.to_string())
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub device_id: String,
    pub db: Mutex<Connection>,
    pub active_session: Mutex<Option<SessionUser>>,
    pub session_gate: RwLock<()>,
    pub passkey_operation: Mutex<()>,
}

impl AppState {
    pub fn open(app: &AppHandle) -> Result<Self, String> {
        let path = resolve_app_data_dir(app)?;
        fs::create_dir_all(&path)
            .map_err(|error| format!("Could not create the Smart POS data folder: {error}"))?;
        let device_id = installation_id(&path)?;
        let database_path: PathBuf = path.join("smart-pos.sqlite3");
        let mut connection = Connection::open(database_path)
            .map_err(|error| format!("Could not open the local Smart POS database: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(include_str!("../migrations/0001_initial.sql"))
            .map_err(|error| format!("Could not prepare the local database: {error}"))?;
        connection
            .execute_batch(include_str!(
                "../migrations/0002_expense_read_permission.sql"
            ))
            .map_err(|error| format!("Could not update local role permissions: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0003_purchase_receiving.sql"))
            .map_err(|error| format!("Could not prepare purchase receiving: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0004_display_preferences.sql"))
            .map_err(|error| format!("Could not prepare store display preferences: {error}"))?;
        ensure_cashier_start_preference(&connection)?;
        connection
            .execute_batch(include_str!("../migrations/0005_cashier_scope.sql"))
            .map_err(|error| format!("Could not update cashier access: {error}"))?;
        ensure_column(
            &connection,
            "users",
            "pin_hash",
            "ALTER TABLE users ADD COLUMN pin_hash TEXT",
        )?;
        ensure_column(
            &connection,
            "sales",
            "source_ticket_id",
            "ALTER TABLE sales ADD COLUMN source_ticket_id TEXT",
        )?;
        ensure_column(
            &connection,
            "sales",
            "amount_tendered_minor",
            "ALTER TABLE sales ADD COLUMN amount_tendered_minor INTEGER NOT NULL DEFAULT 0 CHECK (amount_tendered_minor >= 0)",
        )?;
        ensure_column(
            &connection,
            "sales",
            "change_due_minor",
            "ALTER TABLE sales ADD COLUMN change_due_minor INTEGER NOT NULL DEFAULT 0 CHECK (change_due_minor >= 0)",
        )?;
        ensure_column(
            &connection,
            "cash_shifts",
            "station_key",
            "ALTER TABLE cash_shifts ADD COLUMN station_key TEXT NOT NULL DEFAULT 'main'",
        )?;
        ensure_column(
            &connection,
            "sales",
            "cash_shift_id",
            "ALTER TABLE sales ADD COLUMN cash_shift_id TEXT REFERENCES cash_shifts(id) ON DELETE RESTRICT",
        )?;
        ensure_column(
            &connection,
            "sale_items",
            "unit_cost_minor_snapshot",
            "ALTER TABLE sale_items ADD COLUMN unit_cost_minor_snapshot INTEGER CHECK (unit_cost_minor_snapshot IS NULL OR unit_cost_minor_snapshot >= 0)",
        )?;
        ensure_column(
            &connection,
            "sale_items",
            "line_discount_minor",
            "ALTER TABLE sale_items ADD COLUMN line_discount_minor INTEGER NOT NULL DEFAULT 0 CHECK (line_discount_minor >= 0)",
        )?;
        ensure_column(
            &connection,
            "sale_items",
            "line_tax_minor",
            "ALTER TABLE sale_items ADD COLUMN line_tax_minor INTEGER NOT NULL DEFAULT 0 CHECK (line_tax_minor >= 0)",
        )?;
        connection
            .execute("UPDATE sales SET amount_tendered_minor = total_minor WHERE amount_tendered_minor = 0", [])
            .map_err(|error| format!("Could not prepare saved receipt amounts: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0006_cashier_workflows.sql"))
            .map_err(|error| format!("Could not prepare cashier workflow recovery: {error}"))?;

        connection
            .execute_batch(include_str!("../migrations/0007_identity_attendance.sql"))
            .map_err(|error| {
                format!("Could not prepare account security and attendance: {error}")
            })?;
        crate::attendance::reconcile(&connection, chrono::Utc::now())?;
        connection
            .execute_batch(include_str!("../migrations/0008_payment_options.sql"))
            .map_err(|error| format!("Could not prepare payment settings: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0009_sales_query_indexes.sql"))
            .map_err(|error| format!("Could not prepare sales reporting indexes: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0010_cash_drawer_ledger.sql"))
            .map_err(|error| format!("Could not prepare the cash drawer ledger: {error}"))?;
        connection
            .execute_batch(include_str!(
                "../migrations/0011_sale_line_financial_snapshots.sql"
            ))
            .map_err(|error| format!("Could not prepare sale refund records: {error}"))?;
        backfill_sale_line_components(&mut connection)?;
        connection
            .execute_batch(include_str!(
                "../migrations/0012_manager_refund_permission.sql"
            ))
            .map_err(|error| format!("Could not update manager refund access: {error}"))?;
        connection
            .execute_batch(include_str!(
                "../migrations/0013_payment_integration_requests.sql"
            ))
            .map_err(|error| {
                format!("Could not prepare payment provider request tracking: {error}")
            })?;
        connection
            .execute_batch(include_str!(
                "../migrations/0014_expense_report_indexes.sql"
            ))
            .map_err(|error| format!("Could not prepare expense reporting indexes: {error}"))?;
        connection
            .execute_batch(include_str!(
                "../migrations/0015_cash_drawer_history_index.sql"
            ))
            .map_err(|error| format!("Could not prepare drawer history indexes: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0016_audit_event_index.sql"))
            .map_err(|error| format!("Could not prepare audit history indexes: {error}"))?;
        ensure_column(
            &connection,
            "businesses",
            "owner_user_id",
            "ALTER TABLE businesses ADD COLUMN owner_user_id TEXT REFERENCES users(id) ON DELETE RESTRICT",
        )?;
        ensure_column(
            &connection,
            "roles",
            "workspace_mode",
            "ALTER TABLE roles ADD COLUMN workspace_mode TEXT NOT NULL DEFAULT 'management' CHECK (workspace_mode IN ('management', 'cashier'))",
        )?;
        ensure_column(
            &connection,
            "roles",
            "role_revision",
            "ALTER TABLE roles ADD COLUMN role_revision INTEGER NOT NULL DEFAULT 1 CHECK (role_revision > 0)",
        )?;
        connection
            .execute_batch(include_str!("../migrations/0017_staff_roles_owner.sql"))
            .map_err(|error| format!("Could not prepare staff roles and owner access: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0018_business_scoped_roles.sql"))
            .map_err(|error| format!("Could not enforce business-scoped staff roles: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0019_discount_approvals.sql"))
            .map_err(|error| format!("Could not prepare manager discount approvals: {error}"))?;
        connection
            .execute_batch(include_str!("../migrations/0020_accounting_journals.sql"))
            .map_err(|error| format!("Could not prepare local accounting journals: {error}"))?;
        connection
            .execute_batch("CREATE TABLE IF NOT EXISTS local_schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")
            .map_err(|error| format!("Could not prepare local migration tracking: {error}"))?;
        let accounting_purchase_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=21)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !accounting_purchase_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0021_accounting_purchases.sql"))
                .map_err(|error| format!("Could not extend local accounting journals: {error}"))?;
        }
        let supplier_billing_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=22)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !supplier_billing_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0022_supplier_billing.sql"))
                .map_err(|error| format!("Could not prepare supplier invoice tracking: {error}"))?;
        }
        let station_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=23)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !station_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0023_register_stations.sql"))
                .map_err(|error| format!("Could not prepare register station identity: {error}"))?;
        }
        let store_tax_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=24)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !store_tax_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0024_store_tax_settings.sql"))
                .map_err(|error| format!("Could not prepare store tax settings: {error}"))?;
        }
        let receipt_settings_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=25)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !receipt_settings_migration_applied {
            connection
                .execute_batch(include_str!(
                    "../migrations/0025_receipt_print_settings.sql"
                ))
                .map_err(|error| {
                    format!("Could not prepare receipt and printer settings: {error}")
                })?;
        }
        let cash_source_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=26)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !cash_source_migration_applied {
            connection
                .execute_batch(include_str!(
                    "../migrations/0026_cash_source_reconciliation.sql"
                ))
                .map_err(|error| {
                    format!("Could not prepare cash source reconciliation: {error}")
                })?;
        }
        let cash_opening_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=27)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !cash_opening_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0027_cash_opening_balance.sql"))
                .map_err(|error| {
                    format!("Could not prepare owner-controlled cash opening balances: {error}")
                })?;
        }
        let cash_locations_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=28)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !cash_locations_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0028_cash_locations.sql"))
                .map_err(|error| format!("Could not prepare persistent cash locations: {error}"))?;
        }
        let expense_reversals_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=29)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !expense_reversals_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0029_expense_reversals.sql"))
                .map_err(|error| format!("Could not prepare expense corrections: {error}"))?;
        }
        let expense_reimbursements_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=30)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !expense_reimbursements_migration_applied {
            connection
                .execute_batch(include_str!(
                    "../migrations/0030_expense_reimbursements.sql"
                ))
                .map_err(|error| {
                    format!("Could not prepare expense reimbursement tracking: {error}")
                })?;
        }
        let expense_cost_centers_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=31)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !expense_cost_centers_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0031_expense_cost_centers.sql"))
                .map_err(|error| {
                    format!("Could not prepare expense cost-center attribution: {error}")
                })?;
        }
        let expense_cost_center_reclasses_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=32)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !expense_cost_center_reclasses_migration_applied {
            connection
                .execute_batch(include_str!(
                    "../migrations/0032_expense_cost_center_reclassifications.sql"
                ))
                .map_err(|error| {
                    format!("Could not prepare expense cost-center correction history: {error}")
                })?;
        }
        let entity_images_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=33)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !entity_images_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0033_entity_images.sql"))
                .map_err(|error| {
                    format!("Could not prepare product and contact images: {error}")
                })?;
        }
        let staff_images_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=34)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !staff_images_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0034_staff_images.sql"))
                .map_err(|error| format!("Could not prepare staff profile images: {error}"))?;
        }
        let stock_counts_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=35)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !stock_counts_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0035_stock_counts.sql"))
                .map_err(|error| format!("Could not prepare stock count tracking: {error}"))?;
        }
        let product_brands_tags_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=36)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !product_brands_tags_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0036_product_brands_tags.sql"))
                .map_err(|error| format!("Could not prepare product brands and tags: {error}"))?;
        }
        let product_target_stock_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=37)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !product_target_stock_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0037_product_target_stock.sql"))
                .map_err(|error| format!("Could not prepare product reorder targets: {error}"))?;
        }
        let purchase_orders_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=38)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !purchase_orders_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0038_purchase_orders.sql"))
                .map_err(|error| format!("Could not prepare purchase orders: {error}"))?;
        }
        let accounting_period_closures_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=39)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !accounting_period_closures_migration_applied {
            connection
                .execute_batch(include_str!(
                    "../migrations/0039_accounting_period_closures.sql"
                ))
                .map_err(|error| {
                    format!("Could not prepare accounting period closures: {error}")
                })?;
        }
        let supplier_credit_notes_migration_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=40)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !supplier_credit_notes_migration_applied {
            connection
                .execute_batch(include_str!("../migrations/0040_supplier_credit_notes.sql"))
                .map_err(|error| {
                    format!("Could not prepare source-linked supplier credit notes: {error}")
                })?;
        }
        crate::accounting::backfill_existing(&mut connection)?;
        crate::stations::initialize_installation(&connection, &device_id)?;
        Ok(Self {
            data_dir: path,
            device_id,
            db: Mutex::new(connection),
            active_session: Mutex::new(None),
            session_gate: RwLock::new(()),
            passkey_operation: Mutex::new(()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplier_credit_note_migration_preserves_journals_and_accepts_credit_sources() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE businesses(id TEXT PRIMARY KEY);
             CREATE TABLE stores(id TEXT PRIMARY KEY, business_id TEXT NOT NULL REFERENCES businesses(id));
             CREATE TABLE users(id TEXT PRIMARY KEY);
             CREATE TABLE suppliers(id TEXT NOT NULL, store_id TEXT NOT NULL, UNIQUE(id,store_id));
             CREATE TABLE purchases(id TEXT NOT NULL, store_id TEXT NOT NULL, UNIQUE(id,store_id));
             CREATE TABLE purchase_items(id TEXT PRIMARY KEY);
             CREATE TABLE products(id TEXT PRIMARY KEY, store_id TEXT NOT NULL, stock_quantity INTEGER NOT NULL);
             CREATE TABLE accounting_accounts(id TEXT NOT NULL, business_id TEXT NOT NULL, UNIQUE(id,business_id));
             CREATE TABLE accounting_period_closures(business_id TEXT NOT NULL, store_id TEXT NOT NULL, from_utc TEXT NOT NULL, to_utc_exclusive TEXT NOT NULL);
             CREATE TABLE accounting_journals(
                 id TEXT PRIMARY KEY, business_id TEXT NOT NULL REFERENCES businesses(id), store_id TEXT NOT NULL REFERENCES stores(id),
                 source_type TEXT NOT NULL, source_id TEXT NOT NULL, currency_code TEXT NOT NULL, effective_at TEXT NOT NULL,
                 recorded_at TEXT NOT NULL, created_by TEXT REFERENCES users(id), description TEXT NOT NULL,
                 coverage_status TEXT NOT NULL, reversal_of TEXT REFERENCES accounting_journals(id),
                 UNIQUE(business_id,source_type,source_id), UNIQUE(id,business_id));
             CREATE TABLE accounting_journal_lines(
                 id TEXT PRIMARY KEY, business_id TEXT NOT NULL, journal_id TEXT NOT NULL, account_id TEXT NOT NULL,
                 line_number INTEGER NOT NULL, memo TEXT NOT NULL, debit_minor INTEGER NOT NULL, credit_minor INTEGER NOT NULL,
                 FOREIGN KEY(journal_id,business_id) REFERENCES accounting_journals(id,business_id),
                 FOREIGN KEY(account_id,business_id) REFERENCES accounting_accounts(id,business_id), UNIQUE(journal_id,line_number));
             INSERT INTO businesses VALUES('business-a');
             INSERT INTO stores VALUES('store-a','business-a');
             INSERT INTO users VALUES('owner-a');
             INSERT INTO accounting_accounts VALUES('cash','business-a');
             INSERT INTO accounting_journals VALUES('journal-old','business-a','store-a','sale','sale-old','GHS','2026-11-01','2026-11-01','owner-a','Existing sale','complete',NULL);
             INSERT INTO accounting_journal_lines VALUES('line-old','business-a','journal-old','cash',1,'Existing line',100,0);",
        )
        .unwrap();

        db.execute_batch(include_str!("../migrations/0040_supplier_credit_notes.sql"))
            .unwrap();

        let preserved: (String, i64) = db
            .query_row(
                "SELECT j.source_type,l.debit_minor FROM accounting_journals j JOIN accounting_journal_lines l ON l.journal_id=j.id WHERE j.id='journal-old'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preserved, ("sale".to_owned(), 100));
        db.execute(
            "INSERT INTO accounting_journals(id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,created_by,description,coverage_status,reversal_of)
             VALUES('journal-credit','business-a','store-a','supplier_credit','credit-a','GHS','2026-11-02','2026-11-02','owner-a','Supplier return','complete',NULL)",
            [],
        )
        .unwrap();
        assert!(db
            .execute(
                "UPDATE accounting_journals SET description='changed' WHERE id='journal-credit'",
                []
            )
            .is_err());
        let foreign_key_violations: i64 = db
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(foreign_key_violations, 0);
        let migration_recorded: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_schema_migrations WHERE version=40)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(migration_recorded);
    }

    #[test]
    fn product_brands_and_tags_are_business_scoped_and_keep_existing_products_unassigned() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE businesses(id TEXT PRIMARY KEY);
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT NOT NULL REFERENCES businesses(id));
             CREATE TABLE products(id TEXT PRIMARY KEY,store_id TEXT NOT NULL REFERENCES stores(id));
             INSERT INTO businesses VALUES('business-a'),('business-b');
             INSERT INTO stores VALUES('store-a1','business-a'),('store-a2','business-a'),('store-b1','business-b');
             INSERT INTO products VALUES('product-a1','store-a1'),('product-a2','store-a2'),('product-b1','store-b1');",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0036_product_brands_tags.sql"))
            .unwrap();

        let legacy_brand: Option<String> = db
            .query_row(
                "SELECT brand_id FROM products WHERE id='product-a1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_brand, None);

        db.execute(
            "INSERT INTO product_brands(id,business_id,name,created_at,updated_at) VALUES('brand-a','business-a','Aerojet','now','now')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO product_tag_catalog(id,business_id,name,created_at,updated_at) VALUES('tag-a','business-a','Seasonal','now','now')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO product_brands(id,business_id,name,created_at,updated_at) VALUES('brand-b','business-b','Aerojet','now','now')",
            [],
        )
        .unwrap();
        db.execute(
            "UPDATE products SET brand_id='brand-a' WHERE id='product-a1'",
            [],
        )
        .unwrap();
        db.execute(
            "UPDATE products SET brand_id='brand-a' WHERE id='product-a2'",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO product_tag_links(product_id,tag_id) VALUES('product-a1','tag-a')",
            [],
        )
        .unwrap();
        assert!(db
            .execute(
                "UPDATE products SET brand_id='brand-a' WHERE id='product-b1'",
                []
            )
            .is_err());
        assert!(db
            .execute(
                "INSERT INTO product_tag_links(product_id,tag_id) VALUES('product-b1','tag-a')",
                [],
            )
            .is_err());
        assert!(db
            .execute(
                "INSERT INTO product_brands(id,business_id,name,created_at,updated_at) VALUES('brand-a-duplicate','business-a','aEROJET','now','now')",
                [],
            )
            .is_err());

        let assigned_across_stores: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM products WHERE brand_id='brand-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assigned_across_stores, 2);
    }

    #[test]
    fn product_target_stock_migration_defaults_old_products_and_bounds_restock_targets() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE products(id TEXT PRIMARY KEY,store_id TEXT,is_active INTEGER,reorder_point INTEGER,stock_quantity INTEGER);
             INSERT INTO products VALUES('legacy-product','store-a',1,5,2);",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0037_product_target_stock.sql"))
            .unwrap();

        let old_target: i64 = db
            .query_row(
                "SELECT target_stock_quantity FROM products WHERE id='legacy-product'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_target, 0);
        db.execute(
            "UPDATE products SET target_stock_quantity=12 WHERE id='legacy-product'",
            [],
        )
        .unwrap();
        assert!(db
            .execute(
                "UPDATE products SET target_stock_quantity=1000001 WHERE id='legacy-product'",
                [],
            )
            .is_err());
        assert!(db
            .execute(
                "UPDATE products SET target_stock_quantity=-1 WHERE id='legacy-product'",
                [],
            )
            .is_err());
    }

    #[test]
    fn purchase_order_migration_preserves_legacy_receipts_and_adds_scoped_links() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT NOT NULL);
             CREATE TABLE suppliers(id TEXT PRIMARY KEY,store_id TEXT NOT NULL);
             CREATE TABLE users(id TEXT PRIMARY KEY,business_id TEXT NOT NULL);
             CREATE TABLE products(id TEXT PRIMARY KEY,store_id TEXT NOT NULL);
             CREATE TABLE purchases(id TEXT PRIMARY KEY,store_id TEXT NOT NULL REFERENCES stores(id),supplier_id TEXT REFERENCES suppliers(id),user_id TEXT NOT NULL REFERENCES users(id),reference TEXT,total_minor INTEGER NOT NULL,received_at TEXT NOT NULL,created_at TEXT NOT NULL);
             CREATE TABLE purchase_items(id TEXT PRIMARY KEY,purchase_id TEXT NOT NULL REFERENCES purchases(id),product_id TEXT NOT NULL REFERENCES products(id),sku TEXT NOT NULL,product_name TEXT NOT NULL,quantity INTEGER NOT NULL,unit_cost_minor INTEGER NOT NULL,line_total_minor INTEGER NOT NULL);
             INSERT INTO stores VALUES('store-a','business-a'),('store-b','business-b');
             INSERT INTO suppliers VALUES('supplier-a','store-a'),('supplier-b','store-b');
             INSERT INTO users VALUES('user-a','business-a'),('user-b','business-b');
             INSERT INTO products VALUES('product-a','store-a'),('product-b','store-b');
             INSERT INTO purchases VALUES('receipt-a','store-a',NULL,'user-a','INV-1',600,'2026-10-01T00:00:00Z','2026-10-01T00:00:00Z');
             INSERT INTO purchase_items VALUES('item-a','receipt-a','product-a','SKU-A','Coffee',2,300,600);",
        ).unwrap();
        db.execute_batch(include_str!("../migrations/0038_purchase_orders.sql"))
            .unwrap();

        let receipt: (i64, Option<String>) = db
            .query_row(
                "SELECT total_minor,purchase_order_id FROM purchases WHERE id='receipt-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(receipt, (600, None));
        let legacy_item_link: Option<String> = db
            .query_row(
                "SELECT purchase_order_line_id FROM purchase_items WHERE id='item-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_item_link, None);
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT version FROM local_schema_migrations WHERE version=38",
                [],
                |row| row.get(0)
            )
            .unwrap(),
            38
        );
        db.execute("INSERT INTO purchase_orders(id,store_id,supplier_id,created_by,order_number,created_at,updated_at) VALUES('po-a','store-a','supplier-a','user-a','PO-000001','now','now')", []).unwrap();
        db.execute("INSERT INTO purchase_order_lines(id,order_id,product_id,product_name,sku,ordered_quantity,expected_unit_cost_minor) VALUES('po-line-a','po-a','product-a','Coffee','SKU-A',5,250)", []).unwrap();
        assert!(db.execute("INSERT INTO purchase_order_lines(id,order_id,product_id,product_name,sku,ordered_quantity,expected_unit_cost_minor) VALUES('po-line-cross','po-a','product-b','Cross store','SKU-B',1,1)", []).is_err());
        assert!(db.execute("INSERT INTO purchase_orders(id,store_id,supplier_id,created_by,order_number,created_at,updated_at) VALUES('po-cross','store-a','supplier-b','user-a','PO-000002','now','now')", []).is_err());
        db.execute(
            "UPDATE purchase_orders SET status='placed' WHERE id='po-a'",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO purchases(id,store_id,supplier_id,user_id,reference,total_minor,received_at,created_at,purchase_order_id,idempotency_key) VALUES('receipt-po-a','store-a','supplier-a','user-a','DEL-1',280,'now','now','po-a','attempt-a')", []).unwrap();
        db.execute("INSERT INTO purchase_items(id,purchase_id,product_id,sku,product_name,quantity,unit_cost_minor,line_total_minor,purchase_order_line_id) VALUES('item-po-a','receipt-po-a','product-a','SKU-A','Coffee',1,280,280,'po-line-a')", []).unwrap();
        db.execute(
            "UPDATE purchase_order_lines SET received_quantity=1 WHERE id='po-line-a'",
            [],
        )
        .unwrap();
        assert!(db.execute("INSERT INTO purchases(id,store_id,supplier_id,user_id,reference,total_minor,received_at,created_at,purchase_order_id,idempotency_key) VALUES('receipt-po-replay','store-a','supplier-a','user-a','DEL-2',280,'now','now','po-a','attempt-a')", []).is_err());
        assert!(db.execute("INSERT INTO purchase_items(id,purchase_id,product_id,sku,product_name,quantity,unit_cost_minor,line_total_minor,purchase_order_line_id) VALUES('item-po-cross','receipt-po-a','product-b','SKU-B','Cross store',1,280,280,'po-line-a')", []).is_err());
        db.execute("INSERT INTO purchases(id,store_id,supplier_id,user_id,reference,total_minor,received_at,created_at,purchase_order_id,idempotency_key) VALUES('receipt-po-over','store-a','supplier-a','user-a','DEL-3',1400,'now','now','po-a','attempt-over')", []).unwrap();
        assert!(db.execute("INSERT INTO purchase_items(id,purchase_id,product_id,sku,product_name,quantity,unit_cost_minor,line_total_minor,purchase_order_line_id) VALUES('item-po-over','receipt-po-over','product-a','SKU-A','Coffee',5,280,1400,'po-line-a')", []).is_err());
    }

    #[test]
    fn expense_cost_center_migration_preserves_legacy_and_rejects_unreviewed_reclassification() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE businesses(id TEXT PRIMARY KEY);
             CREATE TABLE stores(id TEXT PRIMARY KEY);
             CREATE TABLE users(id TEXT PRIMARY KEY);
             CREATE TABLE roles(id TEXT PRIMARY KEY,business_id TEXT,role_key TEXT,permissions_json TEXT);
             CREATE TABLE register_stations(business_id TEXT,store_id TEXT,station_key TEXT,display_name TEXT,is_active INTEGER);
             CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,amount_minor INTEGER,occurred_at TEXT);
             INSERT INTO businesses VALUES('business-a');
             INSERT INTO stores VALUES('store-a');
             INSERT INTO users VALUES('owner-a');
             INSERT INTO roles VALUES('role-owner','business-a','owner','[\"expenses:read\"]'),('role-admin','business-a','admin','[]'),('role-manager','business-a','manager','[]');
             INSERT INTO register_stations VALUES('business-a','store-a','station-a','Main register',1);
             INSERT INTO expenses VALUES('legacy-expense','store-a',1200,'2026-10-01T00:00:00Z');",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0031_expense_cost_centers.sql"))
            .unwrap();
        db.execute_batch(include_str!(
            "../migrations/0032_expense_cost_center_reclassifications.sql"
        ))
        .unwrap();

        let legacy_kind: String = db
            .query_row(
                "SELECT cost_center_kind FROM expenses WHERE id='legacy-expense'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_kind, "legacy_unassigned");
        let owner_permissions: String = db
            .query_row(
                "SELECT permissions_json FROM roles WHERE role_key='owner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let admin_permissions: String = db
            .query_row(
                "SELECT permissions_json FROM roles WHERE role_key='admin'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let manager_permissions: String = db
            .query_row(
                "SELECT permissions_json FROM roles WHERE role_key='manager'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(owner_permissions.contains("expenses:reclassify"));
        assert!(admin_permissions.contains("expenses:reclassify"));
        assert!(!manager_permissions.contains("expenses:reclassify"));

        db.execute(
            "INSERT INTO expenses(id,store_id,amount_minor,occurred_at,cost_center_kind,cost_center_station_key,cost_center_station_name_snapshot)
             VALUES('station-expense','store-a',500,'2026-10-02T00:00:00Z','station_direct','station-a','Main register')",
            [],
        )
        .unwrap();
        assert!(db.execute(
            "INSERT INTO expenses(id,store_id,amount_minor,occurred_at,cost_center_kind,cost_center_station_key,cost_center_station_name_snapshot)
             VALUES('invalid-expense','store-a',500,'2026-10-02T00:00:00Z','station_direct','missing','Missing')",
            [],
        ).is_err());
        assert!(db
            .execute(
                "UPDATE expenses SET cost_center_kind='store_overhead' WHERE id='legacy-expense'",
                [],
            )
            .is_err());
    }

    #[test]
    fn cash_source_migration_preserves_legacy_unknowns_and_guards_linked_outflows() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE users(id TEXT PRIMARY KEY);
             CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,user_id TEXT,category TEXT,description TEXT,amount_minor INTEGER,payment_method TEXT,occurred_at TEXT,created_at TEXT);
             CREATE TABLE supplier_payments(id TEXT PRIMARY KEY,store_id TEXT,payment_method TEXT,amount_minor INTEGER,recorded_by TEXT,paid_at TEXT);
             CREATE TABLE cash_shifts(id TEXT PRIMARY KEY,store_id TEXT,status TEXT);
             CREATE TABLE cash_movements(id TEXT PRIMARY KEY,shift_id TEXT,store_id TEXT,user_id TEXT,movement_type TEXT,amount_minor INTEGER,note TEXT,created_at TEXT);
             INSERT INTO users VALUES('user-a');
             INSERT INTO expenses VALUES('legacy-cash','store-a','user-a','Fuel',NULL,500,'cash','2026-10-01T10:00:00Z','2026-10-01T10:00:00Z');
             INSERT INTO expenses VALUES('legacy-bank','store-a','user-a','Rent',NULL,1000,'bank_transfer','2026-10-01T11:00:00Z','2026-10-01T11:00:00Z');
             INSERT INTO supplier_payments VALUES('legacy-payment','store-a','cash',700,'user-a','2026-10-01T11:30:00Z');
             INSERT INTO cash_shifts VALUES('shift-a','store-a','open'),('shift-other','store-b','open');
             INSERT INTO cash_movements VALUES('manual','shift-a','store-a','user-a','paid_out',10,'Manual movement','2026-10-01T12:00:00Z');",
        )
        .unwrap();

        db.execute_batch(include_str!(
            "../migrations/0026_cash_source_reconciliation.sql"
        ))
        .unwrap();

        let migrated: (String, String) = db
            .query_row(
                "SELECT (SELECT cash_origin FROM expenses WHERE id='legacy-cash'),(SELECT cash_origin FROM expenses WHERE id='legacy-bank')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            migrated,
            ("legacy_unknown".to_owned(), "not_applicable".to_owned())
        );
        let payment_origin: String = db
            .query_row(
                "SELECT cash_origin FROM supplier_payments WHERE id='legacy-payment'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payment_origin, "legacy_unknown");
        assert!(db
            .execute(
                "UPDATE expenses SET cash_origin='drawer' WHERE id='legacy-cash'",
                []
            )
            .is_err());
        assert!(db
            .execute("DELETE FROM expenses WHERE id='legacy-cash'", [])
            .is_err());

        db.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at,cash_origin,idempotency_key) VALUES('drawer-expense','store-a','user-a','Fuel',NULL,300,'cash','2026-10-01T13:00:00Z','2026-10-01T13:00:00Z','drawer','retry-a')",
            [],
        )
        .unwrap();
        let movement = "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at,source_type,source_id) VALUES(?1,?2,?3,'user-a','paid_out',?4,'Expense','2026-10-01T13:00:00Z','expense','drawer-expense')";
        db.execute(movement, params!["linked-a", "shift-a", "store-a", 300])
            .unwrap();
        assert!(db
            .execute(
                movement,
                params!["linked-duplicate", "shift-a", "store-a", 300]
            )
            .is_err());
        assert!(db
            .execute(movement, params!["wrong-amount", "shift-a", "store-a", 301])
            .is_err());
        assert!(db
            .execute(
                movement,
                params!["wrong-store", "shift-other", "store-a", 300]
            )
            .is_err());
        assert!(db
            .execute(
                "UPDATE cash_movements SET amount_minor=0 WHERE id='linked-a'",
                []
            )
            .is_err());
        assert!(db
            .execute("DELETE FROM cash_movements WHERE id='linked-a'", [])
            .is_err());
    }

    #[test]
    fn legacy_sale_lines_receive_stable_discount_and_tax_allocations() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE sales(id TEXT PRIMARY KEY,subtotal_minor INTEGER,discount_minor INTEGER,tax_minor INTEGER,total_minor INTEGER);
             CREATE TABLE sale_items(id TEXT PRIMARY KEY,sale_id TEXT,quantity INTEGER,unit_price_minor INTEGER,line_discount_minor INTEGER DEFAULT 0,line_tax_minor INTEGER DEFAULT 0);
             CREATE TABLE refunds(id TEXT,sale_id TEXT);
             INSERT INTO sales VALUES('s',300,1,15,314);
             INSERT INTO sale_items VALUES('i1','s',1,100,0,0);
             INSERT INTO sale_items VALUES('i2','s',1,100,0,0);
             INSERT INTO sale_items VALUES('i3','s',1,100,0,0);",
        )
        .unwrap();
        backfill_sale_line_components(&mut db).unwrap();
        backfill_sale_line_components(&mut db).unwrap();
        let allocations = db
            .prepare("SELECT line_discount_minor,line_tax_minor FROM sale_items ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(allocations, vec![(1, 5), (0, 5), (0, 5)]);
    }

    #[test]
    fn cash_location_migration_tracks_openings_transfers_and_immutable_outflows() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE businesses(id TEXT PRIMARY KEY,currency_code TEXT NOT NULL);
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT,UNIQUE(id,business_id));
             CREATE TABLE users(id TEXT PRIMARY KEY,business_id TEXT,full_name TEXT);
             CREATE TABLE register_stations(store_id TEXT,business_id TEXT,station_key TEXT,display_name TEXT,is_active INTEGER,created_at TEXT,UNIQUE(store_id,station_key));
             CREATE TABLE cash_shifts(id TEXT PRIMARY KEY,store_id TEXT,station_key TEXT,status TEXT);
             CREATE TABLE cash_movements(id TEXT PRIMARY KEY,shift_id TEXT,store_id TEXT,user_id TEXT,movement_type TEXT,amount_minor INTEGER,note TEXT,created_at TEXT,source_type TEXT,source_id TEXT);
             CREATE TABLE cash_close_reports(id TEXT PRIMARY KEY,shift_id TEXT,store_id TEXT,closed_by TEXT,close_type TEXT,expected_cash_minor INTEGER,counted_cash_minor INTEGER,difference_minor INTEGER,report_json TEXT,created_at TEXT);
             CREATE TABLE refunds(id TEXT PRIMARY KEY,store_id TEXT);
             CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,business_id TEXT,user_id TEXT,category TEXT,description TEXT,amount_minor INTEGER,cash_origin TEXT,payment_method TEXT,cash_source_note TEXT,occurred_at TEXT,created_at TEXT,UNIQUE(id,store_id));
             CREATE TABLE supplier_payments(id TEXT PRIMARY KEY,store_id TEXT,cash_origin TEXT,payment_method TEXT,amount_minor INTEGER,cash_source_note TEXT,recorded_by TEXT,paid_at TEXT);
             CREATE TABLE sales(id TEXT PRIMARY KEY,store_id TEXT,cash_shift_id TEXT);
             CREATE TABLE payments(id TEXT PRIMARY KEY,sale_id TEXT,method TEXT,amount_minor INTEGER);
             CREATE TABLE refund_tenders(id TEXT PRIMARY KEY,refund_id TEXT,method TEXT,amount_minor INTEGER);
             INSERT INTO businesses VALUES('business-a','GHS');
             INSERT INTO stores VALUES('store-a','business-a');
             INSERT INTO users VALUES('user-a','business-a','Owner');
             INSERT INTO register_stations VALUES('store-a','business-a','main','Main register',1,'2026-10-01T00:00:00Z');",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0028_cash_locations.sql"))
            .unwrap();

        let register_id: String = db
            .query_row(
                "SELECT id FROM cash_locations WHERE store_id='store-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        db.execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,actor_user_id,effective_at,created_at,note) VALUES('register-opening','business-a','store-a',?1,'opening','in',1000,'opening',?1,'user-a','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z','Counted till')",
            [&register_id],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_locations(id,business_id,store_id,name,location_type,currency_code,is_active,created_by,created_at) VALUES('safe-a','business-a','store-a','Main safe','safe','GHS',1,'user-a','2026-10-01T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,actor_user_id,effective_at,created_at,note) VALUES('safe-opening','business-a','store-a','safe-a','opening','in',500,'opening','safe-a','user-a','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z','Counted safe')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_transfers(id,business_id,store_id,from_location_id,to_location_id,amount_minor,idempotency_key,note,source_type,source_id,created_by,created_at) VALUES('transfer-a','business-a','store-a','safe-a',?1,200,'retry-a','Till float','manual','transfer-a','user-a','2026-10-01T00:00:00Z')",
            [&register_id],
        ).unwrap();
        for (id, location, direction, key) in [
            ("transfer-out", "safe-a", "out", "retry-a:out"),
            ("transfer-in", register_id.as_str(), "in", "retry-a:in"),
        ] {
            db.execute(
                "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,transfer_id,idempotency_key,actor_user_id,effective_at,created_at,note) VALUES(?1,'business-a','store-a',?2,'transfer',?3,200,'transfer','transfer-a','transfer-a',?4,'user-a','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z','Till float')",
                params![id, location, direction, key],
            ).unwrap();
        }
        let total: i64 = db.query_row(
            "SELECT SUM(CASE WHEN direction='in' THEN amount_minor ELSE -amount_minor END) FROM cash_location_events",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(total, 1500);
        assert!(db
            .execute(
                "UPDATE cash_location_events SET amount_minor=1 WHERE id='transfer-in'",
                []
            )
            .is_err());
        assert!(db
            .execute(
                "DELETE FROM cash_location_events WHERE id='transfer-in'",
                []
            )
            .is_err());
        assert!(db
            .execute(
                "UPDATE cash_location_transfers SET amount_minor=1 WHERE id='transfer-a'",
                []
            )
            .is_err());
        db.execute(
            "INSERT INTO cash_shifts(id,store_id,station_key,status,cash_location_id) VALUES('shift-a','store-a','main','open',?1)",
            [&register_id],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,shift_id,event_type,direction,amount_minor,source_type,source_id,actor_user_id,effective_at,created_at,note,review_status) VALUES('variance-a','business-a','store-a',?1,'shift-a','shift_open_variance','out',5,'shift','shift-a','user-a','2026-10-01T01:00:00Z','2026-10-01T01:00:00Z','Counted five less','pending')",
            [&register_id],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_event_reviews(id,business_id,store_id,event_id,reviewed_by,note,reviewed_at) VALUES('review-a','business-a','store-a','variance-a','user-a','Verified physical count','2026-10-01T02:00:00Z')",
            [],
        ).unwrap();
        assert!(db
            .execute(
                "UPDATE cash_location_event_reviews SET note='changed' WHERE id='review-a'",
                []
            )
            .is_err());

        db.execute(
            "INSERT INTO expenses(id,store_id,business_id,user_id,category,description,amount_minor,cash_origin,payment_method,occurred_at,created_at) VALUES('expense-a','store-a','business-a','user-a','Utilities','Power',1250,'not_applicable','bank_transfer','2026-10-02T00:00:00Z','2026-10-02T00:00:00Z')",
            [],
        ).unwrap();
        db.execute_batch(include_str!("../migrations/0029_expense_reversals.sql"))
            .unwrap();
        db.execute_batch(
            "CREATE TRIGGER cash_movements_source_validate_insert BEFORE INSERT ON cash_movements BEGIN SELECT 1; END;",
        )
        .unwrap();
        db.execute(
            "INSERT INTO expense_reversals(id,business_id,store_id,expense_id,amount_minor,payment_method,return_reference,reason,returned_at,idempotency_key,recorded_by,created_at) VALUES('legacy-return','business-a','store-a','expense-a',1250,'bank_transfer','BANK-OLD-1','Duplicate utility payment','2026-10-03T00:00:00Z','legacy-key','user-a','2026-10-03T00:00:00Z')",
            [],
        ).unwrap();
        db.execute_batch(include_str!(
            "../migrations/0030_expense_reimbursements.sql"
        ))
        .unwrap();

        let migrated: (i64, String) = db.query_row(
            "SELECT amount_minor,return_reference FROM expense_reimbursements WHERE id='legacy-return'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(migrated, (1250, "BANK-OLD-1".to_owned()));
        let review_still_links: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_location_event_reviews r JOIN cash_location_events e ON e.id=r.event_id WHERE r.id='review-a' AND e.id='variance-a')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(review_still_links);
        let foreign_key_errors: i64 = db
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(foreign_key_errors, 0);
        assert!(db.execute(
            "INSERT INTO expense_reimbursements(id,business_id,store_id,expense_id,amount_minor,return_method,return_reference,reason,idempotency_key,recorded_by,returned_at,created_at) VALUES('over-return','business-a','store-a','expense-a',1,'bank_transfer','BANK-TOO-MUCH','Over the expense limit','over-key','user-a','2026-10-04T00:00:00Z','2026-10-04T00:00:00Z')",
            [],
        ).is_err());

        db.execute(
            "INSERT INTO expenses(id,store_id,business_id,user_id,category,description,amount_minor,cash_origin,payment_method,occurred_at,created_at) VALUES('expense-b','store-a','business-a','user-a','Supplies','Returned overpayment',150,'not_applicable','bank_transfer','2026-10-02T00:00:00Z','2026-10-02T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO expense_reimbursements(id,business_id,store_id,expense_id,amount_minor,return_method,reason,cash_location_id,idempotency_key,recorded_by,returned_at,created_at) VALUES('cash-return','business-a','store-a','expense-b',50,'cash','Partial supplier reimbursement','safe-a','cash-key','user-a','2026-10-04T00:00:00Z','2026-10-04T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,actor_user_id,effective_at,created_at,note) VALUES('cash-return-event','business-a','store-a','safe-a','expense_reimbursement','in',50,'expense_reimbursement','cash-return','user-a','2026-10-04T00:00:00Z','2026-10-04T00:00:00Z','Returned supplies')",
            [],
        ).unwrap();
        let safe_balance: i64 = db.query_row(
            "SELECT SUM(CASE WHEN direction='in' THEN amount_minor ELSE -amount_minor END) FROM cash_location_events WHERE cash_location_id='safe-a'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(safe_balance, 350);

        db.execute(
            "INSERT INTO cash_shifts(id,store_id,station_key,status,cash_location_id) VALUES('shift-current','store-a','main','open',?1)",
            [&register_id],
        ).unwrap();
        db.execute(
            "INSERT INTO expenses(id,store_id,business_id,user_id,category,description,amount_minor,cash_origin,payment_method,occurred_at,created_at) VALUES('expense-c','store-a','business-a','user-a','Utilities','Returned overpayment',150,'not_applicable','bank_transfer','2026-10-02T00:00:00Z','2026-10-02T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO expense_reimbursements(id,business_id,store_id,expense_id,amount_minor,return_method,reason,cash_location_id,cash_shift_id,idempotency_key,recorded_by,returned_at,created_at) VALUES('register-return','business-a','store-a','expense-c',50,'cash','Cash received in current drawer',?1,'shift-current','register-key','user-a','2026-10-04T01:00:00Z','2026-10-04T01:00:00Z')",
            [&register_id],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,shift_id,event_type,direction,amount_minor,source_type,source_id,actor_user_id,effective_at,created_at,note) VALUES('register-return-event','business-a','store-a',?1,'shift-current','expense_reimbursement','in',50,'expense_reimbursement','register-return','user-a','2026-10-04T01:00:00Z','2026-10-04T01:00:00Z','Returned utilities')",
            [&register_id],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at,source_type,source_id) VALUES('register-return-movement','shift-current','store-a','user-a','paid_in',50,'Returned utilities','2026-10-04T01:00:00Z','expense_reimbursement','register-return')",
            [],
        ).unwrap();
        let shift_paid_in: i64 = db.query_row(
            "SELECT SUM(amount_minor) FROM cash_movements WHERE shift_id='shift-current' AND source_type='expense_reimbursement'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(shift_paid_in, 50);

        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        db.execute(
            "INSERT INTO expenses(id,store_id,business_id,user_id,category,description,amount_minor,cash_origin,payment_method,occurred_at,created_at) VALUES('expense-d','store-a','business-a','user-a','Utilities','Second returned payment',150,'not_applicable','bank_transfer','2026-10-02T00:00:00Z','2026-10-02T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO expense_reimbursements(id,business_id,store_id,expense_id,amount_minor,return_method,reason,cash_location_id,cash_shift_id,idempotency_key,recorded_by,returned_at,created_at) VALUES('register-return-late','business-a','store-a','expense-d',50,'cash','Cash arrived after shift close',?1,'shift-current','late-key','user-a','2026-10-04T02:00:00Z','2026-10-04T02:00:00Z')",
            [&register_id],
        ).unwrap();
        db.execute(
            "UPDATE cash_shifts SET status='closed' WHERE id='shift-current'",
            [],
        )
        .unwrap();
        assert!(db.execute(
            "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at,source_type,source_id) VALUES('late-return-movement','shift-current','store-a','user-a','paid_in',50,'Returned utilities','2026-10-04T02:00:00Z','expense_reimbursement','register-return-late')",
            [],
        ).is_err());
        db.execute_batch("ROLLBACK").unwrap();
    }
}
