// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use pos_core::{allocate_sale_components, calculate_sale_totals, PricedLine};
use rand_core::OsRng;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{collections::HashSet, ops::Deref, sync::RwLockReadGuard};
use tauri::State;
use uuid::Uuid;

use crate::{
    db::AppState,
    models::{
        AttachSaleCustomerInput, BootstrapStatus, Category, CategoryInput, CheckoutInput,
        CompletedRegisterSale, CreateStaffMemberInput, Customer, CustomerInput, DashboardSummary,
        Expense, ExpenseInput, InitialSetupInput, LoginInput, PaymentMethod, Product, ProductInput,
        PurchaseInput, PurchaseSummary, RegisterAction, RegisterEditInput, RegisterTicket,
        RegisterTicketLine, RegisterWorkspaceView, SaleReceipt, SaleReceiptItem, SessionUser,
        StaffProfile, StockAdjustmentInput, StorePreferences, StorePreferencesInput, Supplier,
        SupplierInput, SwitchStaffProfileInput, TemporaryCashierModeInput,
    },
};
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn normalized_text(value: &str) -> String {
    value.trim().to_owned()
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "Could not protect the new password.".to_owned())
}

pub(crate) fn verify_password(password: &str, encoded_hash: &str) -> bool {
    PasswordHash::new(encoded_hash)
        .map(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
        .unwrap_or(false)
}

pub(crate) struct ActiveSession<'a> {
    _gate: RwLockReadGuard<'a, ()>,
    pub(crate) user: SessionUser,
}

impl Deref for ActiveSession<'_> {
    type Target = SessionUser;

    fn deref(&self) -> &Self::Target {
        &self.user
    }
}

pub(crate) fn active_session<'a>(
    state: &'a AppState,
    permission: &str,
) -> Result<ActiveSession<'a>, String> {
    let gate = state.session_gate.read().map_err(|_| {
        "The local session is unavailable. Restart NimbleTill and sign in again.".to_owned()
    })?;
    let cached = state
        .active_session
        .lock()
        .map_err(|_| {
            "The local session is unavailable. Restart Smart POS and sign in again.".to_owned()
        })?
        .clone()
        .ok_or_else(|| "Sign in to continue.".to_owned())?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let refreshed = connection
        .query_row(
            "SELECT u.id,u.business_id,s.id,u.full_name,u.username,r.role_key,r.display_name,r.permissions_json,
                    b.name,s.name,b.currency_code,r.workspace_mode,COALESCE(b.owner_user_id=u.id,0)
             FROM users u
             JOIN roles r ON r.id=u.role_id AND r.business_id=u.business_id
             JOIN businesses b ON b.id=u.business_id
             JOIN stores s ON s.id=?3 AND s.business_id=b.id AND s.is_active=1
             WHERE u.id=?1 AND u.business_id=?2 AND u.is_active=1",
            params![cached.user_id,cached.business_id,cached.store_id],
            |row| Ok(SessionUser {
                user_id:row.get(0)?,business_id:row.get(1)?,store_id:row.get(2)?,full_name:row.get(3)?,username:row.get(4)?,
                role_key:row.get(5)?,role_name:row.get(6)?,permissions:serde_json::from_str(&row.get::<_,String>(7)?).unwrap_or_default(),
                business_name:row.get(8)?,store_name:row.get(9)?,currency_code:row.get(10)?,workspace_mode:row.get(11)?,is_owner:row.get::<_,i64>(12)?!=0,
            }),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    drop(connection);
    let Some(mut session) = refreshed else {
        *state
            .active_session
            .lock()
            .map_err(|_| "The local session is unavailable.")? = None;
        return Err("Your account or store access changed. Sign in again.".to_owned());
    };
    let temporary_cashier = cached
        .permissions
        .iter()
        .any(|value| value == "pos:transact");
    if temporary_cashier
        && matches!(session.role_key.as_str(), "owner" | "admin" | "manager")
        && session.permissions.iter().any(|value| value == "pos:use")
    {
        session.permissions.push("pos:transact".to_owned());
    }
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.")? = Some(session.clone());
    if !permission.is_empty() && !session.permissions.iter().any(|value| value == permission) {
        return Err("Your account does not have permission to do that.".to_owned());
    }
    Ok(ActiveSession {
        _gate: gate,
        user: session,
    })
}

pub(crate) fn require_pos_transaction(session: &SessionUser) -> Result<(), String> {
    let temporary_cashier = session
        .permissions
        .iter()
        .any(|value| value == "pos:transact");
    let cashier_profile = session.workspace_mode == "cashier"
        && !matches!(session.role_key.as_str(), "owner" | "admin" | "manager");
    if temporary_cashier || cashier_profile {
        Ok(())
    } else {
        Err("Checkout is locked. Enable temporary cashier mode to complete a sale.".to_owned())
    }
}

#[cfg(test)]
mod pos_transaction_tests {
    use super::require_pos_transaction;
    use crate::models::SessionUser;

    fn session(role_key: &str, workspace_mode: &str, permissions: &[&str]) -> SessionUser {
        SessionUser {
            user_id: "staff-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Taylor Staff".into(),
            username: "taylor".into(),
            role_key: role_key.into(),
            role_name: role_key.into(),
            workspace_mode: workspace_mode.into(),
            is_owner: role_key == "owner",
            permissions: permissions.iter().map(|value| (*value).into()).collect(),
            business_name: "Test business".into(),
            store_name: "Main store".into(),
            currency_code: "GHS".into(),
        }
    }

    #[test]
    fn management_accounts_can_view_pos_but_cannot_transact_without_temporary_mode() {
        for role in ["owner", "admin", "manager"] {
            assert!(require_pos_transaction(&session(role, "management", &["pos:use"])).is_err());
        }
    }

    #[test]
    fn management_accounts_can_transact_with_the_temporary_capability() {
        for role in ["owner", "admin", "manager"] {
            assert!(require_pos_transaction(&session(
                role,
                "management",
                &["pos:use", "pos:transact"],
            ))
            .is_ok());
        }
    }

    #[test]
    fn cashier_profiles_can_transact_but_manager_mode_cannot_bypass_the_toggle() {
        assert!(require_pos_transaction(&session("cashier", "cashier", &["pos:use"])).is_ok());
        assert!(require_pos_transaction(&session("manager", "cashier", &["pos:use"])).is_err());
    }
}

pub(crate) fn add_audit_event(
    connection: &Connection,
    session: &SessionUser,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    details: serde_json::Value,
) -> Result<(), String> {
    let details_json = serde_json::to_string(&details).map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO audit_events (id, business_id, store_id, user_id, action, entity_type, entity_id, details_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                Uuid::new_v4().to_string(),
                session.business_id,
                session.store_id,
                session.user_id,
                action,
                entity_type,
                entity_id,
                details_json,
                now()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn read_store_preferences(
    connection: &Connection,
    store_id: &str,
) -> Result<StorePreferences, String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO store_preferences (store_id) VALUES (?1)",
            [store_id],
        )
        .map_err(|error| error.to_string())?;
    connection
        .query_row(
            "SELECT cashier_start_at_register, cashier_auto_fullscreen, customer_display_enabled, customer_display_fullscreen,
                    customer_display_monitor_key, customer_display_idle_message, receipt_header_text, receipt_footer_text, receipt_paper_width_mm
             FROM store_preferences WHERE store_id = ?1",
            [store_id],
            |row| {
                Ok(StorePreferences {
                    cashier_start_at_register: row.get::<_, i64>(0)? != 0,
                    cashier_auto_fullscreen: row.get::<_, i64>(1)? != 0,
                    customer_display_enabled: row.get::<_, i64>(2)? != 0,
                    customer_display_fullscreen: row.get::<_, i64>(3)? != 0,
                    customer_display_monitor_key: row.get(4)?,
                    customer_display_idle_message: row.get(5)?,
                    receipt_header_text: row.get(6)?,
                    receipt_footer_text: row.get(7)?,
                    receipt_paper_width_mm: row.get(8)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_register_preferences(state: State<'_, AppState>) -> Result<StorePreferences, String> {
    let session = active_session(&state, "pos:use")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    read_store_preferences(&connection, &session.store_id)
}

#[tauri::command]
pub fn get_store_preferences(state: State<'_, AppState>) -> Result<StorePreferences, String> {
    let session = active_session(&state, "settings:write")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    read_store_preferences(&connection, &session.store_id)
}

#[tauri::command]
pub fn save_store_preferences(
    input: StorePreferencesInput,
    state: State<'_, AppState>,
) -> Result<StorePreferences, String> {
    let session = active_session(&state, "settings:write")?;
    let idle_message = normalized_text(&input.customer_display_idle_message);
    if idle_message.chars().count() > 160 {
        return Err("Keep the customer display greeting under 160 characters.".to_owned());
    }
    let monitor_key = input
        .customer_display_monitor_key
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    if monitor_key
        .as_ref()
        .is_some_and(|value| value.chars().count() > 250)
    {
        return Err("The selected display identifier is too long.".to_owned());
    }
    let receipt_header_text = normalized_text(&input.receipt_header_text);
    let receipt_footer_text = normalized_text(&input.receipt_footer_text);
    if receipt_header_text.chars().count() > 160 {
        return Err("Keep the receipt header under 160 characters.".to_owned());
    }
    if receipt_footer_text.chars().count() > 240 {
        return Err("Keep the receipt footer under 240 characters.".to_owned());
    }
    if !matches!(input.receipt_paper_width_mm, 58 | 80) {
        return Err("Choose a supported receipt width: 58 mm or 80 mm.".to_owned());
    }

    let preferences = StorePreferences {
        cashier_start_at_register: input.cashier_start_at_register,
        cashier_auto_fullscreen: input.cashier_auto_fullscreen,
        customer_display_enabled: input.customer_display_enabled,
        customer_display_fullscreen: input.customer_display_fullscreen,
        customer_display_monitor_key: monitor_key,
        customer_display_idle_message: idle_message,
        receipt_header_text,
        receipt_footer_text,
        receipt_paper_width_mm: input.receipt_paper_width_mm,
    };
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let timestamp = now();
    transaction
        .execute(
            "INSERT INTO store_preferences (
                store_id, cashier_start_at_register, cashier_auto_fullscreen, customer_display_enabled, customer_display_fullscreen,
                customer_display_monitor_key, customer_display_idle_message, updated_at,
                receipt_header_text, receipt_footer_text, receipt_paper_width_mm
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(store_id) DO UPDATE SET
                cashier_start_at_register = excluded.cashier_start_at_register,
                cashier_auto_fullscreen = excluded.cashier_auto_fullscreen,
                customer_display_enabled = excluded.customer_display_enabled,
                customer_display_fullscreen = excluded.customer_display_fullscreen,
                customer_display_monitor_key = excluded.customer_display_monitor_key,
                customer_display_idle_message = excluded.customer_display_idle_message,
                receipt_header_text = excluded.receipt_header_text,
                receipt_footer_text = excluded.receipt_footer_text,
                receipt_paper_width_mm = excluded.receipt_paper_width_mm,
                updated_at = excluded.updated_at",
            params![
                session.store_id,
                preferences.cashier_start_at_register as i64,
                preferences.cashier_auto_fullscreen as i64,
                preferences.customer_display_enabled as i64,
                preferences.customer_display_fullscreen as i64,
                preferences.customer_display_monitor_key,
                preferences.customer_display_idle_message,
                timestamp,
                preferences.receipt_header_text,
                preferences.receipt_footer_text,
                preferences.receipt_paper_width_mm,
            ],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &session,
        "store_display_preferences_updated",
        "store",
        Some(&session.store_id),
        serde_json::json!({
            "cashierAutoFullscreen": preferences.cashier_auto_fullscreen,
            "cashierStartAtRegister": preferences.cashier_start_at_register,
            "customerDisplayEnabled": preferences.customer_display_enabled,
            "customerDisplayFullscreen": preferences.customer_display_fullscreen,
            "customerDisplayMonitorKey": preferences.customer_display_monitor_key,
        }),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(preferences)
}

fn product_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Product> {
    let tag_ids_json: String = row.get(15)?;
    let tags_json: String = row.get(16)?;
    Ok(Product {
        id: row.get(0)?,
        sku: row.get(1)?,
        barcode: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        category_id: row.get(5)?,
        category_name: row.get(6)?,
        supplier_id: row.get(7)?,
        brand_id: row.get(8)?,
        brand_name: row.get(9)?,
        price_minor: row.get(10)?,
        cost_minor: row.get(11)?,
        stock_quantity: row.get(12)?,
        reorder_point: row.get(13)?,
        target_stock_quantity: row.get(17)?,
        is_active: row.get::<_, i64>(14)? != 0,
        tag_ids: serde_json::from_str(&tag_ids_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                15,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        tags: serde_json::from_str(&tags_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                16,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
    })
}

const PRODUCT_SELECT: &str =
    "SELECT p.id, p.sku, p.barcode, p.name, p.description, p.category_id, c.name, p.supplier_id,
            p.brand_id, b.name, p.price_minor, p.cost_minor, p.stock_quantity, p.reorder_point, p.is_active,
            COALESCE((SELECT json_group_array(tag_id) FROM (
                SELECT l.tag_id FROM product_tag_links l JOIN product_tag_catalog t ON t.id=l.tag_id
                WHERE l.product_id=p.id ORDER BY t.name COLLATE NOCASE)), '[]'),
            COALESCE((SELECT json_group_array(name) FROM (
                SELECT t.name FROM product_tag_links l JOIN product_tag_catalog t ON t.id=l.tag_id
                WHERE l.product_id=p.id ORDER BY t.name COLLATE NOCASE)), '[]'), p.target_stock_quantity
     FROM products p LEFT JOIN categories c ON c.id = p.category_id
     LEFT JOIN product_brands b ON b.id=p.brand_id";

fn get_product(
    connection: &Connection,
    store_id: &str,
    product_id: &str,
) -> Result<Product, String> {
    connection
        .query_row(
            &format!("{PRODUCT_SELECT} WHERE p.store_id = ?1 AND p.id = ?2"),
            params![store_id, product_id],
            product_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That product is no longer in this store's catalog.".to_owned())
}

#[tauri::command]
pub fn get_bootstrap_status(state: State<'_, AppState>) -> Result<BootstrapStatus, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let setup = connection
        .query_row(
            "SELECT b.name, s.name, b.currency_code
             FROM businesses b JOIN stores s ON s.business_id = b.id
             ORDER BY b.created_at LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;

    match setup {
        Some((business_name, store_name, currency_code)) => Ok(BootstrapStatus {
            needs_setup: false,
            business_name: Some(business_name),
            store_name: Some(store_name),
            currency_code: Some(currency_code),
        }),
        None => Ok(BootstrapStatus {
            needs_setup: true,
            business_name: None,
            store_name: None,
            currency_code: None,
        }),
    }
}

#[tauri::command]
pub fn setup_store(
    input: InitialSetupInput,
    state: State<'_, AppState>,
) -> Result<SessionUser, String> {
    let business_name = normalized_text(&input.business_name);
    let store_name = normalized_text(&input.store_name);
    let full_name = normalized_text(&input.full_name);
    let username = normalized_text(&input.username);
    let currency_code = input.currency_code.trim().to_ascii_uppercase();
    if business_name.is_empty() || store_name.is_empty() || full_name.is_empty() {
        return Err("Enter a business name, store name, and administrator name.".to_owned());
    }
    if username.chars().count() < 3 || username.chars().count() > 50 {
        return Err("Choose a username between 3 and 50 characters.".to_owned());
    }
    if input.password.chars().count() < 12 {
        return Err("Use a password with at least 12 characters.".to_owned());
    }
    if currency_code.len() != 3
        || !currency_code
            .chars()
            .all(|value| value.is_ascii_alphabetic())
    {
        return Err("Enter a three-letter ISO currency code, such as GHS.".to_owned());
    }

    let password_hash = hash_password(&input.password)?;
    let business_id = Uuid::new_v4().to_string();
    let store_id = Uuid::new_v4().to_string();
    let owner_role_id = Uuid::new_v4().to_string();
    let admin_role_id = Uuid::new_v4().to_string();
    let manager_role_id = Uuid::new_v4().to_string();
    let cashier_role_id = Uuid::new_v4().to_string();
    let user_id = Uuid::new_v4().to_string();
    let timestamp = now();

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let already_initialized: bool = transaction
        .query_row("SELECT EXISTS(SELECT 1 FROM businesses)", [], |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())?;
    if already_initialized {
        return Err("This local database is already set up. Sign in instead.".to_owned());
    }

    transaction
        .execute(
            "INSERT INTO businesses (id, name, currency_code, timezone, created_at) VALUES (?1, ?2, ?3, 'Africa/Accra', ?4)",
            params![business_id, business_name, currency_code, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO stores (id, business_id, name, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![store_id, business_id, store_name, timestamp],
        )
        .map_err(|error| error.to_string())?;

    let role_definitions = [
        (
            &owner_role_id,
            "owner",
            "Owner",
            "management",
            vec![
                "pos:use",
                "products:read",
                "products:write",
                "inventory:write",
                "purchases:read",
                "purchases:write",
                "sales:read",
                "sales:refund",
                "customers:read",
                "customers:write",
                "suppliers:read",
                "suppliers:write",
                "expenses:read",
                "expenses:write",
                "expenses:reclassify",
                "reports:read",
                "accounting:close",
                "settings:write",
                "users:write",
                "license:manage",
                "backups:manage",
                "roles:manage",
            ],
        ),
        (
            &admin_role_id,
            "admin",
            "Administrator",
            "management",
            vec![
                "pos:use",
                "products:read",
                "products:write",
                "inventory:write",
                "purchases:read",
                "purchases:write",
                "sales:read",
                "sales:refund",
                "customers:read",
                "customers:write",
                "suppliers:read",
                "suppliers:write",
                "expenses:read",
                "expenses:write",
                "expenses:reclassify",
                "reports:read",
                "accounting:close",
                "settings:write",
                "users:write",
            ],
        ),
        (
            &manager_role_id,
            "manager",
            "Manager",
            "management",
            vec![
                "pos:use",
                "products:read",
                "products:write",
                "inventory:write",
                "purchases:read",
                "purchases:write",
                "sales:read",
                "customers:read",
                "customers:write",
                "suppliers:read",
                "suppliers:write",
                "expenses:read",
                "expenses:write",
                "reports:read",
            ],
        ),
        (
            &cashier_role_id,
            "cashier",
            "Cashier",
            "cashier",
            vec![
                "pos:use",
                "products:read",
                "customers:read",
                "customers:write",
            ],
        ),
    ];
    for (role_id, role_key, display_name, workspace_mode, permissions) in role_definitions {
        let permissions_json =
            serde_json::to_string(&permissions).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO roles (id, business_id, role_key, display_name, permissions_json, is_system, workspace_mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![role_id, business_id, role_key, display_name, permissions_json, workspace_mode],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "INSERT INTO users (id, business_id, role_id, username, password_hash, full_name, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![user_id, business_id, owner_role_id, username, password_hash, full_name, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE businesses SET owner_user_id=?1 WHERE id=?2",
            params![user_id, business_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO audit_events (id, business_id, store_id, user_id, action, entity_type, entity_id, details_json, created_at)
             VALUES (?1, ?2, ?3, ?4, 'store_initialized', 'business', ?2, '{}', ?5)",
            params![Uuid::new_v4().to_string(), business_id, store_id, user_id, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;

    let session = SessionUser {
        user_id,
        business_id,
        store_id,
        full_name,
        username,
        role_key: "owner".to_owned(),
        role_name: "Owner".to_owned(),
        workspace_mode: "management".to_owned(),
        is_owner: true,
        permissions: role_definitions_permissions("owner"),
        business_name,
        store_name,
        currency_code,
    };
    drop(connection);
    let _gate = state
        .session_gate
        .write()
        .map_err(|_| "The local session is unavailable.".to_owned())?;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = Some(session.clone());
    Ok(session)
}

pub(crate) fn role_definitions_permissions(role: &str) -> Vec<String> {
    let permissions: &[&str] = match role {
        "owner" => &[
            "pos:use",
            "products:read",
            "products:write",
            "inventory:write",
            "purchases:read",
            "purchases:write",
            "purchases:invoice",
            "sales:read",
            "sales:refund",
            "sales:discount:approve",
            "customers:read",
            "customers:write",
            "suppliers:read",
            "suppliers:write",
            "suppliers:settle",
            "expenses:read",
            "expenses:write",
            "expenses:reclassify",
            "reports:read",
            "accounting:close",
            "settings:write",
            "users:write",
            "license:manage",
            "backups:manage",
            "roles:manage",
        ],
        "admin" => &[
            "pos:use",
            "products:read",
            "products:write",
            "inventory:write",
            "purchases:read",
            "purchases:write",
            "purchases:invoice",
            "sales:read",
            "sales:refund",
            "sales:discount:approve",
            "customers:read",
            "customers:write",
            "suppliers:read",
            "suppliers:write",
            "suppliers:settle",
            "expenses:read",
            "expenses:write",
            "expenses:reclassify",
            "reports:read",
            "accounting:close",
            "settings:write",
            "users:write",
        ],
        "manager" => &[
            "pos:use",
            "products:read",
            "products:write",
            "inventory:write",
            "purchases:read",
            "purchases:write",
            "sales:read",
            "sales:refund",
            "sales:discount:approve",
            "customers:read",
            "customers:write",
            "suppliers:read",
            "suppliers:write",
            "expenses:read",
            "expenses:write",
            "reports:read",
        ],
        _ => &[
            "pos:use",
            "products:read",
            "customers:read",
            "customers:write",
        ],
    };
    permissions
        .iter()
        .map(|value| (*value).to_owned())
        .collect()
}

#[tauri::command]
pub fn login(input: LoginInput, state: State<'_, AppState>) -> Result<SessionUser, String> {
    let username = normalized_text(&input.username);
    if username.is_empty() || input.password.is_empty() {
        return Err("Enter your username and password.".to_owned());
    }

    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let row = connection
        .query_row(
            "SELECT u.password_hash, u.is_active, u.id, u.business_id, s.id, u.full_name, u.username,
                    r.role_key, r.display_name, r.permissions_json, b.name, s.name, b.currency_code,r.workspace_mode,COALESCE(b.owner_user_id=u.id,0)
             FROM users u
             JOIN roles r ON r.id = u.role_id
             JOIN businesses b ON b.id = u.business_id
             JOIN stores s ON s.business_id = b.id AND s.is_active = 1
             WHERE u.username = ?1 AND u.is_active = 1
             ORDER BY s.created_at LIMIT 1",
            [&username],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    SessionUser {
                        user_id: row.get(2)?,
                        business_id: row.get(3)?,
                        store_id: row.get(4)?,
                        full_name: row.get(5)?,
                        username: row.get(6)?,
                        role_key: row.get(7)?,
                        role_name: row.get(8)?,
                        workspace_mode: row.get(13)?,
                        is_owner: row.get::<_, i64>(14)? != 0,
                        permissions: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
                        business_name: row.get(10)?,
                        store_name: row.get(11)?,
                        currency_code: row.get(12)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let Some((password_hash, is_active, session)) = row else {
        return Err("Username or password was not recognized.".to_owned());
    };
    if is_active == 0 {
        return Err("Username or password was not recognized.".to_owned());
    }
    let _ = password_hash;
    crate::passkeys::verify_account_password(&connection, &session.user_id, &input.password)?;
    crate::attendance::start_for_login(&connection, &session, Utc::now())?;
    drop(connection);
    let _gate = state.session_gate.write().map_err(|_| {
        "The local session is unavailable. Restart NimbleTill and sign in again.".to_owned()
    })?;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = Some(session.clone());
    Ok(session)
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let _gate = state.session_gate.write().map_err(|_| {
        "The local session is unavailable. Restart NimbleTill and sign in again.".to_owned()
    })?;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = None;
    Ok(())
}

fn empty_register_ticket(session: &SessionUser) -> RegisterTicket {
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    RegisterTicket {
        payment_option_id: None,
        payment_reference: String::new(),
        ticket_number: format!("T-{}", &id[..6].to_ascii_uppercase()),
        id,
        creator_user_id: session.user_id.clone(),
        creator_name: session.full_name.clone(),
        last_editor_user_id: session.user_id.clone(),
        last_editor_name: session.full_name.clone(),
        customer_id: None,
        lines: Vec::new(),
        discount_minor: 0,
        discount_reason: None,
        discount_requested_by_user_id: None,
        discount_approval_id: None,
        discount_approver_name: None,
        discount_approval_expires_at: None,
        payment_method: PaymentMethod::Cash,
        amount_tendered_minor: 0,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    }
}

pub(crate) fn load_register_workspace(
    connection: &Connection,
    session: &SessionUser,
) -> Result<RegisterWorkspaceView, String> {
    let stored = connection
        .query_row(
            "SELECT revision, workspace_json FROM register_workspaces WHERE store_id = ?1",
            [&session.store_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((revision, workspace_json)) = stored {
        let workspace: RegisterWorkspaceView = serde_json::from_str(&workspace_json)
            .map_err(|_| "The saved register workspace could not be read. It has been preserved for recovery.".to_owned())?;
        if workspace.schema_version != 1 {
            return Err("This register workspace uses an unsupported format. It has been preserved for recovery.".to_owned());
        }
        if workspace.revision != revision {
            return Err("The saved register workspace has an inconsistent revision. It has been preserved for recovery.".to_owned());
        }
        return Ok(workspace);
    }

    let workspace = RegisterWorkspaceView {
        schema_version: 1,
        revision: 0,
        active: empty_register_ticket(session),
        held: Vec::new(),
        last_completed: None,
    };
    let json = serde_json::to_string(&workspace).map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO register_workspaces (store_id, revision, workspace_json, updated_at) VALUES (?1, 0, ?2, ?3)",
            params![session.store_id, json, now()],
        )
        .map_err(|error| error.to_string())?;
    Ok(workspace)
}

pub(crate) fn persist_register_workspace(
    connection: &Connection,
    session: &SessionUser,
    expected_revision: i64,
    workspace: &mut RegisterWorkspaceView,
) -> Result<(), String> {
    workspace.revision = expected_revision + 1;
    let json = serde_json::to_string(workspace).map_err(|error| error.to_string())?;
    let changed = connection
        .execute(
            "UPDATE register_workspaces SET revision = ?1, workspace_json = ?2, updated_at = ?3
             WHERE store_id = ?4 AND revision = ?5",
            params![
                workspace.revision,
                json,
                now(),
                session.store_id,
                expected_revision
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err(
            "The register changed in another window. Reload the ticket and try again.".to_owned(),
        );
    }
    Ok(())
}

fn ticket_has_content(ticket: &RegisterTicket) -> bool {
    !ticket.lines.is_empty()
        || ticket.customer_id.is_some()
        || ticket.discount_minor > 0
        || ticket.amount_tendered_minor > 0
}

fn update_ticket_editor(ticket: &mut RegisterTicket, session: &SessionUser) {
    ticket.last_editor_user_id = session.user_id.clone();
    ticket.last_editor_name = session.full_name.clone();
    ticket.updated_at = now();
}

fn payment_method_from_str(value: &str) -> PaymentMethod {
    match value {
        "card" => PaymentMethod::Card,
        "mobile_money" => PaymentMethod::MobileMoney,
        "other" => PaymentMethod::Other,
        _ => PaymentMethod::Cash,
    }
}

fn sale_receipt_by_ticket(
    connection: &Connection,
    store_id: &str,
    ticket_id: &str,
) -> Result<Option<SaleReceipt>, String> {
    let sale = connection
        .query_row(
            "SELECT s.id, s.receipt_number, s.created_at, u.full_name, s.subtotal_minor,
                    s.discount_minor, s.tax_minor, s.total_minor, s.amount_tendered_minor,
                    s.change_due_minor, p.method
             FROM sales s
             JOIN users u ON u.id = s.cashier_id
             JOIN payments p ON p.sale_id = s.id
             WHERE s.store_id = ?1 AND s.source_ticket_id = ?2
             ORDER BY p.created_at LIMIT 1",
            params![store_id, ticket_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((
        id,
        receipt_number,
        created_at,
        cashier_name,
        subtotal,
        discount,
        tax,
        total,
        tendered,
        change,
        method,
    )) = sale
    else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare(
            "SELECT product_id, sku_snapshot, name_snapshot, quantity, unit_price_minor, line_total_minor
             FROM sale_items WHERE sale_id = ?1 ORDER BY rowid",
        )
        .map_err(|error| error.to_string())?;
    let items = statement
        .query_map([&id], |row| {
            Ok(SaleReceiptItem {
                product_id: row.get(0)?,
                sku: row.get(1)?,
                name: row.get(2)?,
                quantity: row.get(3)?,
                unit_price_minor: row.get(4)?,
                line_total_minor: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(Some(SaleReceipt {
        payment_detail: crate::payment_options::receipt_detail(connection, &id)?,
        id,
        receipt_number,
        created_at,
        cashier_name,
        items,
        totals: pos_core::SaleTotals {
            subtotal_minor: subtotal,
            discount_minor: discount,
            tax_minor: tax,
            total_minor: total,
        },
        payment_method: payment_method_from_str(&method),
        amount_tendered_minor: tendered,
        change_due_minor: change,
    }))
}

#[tauri::command]
pub fn get_register_workspace(state: State<'_, AppState>) -> Result<RegisterWorkspaceView, String> {
    let session = active_session(&state, "pos:use")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    load_register_workspace(&connection, &session)
}

#[tauri::command]
pub fn edit_register_workspace(
    input: RegisterEditInput,
    state: State<'_, AppState>,
) -> Result<RegisterWorkspaceView, String> {
    let session = active_session(&state, "pos:use")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut workspace = load_register_workspace(&transaction, &session)?;
    if workspace.revision != input.expected_revision {
        return Err(
            "The register changed in another window. Reload the ticket and try again.".to_owned(),
        );
    }

    let invalidated_ticket_ids: Vec<String> = match &input.action {
        RegisterAction::AddProduct { .. }
        | RegisterAction::SetQuantity { .. }
        | RegisterAction::RemoveLines { .. }
        | RegisterAction::ClearActive
        | RegisterAction::SetDiscount { .. }
        | RegisterAction::SelectCustomer { .. }
        | RegisterAction::AcceptCurrentPrices => vec![workspace.active.id.clone()],
        RegisterAction::DiscardHeld { ticket_id } => vec![ticket_id.clone()],
        _ => Vec::new(),
    };

    match input.action {
        RegisterAction::AddProduct { product_id } => {
            let product = get_product(&transaction, &session.store_id, &product_id)?;
            if !product.is_active || product.stock_quantity < 1 {
                return Err(format!(
                    "{} is not currently available for sale.",
                    product.name
                ));
            }
            if let Some(line) = workspace
                .active
                .lines
                .iter_mut()
                .find(|line| line.product_id == product.id)
            {
                if line.quantity >= product.stock_quantity {
                    return Err(format!(
                        "Only {} {} remain in stock.",
                        product.stock_quantity, product.name
                    ));
                }
                line.quantity += 1;
            } else {
                if workspace.active.lines.len() >= 500 {
                    return Err(
                        "A ticket cannot contain more than 500 different products.".to_owned()
                    );
                }
                workspace.active.lines.push(RegisterTicketLine {
                    product_id: product.id,
                    quantity: 1,
                    accepted_name: product.name,
                    accepted_sku: product.sku,
                    accepted_unit_price_minor: product.price_minor,
                });
            }
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::SetQuantity {
            product_id,
            quantity,
        } => {
            if quantity <= 0 {
                return Err("Item quantities must be positive whole numbers.".to_owned());
            }
            let product = get_product(&transaction, &session.store_id, &product_id)?;
            if quantity > product.stock_quantity {
                return Err(format!(
                    "Only {} {} remain in stock.",
                    product.stock_quantity, product.name
                ));
            }
            let line = workspace
                .active
                .lines
                .iter_mut()
                .find(|line| line.product_id == product_id)
                .ok_or_else(|| "That item is no longer on the active ticket.".to_owned())?;
            line.quantity = quantity;
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::RemoveLines { product_ids } => {
            let ids = product_ids.into_iter().collect::<HashSet<_>>();
            workspace
                .active
                .lines
                .retain(|line| !ids.contains(&line.product_id));
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::ClearActive => {
            workspace.active = empty_register_ticket(&session);
        }
        RegisterAction::SetDiscount {
            discount_minor,
            reason,
        } => {
            if discount_minor < 0 {
                return Err("Discount must be zero or greater.".to_owned());
            }
            let subtotal = workspace
                .active
                .lines
                .iter()
                .try_fold(0_i64, |total, line| {
                    let value =
                        i128::from(line.accepted_unit_price_minor) * i128::from(line.quantity);
                    i64::try_from(i128::from(total) + value)
                        .map_err(|_| "The ticket total is too large.".to_owned())
                })?;
            if discount_minor > subtotal {
                return Err("Discount cannot be greater than the ticket subtotal.".to_owned());
            }
            let reason = reason.as_deref().map(str::trim).unwrap_or("");
            if discount_minor > 0 && !(3..=200).contains(&reason.chars().count()) {
                return Err(
                    "Add a short reason between 3 and 200 characters for this discount.".to_owned(),
                );
            }
            workspace.active.discount_minor = discount_minor;
            workspace.active.discount_reason = (discount_minor > 0).then(|| reason.to_owned());
            workspace.active.discount_requested_by_user_id =
                (discount_minor > 0).then(|| session.user_id.clone());
            workspace.active.discount_approval_id = None;
            workspace.active.discount_approver_name = None;
            workspace.active.discount_approval_expires_at = None;
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::SetPayment {
            payment_option_id,
            payment_reference,
            payment_method,
            amount_tendered_minor,
        } => {
            if amount_tendered_minor < 0 {
                return Err("Amount received must be zero or greater.".to_owned());
            }
            workspace.active.payment_method = payment_method;
            if payment_reference.chars().count() > 120 {
                return Err("Payment reference is too long.".to_owned());
            }
            workspace.active.payment_option_id = payment_option_id;
            workspace.active.payment_reference = payment_reference;
            workspace.active.amount_tendered_minor = amount_tendered_minor;
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::SelectCustomer { customer_id } => {
            if let Some(id) = customer_id.as_deref() {
                let exists: bool = transaction
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM customers WHERE id = ?1 AND store_id = ?2)",
                        params![id, session.store_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                if !exists {
                    return Err("The selected customer is not available in this store.".to_owned());
                }
            }
            workspace.active.customer_id = customer_id;
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::SetPaymentReference { reference } => {
            if reference.chars().count() > 120 {
                return Err("Payment reference is too long.".to_owned());
            }
            workspace.active.payment_reference = reference;
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::HoldActive => {
            if !ticket_has_content(&workspace.active) {
                return Err(
                    "Add products or customer details before holding this ticket.".to_owned(),
                );
            }
            if workspace.held.len() >= 100 {
                return Err("There are already 100 held tickets. Resume or remove one before holding another.".to_owned());
            }
            update_ticket_editor(&mut workspace.active, &session);
            workspace.held.push(workspace.active);
            workspace.active = empty_register_ticket(&session);
        }
        RegisterAction::ResumeHeld {
            ticket_id,
            hold_active,
        } => {
            if ticket_has_content(&workspace.active) && !hold_active {
                return Err(
                    "Hold or clear the current ticket before resuming another one.".to_owned(),
                );
            }
            let held_index = workspace
                .held
                .iter()
                .position(|ticket| ticket.id == ticket_id)
                .ok_or_else(|| "That held ticket is no longer available.".to_owned())?;
            if ticket_has_content(&workspace.active) {
                if workspace.held.len() >= 100 {
                    return Err("There is no room to hold the active ticket. Resume or remove a held ticket first.".to_owned());
                }
                update_ticket_editor(&mut workspace.active, &session);
                let previous =
                    std::mem::replace(&mut workspace.active, workspace.held.remove(held_index));
                workspace.held.push(previous);
            } else {
                workspace.active = workspace.held.remove(held_index);
            }
            update_ticket_editor(&mut workspace.active, &session);
        }
        RegisterAction::DiscardHeld { ticket_id } => {
            let held_index = workspace
                .held
                .iter()
                .position(|ticket| ticket.id == ticket_id)
                .ok_or_else(|| "That held ticket is no longer available.".to_owned())?;
            workspace.held.remove(held_index);
        }
        RegisterAction::AcceptCurrentPrices => {
            for line in &mut workspace.active.lines {
                let product = get_product(&transaction, &session.store_id, &line.product_id)?;
                if !product.is_active {
                    return Err(format!(
                        "{} is no longer active. Remove it before checkout.",
                        line.accepted_name
                    ));
                }
                line.accepted_name = product.name;
                line.accepted_sku = product.sku;
                line.accepted_unit_price_minor = product.price_minor;
            }
            update_ticket_editor(&mut workspace.active, &session);
        }
    }

    for ticket_id in &invalidated_ticket_ids {
        crate::discounts::invalidate_approval_for_ticket(
            &transaction,
            &session,
            ticket_id,
            "ticket_financial_details_changed",
        )?;
        if workspace.active.id == *ticket_id {
            workspace.active.discount_approval_id = None;
            workspace.active.discount_approver_name = None;
            workspace.active.discount_approval_expires_at = None;
        }
        for held in &mut workspace.held {
            if held.id == *ticket_id {
                held.discount_approval_id = None;
                held.discount_approver_name = None;
                held.discount_approval_expires_at = None;
            }
        }
    }

    persist_register_workspace(
        &transaction,
        &session,
        input.expected_revision,
        &mut workspace,
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(workspace)
}

#[tauri::command]
pub fn get_products(state: State<'_, AppState>) -> Result<Vec<Product>, String> {
    let session = active_session(&state, "products:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = connection
        .prepare(&format!("{PRODUCT_SELECT} WHERE p.store_id = ?1 AND p.is_active = 1 ORDER BY p.name COLLATE NOCASE"))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session.store_id.as_str()], product_from_row)
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_product(input: ProductInput, state: State<'_, AppState>) -> Result<Product, String> {
    let session = active_session(&state, "products:write")?;
    let name = normalized_text(&input.name);
    let requested_sku = normalized_text(&input.sku);
    let barcode = input
        .barcode
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let description = input
        .description
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let brand_id = input
        .brand_id
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let tag_ids: Vec<String> = input
        .tag_ids
        .iter()
        .map(|tag_id| normalized_text(tag_id))
        .filter(|tag_id| !tag_id.is_empty())
        .collect();
    let distinct_tag_ids: HashSet<&str> = tag_ids.iter().map(String::as_str).collect();
    if tag_ids.len() > 100 || distinct_tag_ids.len() != tag_ids.len() {
        return Err("Choose up to 100 different product tags.".to_owned());
    }
    if name.is_empty() {
        return Err("Enter a product name.".to_owned());
    }
    if input.price_minor < 0 || input.cost_minor.is_some_and(|value| value < 0) {
        return Err("Prices and costs must be zero or greater.".to_owned());
    }
    if input.stock_quantity < 0
        || input.reorder_point < 0
        || !(0..=1_000_000).contains(&input.target_stock_quantity)
    {
        return Err("Stock values must be zero or within the supported maximum.".to_owned());
    }
    if input.target_stock_quantity > 0 && input.target_stock_quantity <= input.reorder_point {
        return Err("Set the restock target above the reorder point, or leave it at zero to disable reorder suggestions.".to_owned());
    }

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let timestamp = now();
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let existing = transaction
        .query_row(
            "SELECT stock_quantity FROM products WHERE id = ?1 AND store_id = ?2",
            params![id, session.store_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let sku = if requested_sku.is_empty() {
        if existing.is_some() {
            return Err("Enter a SKU or generate one before saving this product.".to_owned());
        }
        let mut generated = None;
        for _ in 0..8 {
            let candidate = format!(
                "SKU-{}",
                Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase()
            );
            let already_used: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM products WHERE store_id=?1 AND sku=?2)",
                    params![session.store_id, candidate],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !already_used {
                generated = Some(candidate);
                break;
            }
        }
        generated
            .ok_or_else(|| "A unique SKU could not be generated. Please try again.".to_owned())?
    } else {
        requested_sku
    };

    let current_brand_id: Option<String> = if existing.is_some() {
        transaction
            .query_row(
                "SELECT brand_id FROM products WHERE id=?1 AND store_id=?2",
                params![id, session.store_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten()
    } else {
        None
    };
    let mut current_tag_ids = HashSet::new();
    if existing.is_some() {
        let mut statement = transaction
            .prepare("SELECT tag_id FROM product_tag_links WHERE product_id=?1")
            .map_err(|error| error.to_string())?;
        current_tag_ids = statement
            .query_map([&id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|error| error.to_string())?;
    }

    if let Some(brand_id) = brand_id.as_deref() {
        let is_active: Option<i64> = transaction
            .query_row(
                "SELECT is_active FROM product_brands WHERE id=?1 AND business_id=?2",
                params![brand_id, session.business_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        match is_active {
            None => return Err("Choose a brand belonging to this business.".to_owned()),
            Some(0) if current_brand_id.as_deref() != Some(brand_id) => {
                return Err("That brand is archived. Choose an active brand.".to_owned());
            }
            Some(_) => {}
        }
    }
    for tag_id in &tag_ids {
        let is_active: Option<i64> = transaction
            .query_row(
                "SELECT is_active FROM product_tag_catalog WHERE id=?1 AND business_id=?2",
                params![tag_id, session.business_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        match is_active {
            None => return Err("Every selected tag must belong to this business.".to_owned()),
            Some(0) if !current_tag_ids.contains(tag_id) => {
                return Err("An archived tag cannot be assigned to a product.".to_owned());
            }
            Some(_) => {}
        }
    }

    if let Some(current_stock) = existing {
        transaction
            .execute(
                "UPDATE products SET category_id = ?1, supplier_id = ?2, brand_id = ?3, sku = ?4, barcode = ?5, name = ?6,
                 description = ?7, price_minor = ?8, cost_minor = ?9, reorder_point = ?10, target_stock_quantity = ?11,
                 is_active = ?12, updated_at = ?13 WHERE id = ?14 AND store_id = ?15",
                params![
                    input.category_id, input.supplier_id, brand_id, sku, barcode, name, description,
                    input.price_minor, input.cost_minor, input.reorder_point, input.target_stock_quantity,
                    input.is_active as i64, timestamp, id, session.store_id
                ],
            )
            .map_err(|error| {
                if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                    "That SKU is already used in this store.".to_owned()
                } else {
                    error.to_string()
                }
            })?;
        add_audit_event(
            &transaction,
            &session,
            "product_updated",
            "product",
            Some(&id),
            serde_json::json!({ "sku": sku, "name": name, "stockQuantity": current_stock, "brandId": brand_id, "tagIds": tag_ids }),
        )?;
    } else {
        transaction
            .execute(
                "INSERT INTO products (id, store_id, category_id, supplier_id, brand_id, sku, barcode, name, description,
                 price_minor, cost_minor, stock_quantity, reorder_point, target_stock_quantity, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
                params![
                    id, session.store_id, input.category_id, input.supplier_id, brand_id, sku, barcode, name,
                    description, input.price_minor, input.cost_minor, input.stock_quantity,
                    input.reorder_point, input.target_stock_quantity, input.is_active as i64, timestamp
                ],
            )
            .map_err(|error| {
                if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                    "That SKU is already used in this store.".to_owned()
                } else {
                    error.to_string()
                }
            })?;
        if input.stock_quantity > 0 {
            transaction
                .execute(
                    "INSERT INTO stock_movements (id, store_id, product_id, user_id, quantity_delta, reason, note, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'receiving', 'Opening stock', ?6)",
                    params![Uuid::new_v4().to_string(), session.store_id, id, session.user_id, input.stock_quantity, timestamp],
                )
                .map_err(|error| error.to_string())?;
        }
        add_audit_event(
            &transaction,
            &session,
            "product_created",
            "product",
            Some(&id),
            serde_json::json!({ "sku": sku, "name": name, "brandId": brand_id, "tagIds": tag_ids }),
        )?;
    }

    transaction
        .execute("DELETE FROM product_tag_links WHERE product_id=?1", [&id])
        .map_err(|error| error.to_string())?;
    for tag_id in &tag_ids {
        transaction
            .execute(
                "INSERT INTO product_tag_links(product_id,tag_id) VALUES(?1,?2)",
                params![id, tag_id],
            )
            .map_err(|error| error.to_string())?;
    }

    transaction.commit().map_err(|error| error.to_string())?;
    get_product(&connection, &session.store_id, &id)
}

#[tauri::command]
pub fn get_categories(state: State<'_, AppState>) -> Result<Vec<Category>, String> {
    let session = active_session(&state, "products:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = connection
        .prepare("SELECT id, name, description FROM categories WHERE store_id = ?1 AND is_active = 1 ORDER BY name COLLATE NOCASE")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session.store_id.as_str()], |row| {
            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_category(input: CategoryInput, state: State<'_, AppState>) -> Result<Category, String> {
    let session = active_session(&state, "products:write")?;
    let name = normalized_text(&input.name);
    if name.is_empty() {
        return Err("Enter a category name.".to_owned());
    }
    let description = input
        .description
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    connection
        .execute(
            "INSERT INTO categories (id, store_id, name, description, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description
             WHERE categories.store_id = excluded.store_id",
            params![id, session.store_id, name, description, now()],
        )
        .map_err(|error| {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                "That category already exists in this store.".to_owned()
            } else {
                error.to_string()
            }
        })?;
    connection
        .query_row(
            "SELECT id, name, description FROM categories WHERE id = ?1 AND store_id = ?2",
            params![id, session.store_id],
            |row| {
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_customers(state: State<'_, AppState>) -> Result<Vec<Customer>, String> {
    let session = active_session(&state, "customers:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = connection
        .prepare("SELECT id, name, email, phone, address, notes FROM customers WHERE store_id = ?1 ORDER BY name COLLATE NOCASE")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session.store_id.as_str()], |row| {
            Ok(Customer {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                phone: row.get(3)?,
                address: row.get(4)?,
                notes: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_customer(input: CustomerInput, state: State<'_, AppState>) -> Result<Customer, String> {
    let session = active_session(&state, "customers:write")?;
    let name = normalized_text(&input.name);
    if name.is_empty() {
        return Err("Enter a customer name.".to_owned());
    }
    let normalize_optional = |value: Option<String>| {
        value
            .map(|text| normalized_text(&text))
            .filter(|text| !text.is_empty())
    };
    let email = normalize_optional(input.email);
    let phone = normalize_optional(input.phone);
    let address = normalize_optional(input.address);
    let notes = normalize_optional(input.notes);
    let is_new = input.id.is_none();
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO customers (id, store_id, name, email, phone, address, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, email = excluded.email,
             phone = excluded.phone, address = excluded.address, notes = excluded.notes
             WHERE customers.store_id = excluded.store_id",
            params![
                id,
                session.store_id,
                name,
                email,
                phone,
                address,
                notes,
                now()
            ],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &session,
        if is_new {
            "customer_created"
        } else {
            "customer_updated"
        },
        "customer",
        Some(&id),
        serde_json::json!({ "name": name }),
    )?;
    let customer = transaction
        .query_row(
            "SELECT id, name, email, phone, address, notes FROM customers WHERE id = ?1 AND store_id = ?2",
            params![id, session.store_id],
            |row| {
                Ok(Customer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    email: row.get(2)?,
                    phone: row.get(3)?,
                    address: row.get(4)?,
                    notes: row.get(5)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(customer)
}

#[tauri::command]
pub fn attach_sale_customer(
    input: AttachSaleCustomerInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session = active_session(&state, "customers:write")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    attach_sale_customer_with_session(input, &session, &mut connection)
}

fn attach_sale_customer_with_session(
    input: AttachSaleCustomerInput,
    session: &SessionUser,
    connection: &mut Connection,
) -> Result<(), String> {
    let can_read_sales = session
        .permissions
        .iter()
        .any(|permission| permission == "sales:read");
    if !can_read_sales
        && !session
            .permissions
            .iter()
            .any(|permission| permission == "pos:use")
    {
        return Err("Your account cannot link a customer to a sale.".to_owned());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let customer_exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM customers WHERE id = ?1 AND store_id = ?2)",
            params![input.customer_id, session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !customer_exists {
        return Err("That customer is not available in this store.".to_owned());
    }

    let sale = transaction
        .query_row(
            "SELECT customer_id, cashier_id, status FROM sales WHERE id = ?1 AND store_id = ?2",
            params![input.sale_id, session.store_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((old_customer_id, cashier_id, status)) = sale else {
        return Err("That sale is not available in this store.".to_owned());
    };
    if !can_read_sales && cashier_id != session.user_id {
        return Err("Cashiers can link a customer only to a sale they completed.".to_owned());
    }
    if status != "completed" && status != "partially_refunded" {
        return Err("A customer can only be linked to a completed sale.".to_owned());
    }
    if old_customer_id.as_deref() == Some(input.customer_id.as_str()) {
        return Ok(());
    }
    if old_customer_id.is_some() && !can_read_sales {
        return Err("Ask a manager to change the customer already linked to this sale.".to_owned());
    }
    transaction
        .execute(
            "UPDATE sales SET customer_id = ?1 WHERE id = ?2 AND store_id = ?3",
            params![input.customer_id, input.sale_id, session.store_id],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &session,
        "sale_customer_linked",
        "sale",
        Some(&input.sale_id),
        serde_json::json!({ "previousCustomerId": old_customer_id, "customerId": input.customer_id }),
    )?;
    transaction.commit().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_staff_profiles(state: State<'_, AppState>) -> Result<Vec<StaffProfile>, String> {
    let session = active_session(&state, "pos:use")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = connection.prepare(
        "SELECT u.id, u.full_name, r.role_key, r.display_name, u.pin_hash IS NOT NULL
         FROM users u JOIN roles r ON r.id = u.role_id
         WHERE u.business_id = ?1 AND u.is_active = 1
           AND EXISTS (SELECT 1 FROM stores s WHERE s.id = ?2 AND s.business_id = u.business_id AND s.is_active = 1)
         ORDER BY CASE r.role_key WHEN 'cashier' THEN 0 WHEN 'manager' THEN 1 ELSE 2 END, u.full_name COLLATE NOCASE",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![session.business_id, session.store_id], |row| {
            Ok(StaffProfile {
                user_id: row.get(0)?,
                full_name: row.get(1)?,
                role_key: row.get(2)?,
                role_name: row.get(3)?,
                has_pin: row.get::<_, i64>(4)? != 0,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_staff_member(
    input: CreateStaffMemberInput,
    state: State<'_, AppState>,
) -> Result<StaffProfile, String> {
    let session = active_session(&state, "users:write")?;
    let full_name = normalized_text(&input.full_name);
    let username = normalized_text(&input.username);
    if full_name.is_empty() || full_name.chars().count() > 100 {
        return Err("Enter a staff name under 100 characters.".to_owned());
    }
    if username.chars().count() < 3 || username.chars().count() > 50 {
        return Err("Choose a username between 3 and 50 characters.".to_owned());
    }
    if input.password.chars().count() < 12 {
        return Err("Use a password with at least 12 characters.".to_owned());
    }
    if !(4..=10).contains(&input.pin.chars().count())
        || !input.pin.chars().all(|digit| digit.is_ascii_digit())
    {
        return Err("Choose a PIN with 4 to 10 digits.".to_owned());
    }
    if input.role_key == "owner" {
        return Err(
            "Owner access is assigned through the protected business ownership flow.".to_owned(),
        );
    }

    let password_hash = hash_password(&input.password)?;
    let pin_hash = hash_password(&input.pin)?;
    let user_id = Uuid::new_v4().to_string();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let role = transaction
        .query_row(
            "SELECT id, display_name, permissions_json FROM roles WHERE business_id = ?1 AND role_key = ?2",
            params![session.business_id, input.role_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That staff role is not configured for this business.".to_owned())?;
    let role_permissions: Vec<String> = serde_json::from_str(&role.2).unwrap_or_default();
    if role_permissions.iter().any(|permission| {
        !session
            .permissions
            .iter()
            .any(|allowed| allowed == permission)
    }) {
        return Err("You cannot assign a role with permissions you do not have.".to_owned());
    }
    transaction.execute(
        "INSERT INTO users (id, business_id, role_id, username, password_hash, full_name, pin_hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![user_id, session.business_id, role.0, username, password_hash, full_name, pin_hash, now()],
    ).map_err(|error| {
        if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
            "That username is already used in this business.".to_owned()
        } else {
            error.to_string()
        }
    })?;
    add_audit_event(
        &transaction,
        &session,
        "staff_member_created",
        "user",
        Some(&user_id),
        serde_json::json!({ "username": username, "role": input.role_key }),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(StaffProfile {
        user_id,
        full_name,
        role_key: input.role_key,
        role_name: role.1,
        has_pin: true,
    })
}

#[tauri::command]
pub fn switch_staff_profile(
    input: SwitchStaffProfileInput,
    state: State<'_, AppState>,
) -> Result<SessionUser, String> {
    let incoming = active_session(&state, "pos:use")?.user.clone();
    let _gate = state.session_gate.write().map_err(|_| {
        "The local session is unavailable. Restart NimbleTill and sign in again.".to_owned()
    })?;
    let current = state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())?
        .clone()
        .ok_or_else(|| "Sign in to continue.".to_owned())?;
    if current.user_id != incoming.user_id
        || !current
            .permissions
            .iter()
            .any(|permission| permission == "pos:use")
    {
        return Err("Your session changed. Sign in again before switching staff.".to_owned());
    }
    if input.credential.is_empty() || input.credential.chars().count() > 128 {
        return Err("Enter the selected staff member's PIN or password.".to_owned());
    }

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let target = transaction.query_row(
        "SELECT u.password_hash, u.pin_hash, u.id, u.business_id, s.id, u.full_name, u.username,
                r.role_key, r.display_name, r.permissions_json, b.name, s.name, b.currency_code,r.workspace_mode,COALESCE(b.owner_user_id=u.id,0)
         FROM users u JOIN roles r ON r.id = u.role_id
         JOIN businesses b ON b.id = u.business_id
         JOIN stores s ON s.id = ?3 AND s.business_id = u.business_id AND s.is_active = 1
         WHERE u.id = ?1 AND u.business_id = ?2 AND u.is_active = 1",
        params![input.user_id, current.business_id, current.store_id],
        |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?,
            SessionUser {
                user_id: row.get(2)?, business_id: row.get(3)?, store_id: row.get(4)?,
                full_name: row.get(5)?, username: row.get(6)?, role_key: row.get(7)?,
                role_name: row.get(8)?, permissions: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
                workspace_mode: row.get(13)?, is_owner: row.get::<_, i64>(14)? != 0,
                business_name: row.get(10)?, store_name: row.get(11)?, currency_code: row.get(12)?,
            },
        )),
    ).optional().map_err(|error| error.to_string())?;
    let Some((password_hash, pin_hash, next_session)) = target else {
        return Err("That staff profile could not be verified.".to_owned());
    };
    if !next_session
        .permissions
        .iter()
        .any(|permission| permission == "pos:use")
    {
        return Err("That profile cannot use this register.".to_owned());
    }

    let locked_until = transaction
        .query_row(
            "SELECT locked_until FROM staff_switch_attempts WHERE user_id = ?1",
            [&next_session.user_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    if let Some(remaining_seconds) = locked_until
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            let milliseconds = value
                .with_timezone(&Utc)
                .signed_duration_since(Utc::now())
                .num_milliseconds()
                .max(0);
            (milliseconds + 999) / 1000
        })
        .filter(|seconds| *seconds > 0)
    {
        return Err(format!(
            "Too many attempts. Wait {remaining_seconds} seconds, then try again."
        ));
    }

    let credential_matches = verify_password(&input.credential, &password_hash)
        || pin_hash
            .as_deref()
            .is_some_and(|hash| verify_password(&input.credential, hash));
    if !credential_matches {
        let failed_attempts = transaction
            .query_row(
                "SELECT failed_attempts FROM staff_switch_attempts WHERE user_id = ?1",
                [&next_session.user_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .unwrap_or(0)
            + 1;
        let locked_until = if failed_attempts >= 5 {
            Some((Utc::now() + Duration::seconds(30)).to_rfc3339_opts(SecondsFormat::Millis, true))
        } else {
            None
        };
        transaction.execute(
            "INSERT INTO staff_switch_attempts (user_id, failed_attempts, locked_until) VALUES (?1, ?2, ?3)
             ON CONFLICT(user_id) DO UPDATE SET failed_attempts = excluded.failed_attempts, locked_until = excluded.locked_until",
            params![next_session.user_id, failed_attempts, locked_until],
        ).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        return Err("That PIN or password was not recognized.".to_owned());
    }

    transaction
        .execute(
            "DELETE FROM staff_switch_attempts WHERE user_id = ?1",
            [&next_session.user_id],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &current,
        "staff_profile_switched",
        "user",
        Some(&next_session.user_id),
        serde_json::json!({ "fromUserId": current.user_id, "toRole": next_session.role_key }),
    )?;
    crate::attendance::start_for_login(&transaction, &next_session, Utc::now())?;
    transaction.commit().map_err(|error| error.to_string())?;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = Some(next_session.clone());
    Ok(next_session)
}

#[tauri::command]
pub fn set_temporary_cashier_mode(
    input: TemporaryCashierModeInput,
    state: State<'_, AppState>,
) -> Result<SessionUser, String> {
    let active = active_session(&state, "pos:use")?;
    let actor = active.user.clone();
    drop(active);
    if !matches!(actor.role_key.as_str(), "owner" | "admin" | "manager") {
        return Err(
            "Temporary cashier mode is available to owners, administrators, and managers."
                .to_owned(),
        );
    }
    if input.credential.is_empty() || input.credential.chars().count() > 128 {
        return Err("Enter your PIN or password to change cashier mode.".to_owned());
    }

    let _gate = state.session_gate.write().map_err(|_| {
        "The local session is unavailable. Restart Advanced POS and sign in again.".to_owned()
    })?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let (password_hash, pin_hash, role_key, role_permissions) = transaction
        .query_row(
            "SELECT u.password_hash,u.pin_hash,r.role_key,r.permissions_json
             FROM users u JOIN roles r ON r.id=u.role_id AND r.business_id=u.business_id
             WHERE u.id=?1 AND u.business_id=?2 AND u.is_active=1",
            params![actor.user_id, actor.business_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Your account is no longer active.".to_owned())?;
    let role_permissions: Vec<String> = serde_json::from_str(&role_permissions).unwrap_or_default();
    if !matches!(role_key.as_str(), "owner" | "admin" | "manager")
        || !role_permissions
            .iter()
            .any(|permission| permission == "pos:use")
    {
        return Err("Your account no longer has access to temporary cashier mode.".to_owned());
    }

    let locked_until = transaction
        .query_row(
            "SELECT locked_until FROM staff_switch_attempts WHERE user_id=?1",
            [&actor.user_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    if let Some(remaining_seconds) = locked_until
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            let milliseconds = value
                .with_timezone(&Utc)
                .signed_duration_since(Utc::now())
                .num_milliseconds()
                .max(0);
            (milliseconds + 999) / 1000
        })
        .filter(|seconds| *seconds > 0)
    {
        return Err(format!(
            "Too many attempts. Wait {remaining_seconds} seconds, then try again."
        ));
    }
    let credential_matches = verify_password(&input.credential, &password_hash)
        || pin_hash
            .as_deref()
            .is_some_and(|hash| verify_password(&input.credential, hash));
    if !credential_matches {
        let failed_attempts = transaction
            .query_row(
                "SELECT failed_attempts FROM staff_switch_attempts WHERE user_id=?1",
                [&actor.user_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .unwrap_or(0)
            + 1;
        let locked_until = if failed_attempts >= 5 {
            Some((Utc::now() + Duration::seconds(30)).to_rfc3339_opts(SecondsFormat::Millis, true))
        } else {
            None
        };
        transaction.execute(
            "INSERT INTO staff_switch_attempts(user_id,failed_attempts,locked_until) VALUES(?1,?2,?3)
             ON CONFLICT(user_id) DO UPDATE SET failed_attempts=excluded.failed_attempts,locked_until=excluded.locked_until",
            params![actor.user_id, failed_attempts, locked_until],
        ).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        return Err("Your PIN or password was not recognized.".to_owned());
    }

    if !input.enabled {
        let station_key =
            crate::stations::station_key_for_device(&transaction, &state.device_id, &actor)?;
        let drawer_open: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open')",
                params![actor.store_id, station_key],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if drawer_open {
            return Err(
                "Close the open cash drawer before ending temporary cashier mode.".to_owned(),
            );
        }
    }

    let mut session = state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())?
        .clone()
        .filter(|session| session.user_id == actor.user_id)
        .ok_or_else(|| "Your session changed. Sign in again before continuing.".to_owned())?;
    transaction
        .execute(
            "DELETE FROM staff_switch_attempts WHERE user_id=?1",
            [&actor.user_id],
        )
        .map_err(|error| error.to_string())?;
    if input.enabled {
        crate::attendance::start_for_manual_action(&transaction, &actor, Utc::now())?;
        if !session
            .permissions
            .iter()
            .any(|value| value == "pos:transact")
        {
            session.permissions.push("pos:transact".to_owned());
        }
    } else {
        session.permissions.retain(|value| value != "pos:transact");
    }
    add_audit_event(
        &transaction,
        &session,
        if input.enabled {
            "temporary_cashier_mode_enabled"
        } else {
            "temporary_cashier_mode_disabled"
        },
        "user",
        Some(&session.user_id),
        serde_json::json!({ "role": role_key }),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = Some(session.clone());
    Ok(session)
}

#[tauri::command]
pub fn get_suppliers(state: State<'_, AppState>) -> Result<Vec<Supplier>, String> {
    let session = active_session(&state, "suppliers:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = connection
        .prepare("SELECT id, name, contact_person, email, phone, address, is_active FROM suppliers WHERE store_id = ?1 ORDER BY name COLLATE NOCASE")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session.store_id.as_str()], |row| {
            Ok(Supplier {
                id: row.get(0)?,
                name: row.get(1)?,
                contact_person: row.get(2)?,
                email: row.get(3)?,
                phone: row.get(4)?,
                address: row.get(5)?,
                is_active: row.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_supplier(input: SupplierInput, state: State<'_, AppState>) -> Result<Supplier, String> {
    let session = active_session(&state, "suppliers:write")?;
    let name = normalized_text(&input.name);
    if name.is_empty() {
        return Err("Enter a supplier name.".to_owned());
    }
    let normalize_optional = |value: Option<String>| {
        value
            .map(|text| normalized_text(&text))
            .filter(|text| !text.is_empty())
    };
    let contact_person = normalize_optional(input.contact_person);
    let email = normalize_optional(input.email);
    let phone = normalize_optional(input.phone);
    let address = normalize_optional(input.address);
    let is_new = input.id.is_none();
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO suppliers (id, store_id, name, contact_person, email, phone, address, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, contact_person = excluded.contact_person,
             email = excluded.email, phone = excluded.phone, address = excluded.address, is_active = excluded.is_active
             WHERE suppliers.store_id = excluded.store_id",
            params![id, session.store_id, name, contact_person, email, phone, address, input.is_active as i64, now()],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &session,
        if is_new {
            "supplier_created"
        } else {
            "supplier_updated"
        },
        "supplier",
        Some(&id),
        serde_json::json!({ "name": name }),
    )?;
    let supplier = transaction
        .query_row(
            "SELECT id, name, contact_person, email, phone, address, is_active FROM suppliers WHERE id = ?1 AND store_id = ?2",
            params![id, session.store_id],
            |row| {
                Ok(Supplier {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    contact_person: row.get(2)?,
                    email: row.get(3)?,
                    phone: row.get(4)?,
                    address: row.get(5)?,
                    is_active: row.get::<_, i64>(6)? != 0,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(supplier)
}

#[tauri::command]
pub fn adjust_stock(
    input: StockAdjustmentInput,
    state: State<'_, AppState>,
) -> Result<Product, String> {
    let session = active_session(&state, "inventory:write")?;
    let note = normalized_text(&input.note);
    if input.quantity_delta == 0 {
        return Err("Enter a non-zero stock change.".to_owned());
    }
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let product = get_product(&transaction, &session.store_id, &input.product_id)?;
    let new_quantity = product
        .stock_quantity
        .checked_add(input.quantity_delta)
        .ok_or_else(|| "That stock change is too large.".to_owned())?;
    if new_quantity < 0 {
        return Err("Stock cannot be reduced below zero.".to_owned());
    }
    transaction
        .execute(
            "UPDATE products SET stock_quantity = ?1, updated_at = ?2 WHERE id = ?3 AND store_id = ?4",
            params![new_quantity, now(), product.id, session.store_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO stock_movements (id, store_id, product_id, user_id, quantity_delta, reason, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'adjustment', ?6, ?7)",
            params![
                Uuid::new_v4().to_string(), session.store_id, product.id, session.user_id,
                input.quantity_delta, note, now()
            ],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &session,
        "stock_adjusted",
        "product",
        Some(&product.id),
        serde_json::json!({ "delta": input.quantity_delta, "note": note }),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    get_product(&connection, &session.store_id, &product.id)
}

#[tauri::command]
pub fn checkout_sale(
    input: CheckoutInput,
    state: State<'_, AppState>,
) -> Result<SaleReceipt, String> {
    let session = active_session(&state, "pos:use")?;
    require_pos_transaction(&session)?;
    let ticket_id = input.ticket_id.clone();
    let workspace_revision = input.workspace_revision;
    if ticket_id.is_none() && input.discount_minor > 0 {
        return Err(
            "Discounted checkout requires a saved register ticket and manager approval.".to_owned(),
        );
    }
    if ticket_id.is_some() != workspace_revision.is_some() {
        return Err("The register ticket changed. Reload it before taking payment.".to_owned());
    }

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(ticket_id) = ticket_id.as_deref() {
        if let Some(receipt) = sale_receipt_by_ticket(&transaction, &session.store_id, ticket_id)? {
            return Ok(receipt);
        }
    }

    let mut input = input;
    let mut workspace = if let (Some(ticket_id), Some(expected_revision)) =
        (ticket_id.as_deref(), workspace_revision)
    {
        let workspace = load_register_workspace(&transaction, &session)?;
        if workspace.revision != expected_revision || workspace.active.id != ticket_id {
            return Err("The register ticket changed. Reload it before taking payment.".to_owned());
        }
        if workspace.active.lines.is_empty() {
            return Err("Add at least one item to the sale.".to_owned());
        }
        let mut lines = Vec::with_capacity(workspace.active.lines.len());
        for line in &workspace.active.lines {
            let product = get_product(&transaction, &session.store_id, &line.product_id)?;
            if !product.is_active {
                return Err(format!(
                    "{} is no longer available for sale. Remove it before checkout.",
                    product.name
                ));
            }
            if product.price_minor != line.accepted_unit_price_minor {
                return Err(format!(
                    "The price of {} changed. Review and accept the current price before checkout.",
                    product.name
                ));
            }
            lines.push(crate::models::CheckoutLineInput {
                product_id: line.product_id.clone(),
                quantity: line.quantity,
            });
        }
        input.customer_id = workspace.active.customer_id.clone();
        input.lines = lines;
        input.discount_minor = workspace.active.discount_minor;
        input.payment_method = workspace.active.payment_method;
        input.payment_option_id = workspace.active.payment_option_id.clone();
        input.payment_reference = workspace.active.payment_reference.clone();
        input.amount_tendered_minor = workspace.active.amount_tendered_minor;
        Some(workspace)
    } else {
        None
    };

    if input.lines.is_empty() {
        return Err("Add at least one item to the sale.".to_owned());
    }
    if input.lines.len() > 500 {
        return Err("A sale cannot contain more than 500 lines.".to_owned());
    }
    if let Some(customer_id) = input.customer_id.as_deref() {
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM customers WHERE id = ?1 AND store_id = ?2)",
                params![customer_id, session.store_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !exists {
            return Err("The selected customer is not available in this store.".to_owned());
        }
    }

    let mut products = Vec::with_capacity(input.lines.len());
    let mut priced_lines = Vec::with_capacity(input.lines.len());
    for line in &input.lines {
        if line.quantity <= 0 {
            return Err("Item quantities must be positive whole numbers.".to_owned());
        }
        let product = get_product(&transaction, &session.store_id, &line.product_id)?;
        if !product.is_active {
            return Err(format!("{} is no longer available for sale.", product.name));
        }
        if product.stock_quantity < line.quantity {
            return Err(format!(
                "Only {} {} remain in stock.",
                product.stock_quantity, product.name
            ));
        }
        priced_lines.push(PricedLine {
            quantity: line.quantity,
            unit_price_minor: product.price_minor,
        });
        products.push((product, line.quantity));
    }

    let tax_rate_bps = crate::discounts::tax_rate_bps(&transaction, &session.store_id)?;
    let totals = calculate_sale_totals(&priced_lines, input.discount_minor, tax_rate_bps)
        .map_err(|error| error.to_string())?;
    let discount_approval = if totals.discount_minor > 0 {
        let ticket = workspace
            .as_ref()
            .map(|workspace| &workspace.active)
            .ok_or_else(|| "Discounted checkout requires a saved ticket.".to_owned())?;
        Some(crate::discounts::validate_checkout_discount(
            &transaction,
            &session,
            ticket,
        )?)
    } else {
        None
    };
    let line_allocations =
        allocate_sale_components(&priced_lines, totals).map_err(|error| error.to_string())?;
    if totals.total_minor <= 0 {
        return Err("The sale total must be greater than zero.".to_owned());
    }

    let payment_detail = crate::payment_options::validate(
        &transaction,
        &session.store_id,
        input.payment_option_id.as_deref(),
        input.payment_method,
        &input.payment_reference,
    )?;
    let change_due_minor = match input.payment_method {
        PaymentMethod::Cash if input.amount_tendered_minor < totals.total_minor => {
            return Err("Cash received is less than the amount due.".to_owned());
        }
        PaymentMethod::Cash => input.amount_tendered_minor - totals.total_minor,
        _ if input.amount_tendered_minor != totals.total_minor => {
            return Err("The payment amount must match the amount due.".to_owned());
        }
        _ => 0,
    };

    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    let date_stamp = Utc::now().format("%Y%m%d").to_string();
    let receipt_number = format!("POS-{date_stamp}-{}", &id[..8].to_uppercase());
    let station_key =
        crate::stations::station_key_for_device(&transaction, &state.device_id, &session)?;
    let station_name =
        crate::stations::station_name_for_key(&transaction, &session.store_id, &station_key)?;
    let cash_shift_id = if matches!(input.payment_method, PaymentMethod::Cash) {
        Some(transaction
            .query_row(
                "SELECT id FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open'",
                params![session.store_id, station_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Open the cash drawer before completing a cash sale.".to_owned())?)
    } else {
        None
    };
    transaction
        .execute(
            "INSERT INTO sales (id, store_id, cashier_id, customer_id, receipt_number, status,
             subtotal_minor, discount_minor, tax_minor, total_minor, created_at,
             source_ticket_id, amount_tendered_minor, change_due_minor, cash_shift_id, station_key, station_name_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, 'completed', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                id,
                session.store_id,
                session.user_id,
                input.customer_id,
                receipt_number,
                totals.subtotal_minor,
                totals.discount_minor,
                totals.tax_minor,
                totals.total_minor,
                timestamp,
                ticket_id,
                input.amount_tendered_minor,
                change_due_minor,
                cash_shift_id,
                station_key,
                station_name,
            ],
        )
        .map_err(|error| error.to_string())?;
    if let Some(proof) = discount_approval.as_ref() {
        crate::discounts::consume_checkout_discount(&transaction, proof, &session.store_id, &id)?;
    }

    let mut receipt_items = Vec::with_capacity(products.len());
    for ((product, quantity), allocation) in products.into_iter().zip(line_allocations) {
        let line_total = i64::try_from(i128::from(product.price_minor) * i128::from(quantity))
            .map_err(|_| "The sale line total is too large.".to_owned())?;
        let line_id = Uuid::new_v4().to_string();
        transaction
            .execute(
                "INSERT INTO sale_items (id, sale_id, product_id, sku_snapshot, name_snapshot,
                 quantity, unit_price_minor, line_total_minor, unit_cost_minor_snapshot,
                 line_discount_minor, line_tax_minor)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    line_id,
                    id,
                    product.id,
                    product.sku,
                    product.name,
                    quantity,
                    product.price_minor,
                    line_total,
                    product.cost_minor,
                    allocation.discount_minor,
                    allocation.tax_minor,
                ],
            )
            .map_err(|error| error.to_string())?;
        let updated = transaction
            .execute(
                "UPDATE products SET stock_quantity = stock_quantity - ?1, updated_at = ?2
                 WHERE id = ?3 AND store_id = ?4 AND stock_quantity >= ?1 AND is_active = 1",
                params![quantity, timestamp, product.id, session.store_id],
            )
            .map_err(|error| error.to_string())?;
        if updated != 1 {
            return Err(format!("{} no longer has enough stock.", product.name));
        }
        transaction
            .execute(
                "INSERT INTO stock_movements (id, store_id, product_id, user_id, quantity_delta,
                 reason, reference_id, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'sale', ?6, 'Sold at register', ?7)",
                params![
                    Uuid::new_v4().to_string(),
                    session.store_id,
                    product.id,
                    session.user_id,
                    -quantity,
                    id,
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
        receipt_items.push(SaleReceiptItem {
            product_id: product.id,
            sku: product.sku,
            name: product.name,
            quantity,
            unit_price_minor: product.price_minor,
            line_total_minor: line_total,
        });
    }

    transaction
        .execute(
            "INSERT INTO payments (id, sale_id, method, amount_minor, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![Uuid::new_v4().to_string(), id, input.payment_method.as_str(), totals.total_minor, timestamp],
        )
        .map_err(|error| error.to_string())?;
    if matches!(input.payment_method, PaymentMethod::Cash) {
        crate::cash_locations::record_cash_sale(
            &transaction,
            &session,
            &id,
            cash_shift_id.as_deref(),
            totals.total_minor,
            &timestamp,
        )?;
    }
    add_audit_event(
        &transaction,
        &session,
        "sale_completed",
        "sale",
        Some(&id),
        serde_json::json!({ "receiptNumber": receipt_number, "totalMinor": totals.total_minor,"discountRequesterUserId":discount_approval.as_ref().map(|proof|proof.requester_user_id.as_str()),"discountApproverUserId":discount_approval.as_ref().map(|proof|proof.approver_user_id.as_str()) }),
    )?;
    crate::accounting::post_sale_for_session(&transaction, &session, &id)?;
    crate::payment_options::record(
        &transaction,
        &id,
        input.payment_option_id.as_deref(),
        &payment_detail,
    )?;
    let receipt = SaleReceipt {
        payment_detail: Some(payment_detail),
        id,
        receipt_number,
        created_at: timestamp,
        cashier_name: session.full_name.clone(),
        items: receipt_items,
        totals,
        payment_method: input.payment_method,
        amount_tendered_minor: input.amount_tendered_minor,
        change_due_minor,
    };
    if let Some(workspace) = workspace.as_mut() {
        let ticket_id = ticket_id
            .as_deref()
            .expect("workspace checkout requires a ticket id");
        workspace.last_completed = Some(CompletedRegisterSale {
            ticket_id: ticket_id.to_owned(),
            customer_id: input.customer_id.clone(),
            receipt: receipt.clone(),
        });
        workspace.active = empty_register_ticket(&session);
        persist_register_workspace(
            &transaction,
            &session,
            workspace_revision.expect("workspace checkout requires a revision"),
            workspace,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;

    Ok(receipt)
}

#[tauri::command]
pub fn get_dashboard_summary(state: State<'_, AppState>) -> Result<DashboardSummary, String> {
    let session = active_session(&state, "pos:use")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let (today_sales_minor, today_sales_count) = connection
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0), COUNT(*)
             FROM sales WHERE store_id = ?1 AND status = 'completed' AND substr(created_at, 1, 10) = ?2",
            params![session.store_id, today],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let low_stock_count = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE store_id = ?1 AND is_active = 1 AND stock_quantity <= reorder_point",
            [&session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let product_count = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE store_id = ?1 AND is_active = 1",
            [&session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(DashboardSummary {
        today_sales_minor,
        today_sales_count,
        low_stock_count,
        product_count,
    })
}

#[tauri::command]
pub fn save_expense(input: ExpenseInput, state: State<'_, AppState>) -> Result<Expense, String> {
    let session = active_session(&state, "expenses:write")?;
    let category = normalized_text(&input.category);
    let description = input
        .description
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let cash_source_note = input
        .cash_source_note
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let cost_center_kind = input.cost_center_kind.trim().to_owned();
    let cost_center_station_key = input
        .cost_center_station_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let idempotency_key = normalized_text(&input.idempotency_key);
    if category.is_empty() || category.chars().count() > 80 {
        return Err("Enter an expense category with no more than 80 characters.".to_owned());
    }
    if description
        .as_ref()
        .is_some_and(|value| value.chars().count() > 500)
    {
        return Err("Keep the expense description under 500 characters.".to_owned());
    }
    if input.amount_minor <= 0 {
        return Err("Expense amount must be greater than zero.".to_owned());
    }
    if !matches!(
        input.payment_method.as_str(),
        "cash" | "mobile_money" | "bank_transfer" | "other"
    ) {
        return Err("Choose a supported payment method.".to_owned());
    }
    if idempotency_key.is_empty() || idempotency_key.len() > 100 {
        return Err("An expense retry key between 1 and 100 characters is required.".to_owned());
    }
    if cash_source_note
        .as_ref()
        .is_some_and(|value| value.chars().count() > 100)
    {
        return Err("Keep the cash source name under 100 characters.".to_owned());
    }
    if !matches!(
        cost_center_kind.as_str(),
        "station_direct" | "store_overhead"
    ) {
        return Err("Choose a station or store-wide expense cost center.".to_owned());
    }
    if cost_center_station_key
        .as_ref()
        .is_some_and(|value| value.len() > 80)
    {
        return Err("Choose a valid register station for this expense.".to_owned());
    }
    if (cost_center_kind == "station_direct") != cost_center_station_key.is_some() {
        return Err("Choose a station for a direct expense, or choose store overhead.".to_owned());
    }
    match (input.payment_method.as_str(), input.cash_origin.as_str()) {
        ("cash", "drawer") if cash_source_note.is_none() => {}
        ("cash", "other_cash") => {}
        ("cash", _) => {
            return Err("Choose the register drawer or a tracked cash location.".to_owned())
        }
        (_, "not_applicable") if cash_source_note.is_none() => {}
        _ => return Err("Cash origin must match the selected payment method.".to_owned()),
    }

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let cash_location_id = if input.payment_method == "cash" {
        crate::cash_locations::resolve_source_location(
            &transaction,
            &state.device_id,
            &session,
            &input.cash_origin,
            input.cash_location_id.as_deref(),
        )?
    } else {
        None
    };
    let existing: Option<Expense> = transaction
        .query_row(
            "SELECT e.id,e.category,e.description,e.amount_minor,e.payment_method,e.cash_origin,e.cash_source_note,e.cost_center_kind,e.cost_center_station_key,e.cost_center_station_name_snapshot,e.occurred_at,e.created_at,u.full_name
             FROM expenses e JOIN users u ON u.id=e.user_id
             WHERE e.store_id=?1 AND e.idempotency_key=?2",
            params![session.store_id,idempotency_key],
            |row| Ok(Expense {
                id: row.get(0)?, category: row.get(1)?, description: row.get(2)?, amount_minor: row.get(3)?,
                payment_method: row.get(4)?, cash_origin: row.get(5)?, cash_source_note: row.get(6)?,
                cost_center_kind: row.get(7)?, cost_center_station_key: row.get(8)?,
                cost_center_station_name_snapshot: row.get(9)?, occurred_at: row.get(10)?,
                created_at: row.get(11)?, recorded_by: row.get(12)?,
            }),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(existing) = existing {
        if existing.category != category
            || existing.description != description
            || existing.amount_minor != input.amount_minor
            || existing.payment_method != input.payment_method
            || existing.cash_origin != input.cash_origin
            || existing.cash_source_note != cash_source_note
            || existing.cost_center_kind != cost_center_kind
            || existing.cost_center_station_key != cost_center_station_key
            || transaction
                .query_row(
                    "SELECT cash_location_id FROM expenses WHERE id=?1",
                    [&existing.id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .map_err(|error| error.to_string())?
                != cash_location_id
        {
            return Err(
                "This expense retry key was already used for different details.".to_owned(),
            );
        }
        return Ok(existing);
    }
    let cost_center_station_name_snapshot = if let Some(station_key) =
        cost_center_station_key.as_deref()
    {
        let may_assign_other_station = session.is_owner
            || session
                .permissions
                .iter()
                .any(|permission| matches!(permission.as_str(), "settings:write" | "reports:read"));
        let current_station_key = if may_assign_other_station {
            None
        } else {
            Some(crate::stations::station_key_for_device(
                &transaction,
                &state.device_id,
                &session,
            )?)
        };
        if current_station_key
            .as_deref()
            .is_some_and(|current| current != station_key)
        {
            return Err(
                "You may only assign an expense to this device's register station.".to_owned(),
            );
        }
        Some(
            transaction
                .query_row(
                    "SELECT display_name FROM register_stations WHERE business_id=?1 AND store_id=?2 AND station_key=?3 AND is_active=1",
                    params![session.business_id,session.store_id,station_key],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Choose an active register station in this store.".to_owned())?,
        )
    } else {
        None
    };
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    transaction
        .execute(
            "INSERT INTO expenses (id, store_id, user_id, category, description, amount_minor, payment_method, occurred_at, created_at, cash_origin, cash_source_note, idempotency_key, cash_location_id, cost_center_kind, cost_center_station_key, cost_center_station_name_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                session.store_id,
                session.user_id,
                category,
                description,
                input.amount_minor,
                input.payment_method,
                timestamp,
                input.cash_origin,
                cash_source_note,
                idempotency_key,
                cash_location_id,
                cost_center_kind,
                cost_center_station_key,
                cost_center_station_name_snapshot
            ],
        )
        .map_err(|error| error.to_string())?;
    if input.cash_origin == "drawer" {
        crate::cash_drawer::record_source_paid_out(
            &transaction,
            &state.device_id,
            &session.user,
            "expense",
            &id,
            input.amount_minor,
            &format!("Expense · {category}"),
            &timestamp,
        )?;
    }
    if input.payment_method == "cash" {
        let shift_id = if input.cash_origin == "drawer" {
            let station_key =
                crate::stations::station_key_for_device(&transaction, &state.device_id, &session)?;
            transaction.query_row(
                "SELECT id FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open'",
                params![session.store_id, station_key],
                |row| row.get::<_, String>(0),
            ).optional().map_err(|error|error.to_string())?
        } else {
            None
        };
        crate::cash_locations::record_source_cash_outflow(
            &transaction,
            &session,
            "expense",
            &id,
            shift_id.as_deref(),
            input.amount_minor,
            &format!("Expense · {category}"),
            &timestamp,
        )?;
    }
    add_audit_event(
        &transaction,
        &session,
        "expense_recorded",
        "expense",
        Some(&id),
        serde_json::json!({
            "category": category,
            "amountMinor": input.amount_minor,
            "paymentMethod": input.payment_method,
            "cashOrigin": input.cash_origin,
            "cashSourceNote": cash_source_note,
            "costCenterKind": cost_center_kind,
            "costCenterStationKey": cost_center_station_key,
            "costCenterStationName": cost_center_station_name_snapshot,
            "idempotencyKey": idempotency_key
        }),
    )?;
    crate::accounting::post_expense_for_session(&transaction, &session, &id)?;
    transaction.commit().map_err(|error| error.to_string())?;

    Ok(Expense {
        id,
        category,
        description,
        amount_minor: input.amount_minor,
        payment_method: input.payment_method,
        cash_origin: input.cash_origin,
        cash_source_note,
        cost_center_kind,
        cost_center_station_key,
        cost_center_station_name_snapshot,
        occurred_at: timestamp.clone(),
        created_at: timestamp,
        recorded_by: session.full_name.clone(),
    })
}

fn purchase_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PurchaseSummary> {
    Ok(PurchaseSummary {
        id: row.get(0)?,
        reference: row.get(1)?,
        supplier_name: row.get(2)?,
        received_at: row.get(3)?,
        total_minor: row.get(4)?,
        item_count: row.get(5)?,
        items_summary: row.get(6)?,
        recorded_by: row.get(7)?,
    })
}

fn get_purchase_summary(
    connection: &Connection,
    store_id: &str,
    purchase_id: &str,
) -> Result<PurchaseSummary, String> {
    connection
        .query_row(
            "SELECT p.id, p.reference, s.name, p.received_at, p.total_minor,
                    COALESCE(SUM(pi.quantity), 0),
                    COALESCE(GROUP_CONCAT(pi.product_name || ' × ' || pi.quantity, ', '), ''),
                    u.full_name
             FROM purchases p
             LEFT JOIN suppliers s ON s.id = p.supplier_id
             JOIN users u ON u.id = p.user_id
             LEFT JOIN purchase_items pi ON pi.purchase_id = p.id
             WHERE p.store_id = ?1 AND p.id = ?2
             GROUP BY p.id",
            params![store_id, purchase_id],
            purchase_summary_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The received purchase could not be found.".to_owned())
}

#[tauri::command]
pub fn receive_purchase(
    input: PurchaseInput,
    state: State<'_, AppState>,
) -> Result<PurchaseSummary, String> {
    receive_purchase_for_state(input, &state)
}

pub(crate) fn receive_purchase_for_state(
    input: PurchaseInput,
    state: &AppState,
) -> Result<PurchaseSummary, String> {
    let session = active_session(state, "purchases:write")?;
    if input.lines.is_empty() || input.lines.len() > 100 {
        return Err("Add between 1 and 100 products to this delivery.".to_owned());
    }
    let supplier_id = input
        .supplier_id
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let purchase_order_id = input
        .purchase_order_id
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    let idempotency_key = input
        .idempotency_key
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    if idempotency_key
        .as_ref()
        .is_some_and(|value| value.chars().count() > 100)
    {
        return Err("The delivery request key is invalid.".to_owned());
    }
    let reference = input
        .reference
        .as_deref()
        .map(normalized_text)
        .filter(|value| !value.is_empty());
    if reference
        .as_ref()
        .is_some_and(|value| value.chars().count() > 100)
    {
        return Err("Keep the supplier reference under 100 characters.".to_owned());
    }

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;

    if let Some(key) = idempotency_key.as_deref() {
        let prior_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM purchases WHERE store_id=?1 AND idempotency_key=?2",
                params![session.store_id, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(prior_id) = prior_id {
            drop(transaction);
            return get_purchase_summary(&connection, &session.store_id, &prior_id);
        }
    }

    let order_supplier_id = if let Some(order_id) = purchase_order_id.as_deref() {
        let order: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT status,supplier_id FROM purchase_orders WHERE id=?1 AND store_id=?2",
                params![order_id, session.store_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let (status, order_supplier) =
            order.ok_or_else(|| "Purchase order not found for this store.".to_owned())?;
        if status != "placed" && status != "partially_received" {
            return Err("Only placed or partially received orders can receive goods.".to_owned());
        }
        if supplier_id
            .as_ref()
            .zip(order_supplier.as_ref())
            .is_some_and(|(received_supplier, ordered_supplier)| {
                received_supplier != ordered_supplier
            })
        {
            return Err("The delivery supplier must match the purchase order.".to_owned());
        }
        order_supplier.or_else(|| supplier_id.clone())
    } else {
        if input.lines.iter().any(|line| {
            line.purchase_order_line_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
        }) {
            return Err(
                "An order line can only be received against its purchase order.".to_owned(),
            );
        }
        supplier_id.clone()
    };
    let supplier_id = order_supplier_id;

    if let Some(supplier_id) = supplier_id.as_deref() {
        let supplier_is_active: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM suppliers WHERE id = ?1 AND store_id = ?2 AND is_active = 1)",
                params![supplier_id, session.store_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !supplier_is_active {
            return Err("Choose an active supplier from this store.".to_owned());
        }
    }

    let mut seen_products = HashSet::with_capacity(input.lines.len());
    let mut prepared_lines = Vec::with_capacity(input.lines.len());
    let mut total_minor = 0_i64;
    let mut total_units = 0_i64;
    for line in input.lines {
        let product_id = normalized_text(&line.product_id);
        if product_id.is_empty() || !seen_products.insert(product_id.clone()) {
            return Err("Each product can appear only once in a delivery.".to_owned());
        }
        if line.quantity <= 0 || line.quantity > 1_000_000 {
            return Err("Each delivered quantity must be between 1 and 1,000,000.".to_owned());
        }
        if line.unit_cost_minor < 0 {
            return Err("Unit costs must be zero or greater.".to_owned());
        }
        let product = get_product(&transaction, &session.store_id, &product_id)?;
        if !product.is_active {
            return Err(format!(
                "{} is archived and cannot receive stock.",
                product.name
            ));
        }
        let new_stock = product
            .stock_quantity
            .checked_add(line.quantity)
            .ok_or_else(|| format!("Stock quantity for {} is too large.", product.name))?;
        let line_total = line
            .unit_cost_minor
            .checked_mul(line.quantity)
            .ok_or_else(|| "The purchase total is too large.".to_owned())?;
        let purchase_order_line_id = if let Some(order_id) = purchase_order_id.as_deref() {
            let line_id = line
                .purchase_order_line_id
                .as_deref()
                .map(normalized_text)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Every ordered product must be linked to its purchase-order line.".to_owned()
                })?;
            let order_line: Option<(String, i64, i64)> = transaction.query_row(
                "SELECT pol.product_id,pol.ordered_quantity,pol.received_quantity FROM purchase_order_lines pol JOIN purchase_orders po ON po.id=pol.order_id WHERE pol.id=?1 AND po.id=?2 AND po.store_id=?3",
                params![line_id,order_id,session.store_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
            ).optional().map_err(|error| error.to_string())?;
            let (ordered_product_id, ordered_quantity, received_quantity) =
                order_line.ok_or_else(|| {
                    "An order line does not belong to this purchase order and store.".to_owned()
                })?;
            if ordered_product_id != product_id {
                return Err(
                    "The selected product does not match its purchase-order line.".to_owned(),
                );
            }
            let remaining = ordered_quantity.saturating_sub(received_quantity);
            if line.quantity > remaining {
                return Err(format!("{} has only {remaining} units remaining on the order. Record any extra units as a separate delivery.", product.name));
            }
            Some(line_id)
        } else {
            None
        };
        total_minor = total_minor
            .checked_add(line_total)
            .ok_or_else(|| "The purchase total is too large.".to_owned())?;
        total_units = total_units
            .checked_add(line.quantity)
            .ok_or_else(|| "The delivered quantity is too large.".to_owned())?;
        prepared_lines.push((
            product,
            line.quantity,
            line.unit_cost_minor,
            line_total,
            new_stock,
            purchase_order_line_id,
        ));
    }

    let purchase_id = Uuid::new_v4().to_string();
    let timestamp = now();
    transaction
        .execute(
            "INSERT INTO purchases (id, store_id, supplier_id, user_id, reference, total_minor, received_at, created_at, purchase_order_id, idempotency_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9)",
            params![
                purchase_id,
                session.store_id,
                supplier_id,
                session.user_id,
                reference,
                total_minor,
                timestamp,
                purchase_order_id,
                idempotency_key
            ],
        )
        .map_err(|error| error.to_string())?;

    for (product, quantity, unit_cost_minor, line_total_minor, new_stock, purchase_order_line_id) in
        prepared_lines
    {
        transaction
            .execute(
                "INSERT INTO purchase_items (id, purchase_id, product_id, sku, product_name, quantity, unit_cost_minor, line_total_minor, purchase_order_line_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    Uuid::new_v4().to_string(),
                    purchase_id,
                    product.id,
                    product.sku,
                    product.name,
                    quantity,
                    unit_cost_minor,
                    line_total_minor,
                    purchase_order_line_id
                ],
            )
            .map_err(|error| error.to_string())?;
        if let Some(order_line_id) = purchase_order_line_id.as_deref() {
            let updated = transaction.execute("UPDATE purchase_order_lines SET received_quantity=received_quantity+?1 WHERE id=?2 AND received_quantity+?1<=ordered_quantity",params![quantity,order_line_id]).map_err(|error| error.to_string())?;
            if updated != 1 {
                return Err("The purchase-order quantity changed. The delivery was not received; reload the order.".to_owned());
            }
        }
        transaction
            .execute(
                "UPDATE products SET stock_quantity = ?1, cost_minor = ?2, updated_at = ?3 WHERE id = ?4 AND store_id = ?5",
                params![new_stock, unit_cost_minor, timestamp, product.id, session.store_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO stock_movements (id, store_id, product_id, user_id, quantity_delta, reason, reference_id, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'receiving', ?6, ?7, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    session.store_id,
                    product.id,
                    session.user_id,
                    quantity,
                    purchase_id,
                    reference.as_deref().map(|value| format!("Supplier reference {value}")),
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    add_audit_event(
        &transaction,
        &session,
        "purchase_received",
        "purchase",
        Some(&purchase_id),
        serde_json::json!({
            "supplierId": supplier_id,
            "reference": reference,
            "itemCount": total_units,
            "totalMinor": total_minor
        }),
    )?;
    crate::accounting::post_purchase_for_session(&transaction, &session, &purchase_id)?;
    if let Some(order_id) = purchase_order_id.as_deref() {
        let remaining: i64 = transaction.query_row("SELECT COUNT(*) FROM purchase_order_lines WHERE order_id=?1 AND received_quantity<ordered_quantity",[order_id],|row|row.get(0)).map_err(|error|error.to_string())?;
        let next_status = if remaining == 0 {
            "received"
        } else {
            "partially_received"
        };
        transaction.execute("UPDATE purchase_orders SET status=?1,revision=revision+1,updated_at=?2 WHERE id=?3 AND store_id=?4 AND status IN ('placed','partially_received')",params![next_status,timestamp,order_id,session.store_id]).map_err(|error|error.to_string())?;
        add_audit_event(
            &transaction,
            &session,
            "purchase_order_receipt_linked",
            "purchase_order",
            Some(order_id),
            serde_json::json!({"purchaseId":purchase_id,"status":next_status}),
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    get_purchase_summary(&connection, &session.store_id, &purchase_id)
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn seed_demo_workspace(state: State<'_, AppState>) -> Result<SessionUser, String> {
    let business_id = Uuid::new_v4().to_string();
    let store_id = Uuid::new_v4().to_string();
    let admin_role_id = Uuid::new_v4().to_string();
    let manager_role_id = Uuid::new_v4().to_string();
    let cashier_role_id = Uuid::new_v4().to_string();
    let owner_role_id = Uuid::new_v4().to_string();
    let owner_id = Uuid::new_v4().to_string();
    let admin_id = Uuid::new_v4().to_string();
    let manager_id = Uuid::new_v4().to_string();
    let cashier_id = Uuid::new_v4().to_string();
    let category_id = Uuid::new_v4().to_string();
    let supplier_id = Uuid::new_v4().to_string();
    let customer_id = Uuid::new_v4().to_string();
    let purchase_id = Uuid::new_v4().to_string();
    let timestamp = now();
    let owner_hash = hash_password("OwnerDemo2026!")?;
    let admin_hash = hash_password("AdminDemo2026!")?;
    let manager_hash = hash_password("ManagerDemo2026!")?;
    let cashier_hash = hash_password("CashierDemo2026!")?;
    let owner_pin_hash = hash_password("135790")?;
    let admin_pin_hash = hash_password("864209")?;
    let manager_pin_hash = hash_password("246802")?;
    let cashier_pin_hash = hash_password("357913")?;

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let already_initialized: bool = transaction
        .query_row("SELECT EXISTS(SELECT 1 FROM businesses)", [], |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())?;
    if already_initialized {
        return Err("Demo data can only be loaded into a new, empty local database.".to_owned());
    }

    transaction
        .execute(
            "INSERT INTO businesses (id, name, currency_code, timezone, created_at)
             VALUES (?1, ?2, 'GHS', 'Africa/Accra', ?3)",
            params![business_id, "Accra Wood Supplies Ltd", timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO stores (id, business_id, name, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![store_id, business_id, "Aerojet Demo Counter", timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO register_stations (business_id,store_id,station_key,display_name,is_active,created_at,updated_at)
             VALUES (?1,?2,'main','Main register',1,?3,?3)",
            params![business_id, store_id, timestamp],
        )
        .map_err(|error| error.to_string())?;

    for (role_id, role_key, display_name, workspace_mode) in [
        (owner_role_id.as_str(), "owner", "Owner", "management"),
        (
            admin_role_id.as_str(),
            "admin",
            "Administrator",
            "management",
        ),
        (manager_role_id.as_str(), "manager", "Manager", "management"),
        (cashier_role_id.as_str(), "cashier", "Cashier", "cashier"),
    ] {
        let permissions = serde_json::to_string(&role_definitions_permissions(role_key))
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO roles (id, business_id, role_key, display_name, permissions_json, is_system, workspace_mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![role_id, business_id, role_key, display_name, permissions, workspace_mode],
            )
            .map_err(|error| error.to_string())?;
    }
    for (user_id, role_id, username, password_hash, full_name, pin_hash) in [
        (
            owner_id.as_str(),
            owner_role_id.as_str(),
            "owner",
            owner_hash.as_str(),
            "Demo Owner",
            owner_pin_hash.as_str(),
        ),
        (
            admin_id.as_str(),
            admin_role_id.as_str(),
            "admin",
            admin_hash.as_str(),
            "Demo Administrator",
            admin_pin_hash.as_str(),
        ),
        (
            manager_id.as_str(),
            manager_role_id.as_str(),
            "manager",
            manager_hash.as_str(),
            "Demo Manager",
            manager_pin_hash.as_str(),
        ),
        (
            cashier_id.as_str(),
            cashier_role_id.as_str(),
            "cashier",
            cashier_hash.as_str(),
            "Demo Cashier",
            cashier_pin_hash.as_str(),
        ),
    ] {
        transaction
            .execute(
                "INSERT INTO users (id, business_id, role_id, username, password_hash, full_name, pin_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![user_id, business_id, role_id, username, password_hash, full_name, pin_hash, timestamp],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE businesses SET owner_user_id=?1 WHERE id=?2",
            params![owner_id, business_id],
        )
        .map_err(|error| error.to_string())?;

    // Debug demo workspaces are ready for register testing with a truthful zero
    // opening count. Real stores must record their physical opening cash in Settings.
    let demo_register_location_id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO cash_locations(id,business_id,store_id,name,location_type,currency_code,station_key,is_active,created_by,created_at)
             VALUES(?1,?2,?3,'Main register','register','GHS','main',1,?4,?5)",
            params![demo_register_location_id, business_id, store_id, owner_id, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,idempotency_key,note,review_status,actor_user_id,effective_at,created_at)
             VALUES(?1,?2,?3,?4,'opening','in',0,'opening',?4,?5,'Demo register opening count: GHS 0.00','cleared',?6,?7,?7)",
            params![
                Uuid::new_v4().to_string(),
                business_id,
                store_id,
                demo_register_location_id,
                format!("demo-opening-{store_id}"),
                owner_id,
                timestamp
            ],
        )
        .map_err(|error| error.to_string())?;
    let demo_safe_location_id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO cash_locations(id,business_id,store_id,name,location_type,currency_code,station_key,is_active,created_by,created_at)
             VALUES(?1,?2,?3,'Main safe','safe','GHS',NULL,1,?4,?5)",
            params![demo_safe_location_id, business_id, store_id, owner_id, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,idempotency_key,note,review_status,actor_user_id,effective_at,created_at)
             VALUES(?1,?2,?3,?4,'opening','in',0,'opening',?4,?5,'Demo safe opening count: GHS 0.00','cleared',?6,?7,?7)",
            params![
                Uuid::new_v4().to_string(),
                business_id,
                store_id,
                demo_safe_location_id,
                format!("demo-opening-safe-{store_id}"),
                owner_id,
                timestamp
            ],
        )
        .map_err(|error| error.to_string())?;

    transaction
        .execute(
            "INSERT INTO categories (id, store_id, name, description, created_at)
             VALUES (?1, ?2, 'Store goods', 'Sample inventory for local workflow review', ?3)",
            params![category_id, store_id, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO suppliers (id, store_id, name, contact_person, phone, is_active, created_at)
             VALUES (?1, ?2, 'Aerojet Materials', 'Demo Supplier', '+233 20 000 0000', 1, ?3)",
            params![supplier_id, store_id, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO customers (id, store_id, name, phone, notes, created_at)
             VALUES (?1, ?2, 'Demo Walk-in Customer', '+233 24 000 0000', 'Sample customer profile', ?3)",
            params![customer_id, store_id, timestamp],
        )
        .map_err(|error| error.to_string())?;

    let products = [
        (
            "PLY-18",
            "2000000000010",
            "Plywood 18 mm",
            35_000_i64,
            27_000_i64,
            14_i64,
            3_i64,
        ),
        (
            "MDF-12",
            "2000000000027",
            "MDF sheet 12 mm",
            22_000_i64,
            16_800_i64,
            8_i64,
            2_i64,
        ),
        (
            "PINE-24",
            "2000000000034",
            "Pine timber 2 × 4",
            6_500_i64,
            4_400_i64,
            30_i64,
            8_i64,
        ),
    ];
    let mut product_ids = Vec::with_capacity(products.len());
    let mut purchase_total_minor = 0_i64;
    for (sku, barcode, name, price_minor, cost_minor, quantity, reorder_point) in products {
        let id = Uuid::new_v4().to_string();
        let line_total = cost_minor
            .checked_mul(quantity)
            .ok_or_else(|| "Demo inventory total is too large.".to_owned())?;
        purchase_total_minor = purchase_total_minor
            .checked_add(line_total)
            .ok_or_else(|| "Demo inventory total is too large.".to_owned())?;
        transaction
            .execute(
                "INSERT INTO products (id, store_id, category_id, supplier_id, sku, barcode, name, price_minor, cost_minor,
                 stock_quantity, reorder_point, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12)",
                params![
                    id,
                    store_id,
                    category_id,
                    supplier_id,
                    sku,
                    barcode,
                    name,
                    price_minor,
                    cost_minor,
                    quantity,
                    reorder_point,
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
        product_ids.push((id, sku, name, quantity, cost_minor, line_total));
    }

    transaction
        .execute(
            "INSERT INTO purchases (id, store_id, supplier_id, user_id, reference, total_minor, received_at, created_at)
             VALUES (?1, ?2, ?3, ?4, 'DEMO-OPENING-001', ?5, ?6, ?6)",
            params![purchase_id, store_id, supplier_id, owner_id, purchase_total_minor, timestamp],
        )
        .map_err(|error| error.to_string())?;
    for (product_id, sku, name, quantity, cost_minor, line_total) in product_ids {
        transaction
            .execute(
                "INSERT INTO purchase_items (id, purchase_id, product_id, sku, product_name, quantity, unit_cost_minor, line_total_minor)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    purchase_id,
                    product_id,
                    sku,
                    name,
                    quantity,
                    cost_minor,
                    line_total
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO stock_movements (id, store_id, product_id, user_id, quantity_delta, reason, reference_id, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'receiving', ?6, 'Demo opening stock', ?7)",
                params![
                    Uuid::new_v4().to_string(),
                    store_id,
                    product_id,
                    owner_id,
                    quantity,
                    purchase_id,
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "INSERT INTO expenses (id, store_id, user_id, category, description, amount_minor, payment_method, occurred_at, created_at, cash_origin, cash_source_note, cash_location_id)
             VALUES (?1, ?2, ?3, 'Utilities', 'Sample monthly power and water bill', 24500, 'mobile_money', ?4, ?4, 'not_applicable', NULL, NULL)",
            params![Uuid::new_v4().to_string(), store_id, owner_id, timestamp],
        )
        .map_err(|error| error.to_string())?;

    let session = SessionUser {
        user_id: owner_id.clone(),
        business_id: business_id.clone(),
        store_id: store_id.clone(),
        full_name: "Demo Owner".to_owned(),
        username: "owner".to_owned(),
        role_key: "owner".to_owned(),
        role_name: "Owner".to_owned(),
        workspace_mode: "management".to_owned(),
        is_owner: true,
        permissions: role_definitions_permissions("owner"),
        business_name: "Accra Wood Supplies Ltd".to_owned(),
        store_name: "Aerojet Demo Counter".to_owned(),
        currency_code: "GHS".to_owned(),
    };
    add_audit_event(
        &transaction,
        &session,
        "demo_workspace_created",
        "business",
        Some(&business_id),
        serde_json::json!({
            "accounts": ["admin", "manager", "cashier"],
            "sampleProducts": products.len(),
            "purchaseId": purchase_id
        }),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    drop(connection);
    let _gate = state
        .session_gate
        .write()
        .map_err(|_| "The local session is unavailable.".to_owned())?;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = Some(session.clone());
    Ok(session)
}

#[cfg(test)]
pub(crate) mod purchase_receipt_tests {
    use super::*;
    use crate::models::PurchaseLineInput;
    use std::path::PathBuf;

    pub(crate) fn receipt_test_state() -> AppState {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection.execute_batch(
            "CREATE TABLE businesses(id TEXT PRIMARY KEY,name TEXT NOT NULL,currency_code TEXT NOT NULL,owner_user_id TEXT);
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,name TEXT NOT NULL,is_active INTEGER NOT NULL);
             CREATE TABLE roles(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,role_key TEXT NOT NULL,display_name TEXT NOT NULL,permissions_json TEXT NOT NULL,workspace_mode TEXT NOT NULL);
             CREATE TABLE users(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,role_id TEXT NOT NULL,username TEXT NOT NULL,full_name TEXT NOT NULL,is_active INTEGER NOT NULL);
             CREATE TABLE suppliers(id TEXT PRIMARY KEY,store_id TEXT NOT NULL,name TEXT NOT NULL,is_active INTEGER NOT NULL);
             CREATE TABLE categories(id TEXT PRIMARY KEY,name TEXT);
             CREATE TABLE product_brands(id TEXT PRIMARY KEY,name TEXT);
             CREATE TABLE product_tag_catalog(id TEXT PRIMARY KEY,name TEXT);
             CREATE TABLE product_tag_links(product_id TEXT,tag_id TEXT);
             CREATE TABLE products(id TEXT PRIMARY KEY,store_id TEXT NOT NULL,sku TEXT NOT NULL,barcode TEXT,name TEXT NOT NULL,description TEXT,category_id TEXT,supplier_id TEXT,brand_id TEXT,price_minor INTEGER NOT NULL,cost_minor INTEGER,stock_quantity INTEGER NOT NULL,reorder_point INTEGER NOT NULL,is_active INTEGER NOT NULL,target_stock_quantity INTEGER NOT NULL,updated_at TEXT);
             CREATE TABLE stock_movements(id TEXT PRIMARY KEY,store_id TEXT,product_id TEXT,user_id TEXT,quantity_delta INTEGER,reason TEXT,reference_id TEXT,note TEXT,created_at TEXT);
             CREATE TABLE audit_events(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,user_id TEXT,action TEXT,entity_type TEXT,entity_id TEXT,details_json TEXT,created_at TEXT);
             CREATE TABLE purchases(id TEXT PRIMARY KEY,store_id TEXT NOT NULL,supplier_id TEXT,user_id TEXT NOT NULL,reference TEXT,total_minor INTEGER NOT NULL,received_at TEXT NOT NULL,created_at TEXT NOT NULL);
             CREATE TABLE purchase_items(id TEXT PRIMARY KEY,purchase_id TEXT NOT NULL,product_id TEXT NOT NULL,sku TEXT NOT NULL,product_name TEXT NOT NULL,quantity INTEGER NOT NULL,unit_cost_minor INTEGER NOT NULL,line_total_minor INTEGER NOT NULL,UNIQUE(purchase_id,product_id));
             CREATE TABLE accounting_accounts(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,code TEXT NOT NULL,name TEXT NOT NULL,account_type TEXT NOT NULL,is_system INTEGER NOT NULL,is_active INTEGER NOT NULL,created_at TEXT NOT NULL,UNIQUE(business_id,code));
             CREATE TABLE accounting_journals(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,store_id TEXT NOT NULL,source_type TEXT NOT NULL,source_id TEXT NOT NULL,currency_code TEXT NOT NULL,effective_at TEXT NOT NULL,recorded_at TEXT NOT NULL,created_by TEXT,description TEXT NOT NULL,coverage_status TEXT NOT NULL,reversal_of TEXT,UNIQUE(business_id,source_type,source_id));
             CREATE TABLE accounting_journal_lines(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,journal_id TEXT NOT NULL,account_id TEXT NOT NULL,line_number INTEGER NOT NULL,memo TEXT NOT NULL,debit_minor INTEGER NOT NULL,credit_minor INTEGER NOT NULL);
             CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             INSERT INTO businesses VALUES('business-a','Test business','GHS','user-a');
             INSERT INTO stores VALUES('store-a','business-a','Test store',1);
             INSERT INTO roles VALUES('role-a','business-a','owner','Owner','[\"purchases:read\",\"purchases:write\"]','owner');
             INSERT INTO users VALUES('user-a','business-a','role-a','owner','Test Owner',1);
             INSERT INTO suppliers VALUES('supplier-a','store-a','Supplier A',1);
             INSERT INTO products VALUES('product-a','store-a','SKU-A',NULL,'Test product',NULL,NULL,'supplier-a',NULL,500,90,10,2,1,15,NULL);"
        ).unwrap();
        connection
            .execute_batch(include_str!("../migrations/0038_purchase_orders.sql"))
            .unwrap();
        connection.execute("INSERT INTO purchase_orders(id,store_id,supplier_id,created_by,order_number,status,created_at,updated_at) VALUES('order-a','store-a','supplier-a','user-a','PO-000001','placed','now','now')", []).unwrap();
        connection.execute("INSERT INTO purchase_order_lines(id,order_id,product_id,product_name,sku,ordered_quantity,expected_unit_cost_minor) VALUES('line-a','order-a','product-a','Test product','SKU-A',5,100)", []).unwrap();
        let session = SessionUser {
            user_id: "user-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Test Owner".into(),
            username: "owner".into(),
            role_key: "owner".into(),
            role_name: "Owner".into(),
            workspace_mode: "owner".into(),
            is_owner: true,
            permissions: vec!["purchases:write".into()],
            business_name: "Test business".into(),
            store_name: "Test store".into(),
            currency_code: "GHS".into(),
        };
        AppState {
            data_dir: PathBuf::from("C:/nimbletill-purchase-test"),
            device_id: "test-device".into(),
            db: std::sync::Mutex::new(connection),
            active_session: std::sync::Mutex::new(Some(session)),
            session_gate: std::sync::RwLock::new(()),
            passkey_operation: std::sync::Mutex::new(()),
        }
    }

    fn order_receipt_input(quantity: i64, cost: i64, key: &str) -> PurchaseInput {
        PurchaseInput {
            purchase_order_id: Some("order-a".into()),
            idempotency_key: Some(key.into()),
            supplier_id: Some("supplier-a".into()),
            reference: Some(format!("DEL-{key}")),
            lines: vec![PurchaseLineInput {
                product_id: "product-a".into(),
                quantity,
                unit_cost_minor: cost,
                purchase_order_line_id: Some("line-a".into()),
            }],
        }
    }

    #[test]
    fn linked_partial_receipts_post_actual_cost_once_and_roll_back_with_accounting_failure() {
        let state = receipt_test_state();
        let first =
            receive_purchase_for_state(order_receipt_input(2, 120, "first"), &state).unwrap();
        assert_eq!(first.total_minor, 240);
        let connection = state.db.lock().unwrap();
        connection.execute_batch("CREATE TRIGGER reject_purchase_journal BEFORE INSERT ON accounting_journals WHEN NEW.source_type='purchase' BEGIN SELECT RAISE(ABORT,'injected journal failure'); END;").unwrap();
        drop(connection);

        assert!(receive_purchase_for_state(order_receipt_input(3, 110, "second"), &state).is_err());
        let connection = state.db.lock().unwrap();
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT stock_quantity FROM products WHERE id='product-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            12
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT received_quantity FROM purchase_order_lines WHERE id='line-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row::<String, _, _>(
                    "SELECT status FROM purchase_orders WHERE id='order-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            "partially_received"
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM purchases", [], |row| row.get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM stock_movements", [], |row| row
                    .get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM accounting_journals", [], |row| row
                    .get(0))
                .unwrap(),
            1
        );
        connection
            .execute_batch("DROP TRIGGER reject_purchase_journal;")
            .unwrap();
        drop(connection);

        let second =
            receive_purchase_for_state(order_receipt_input(3, 110, "second"), &state).unwrap();
        assert_eq!(second.total_minor, 330);
        let replay =
            receive_purchase_for_state(order_receipt_input(3, 110, "second"), &state).unwrap();
        assert_eq!(replay.id, second.id);
        let connection = state.db.lock().unwrap();
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT stock_quantity FROM products WHERE id='product-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            15
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT received_quantity FROM purchase_order_lines WHERE id='line-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            5
        );
        assert_eq!(
            connection
                .query_row::<String, _, _>(
                    "SELECT status FROM purchase_orders WHERE id='order-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            "received"
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM purchases", [], |row| row.get(0))
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM stock_movements", [], |row| row
                    .get(0))
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM accounting_journals WHERE source_type='purchase'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT SUM(line_total_minor) FROM purchase_items",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            570
        );
        drop(connection);

        let error =
            receive_purchase_for_state(order_receipt_input(1, 110, "after-complete"), &state)
                .unwrap_err();
        assert!(error.contains("Only placed or partially received"));
    }
}

#[cfg(test)]
mod attach_sale_customer_tests {
    use super::*;

    fn cashier_session() -> SessionUser {
        SessionUser {
            user_id: "cashier-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Casey Cashier".into(),
            username: "casey".into(),
            role_key: "cashier".into(),
            role_name: "Cashier".into(),
            workspace_mode: "pos".into(),
            is_owner: false,
            permissions: vec!["customers:write".into(), "pos:use".into()],
            business_name: "Test business".into(),
            store_name: "Main store".into(),
            currency_code: "GHS".into(),
        }
    }

    fn fixture() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE customers(id TEXT PRIMARY KEY,store_id TEXT NOT NULL);
                 CREATE TABLE sales(id TEXT PRIMARY KEY,store_id TEXT NOT NULL,customer_id TEXT,cashier_id TEXT NOT NULL,status TEXT NOT NULL);
                 CREATE TABLE audit_events(id TEXT PRIMARY KEY,business_id TEXT NOT NULL,store_id TEXT NOT NULL,user_id TEXT NOT NULL,action TEXT NOT NULL,entity_type TEXT NOT NULL,entity_id TEXT,details_json TEXT NOT NULL,created_at TEXT NOT NULL);
                 INSERT INTO customers VALUES('customer-a','store-a'),('customer-linked','store-a'),('customer-other-store','store-b');
                 INSERT INTO sales VALUES
                    ('sale-own','store-a',NULL,'cashier-a','completed'),
                    ('sale-other-cashier','store-a',NULL,'cashier-b','completed'),
                    ('sale-linked','store-a','customer-linked','cashier-a','completed'),
                    ('sale-refunded','store-a',NULL,'cashier-a','refunded');",
            )
            .unwrap();
        connection
    }

    fn attach(sale_id: &str, customer_id: &str) -> AttachSaleCustomerInput {
        AttachSaleCustomerInput {
            sale_id: sale_id.into(),
            customer_id: customer_id.into(),
        }
    }

    #[test]
    fn cashier_can_link_walk_in_customer_to_their_completed_sale() {
        let mut connection = fixture();
        let session = cashier_session();

        attach_sale_customer_with_session(
            attach("sale-own", "customer-a"),
            &session,
            &mut connection,
        )
        .unwrap();
        // A retry after the receipt flow is interrupted is safe and does not duplicate audit rows.
        attach_sale_customer_with_session(
            attach("sale-own", "customer-a"),
            &session,
            &mut connection,
        )
        .unwrap();

        let saved_customer: String = connection
            .query_row(
                "SELECT customer_id FROM sales WHERE id='sale-own'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let (audit_count, action, audit_sale): (i64, String, String) = connection
            .query_row(
                "SELECT COUNT(*),MAX(action),MAX(entity_id) FROM audit_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(saved_customer, "customer-a");
        assert_eq!(
            (audit_count, action.as_str(), audit_sale.as_str()),
            (1, "sale_customer_linked", "sale-own")
        );
    }

    #[test]
    fn cashier_cannot_link_customer_to_another_cashiers_sale() {
        let mut connection = fixture();
        let error = attach_sale_customer_with_session(
            attach("sale-other-cashier", "customer-a"),
            &cashier_session(),
            &mut connection,
        )
        .unwrap_err();

        assert!(error.contains("sale they completed"));
        let (customer, audits): (Option<String>, i64) = connection
            .query_row(
                "SELECT customer_id,(SELECT COUNT(*) FROM audit_events) FROM sales WHERE id='sale-other-cashier'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((customer, audits), (None, 0));
    }

    #[test]
    fn cashier_cannot_link_a_customer_from_another_store() {
        let mut connection = fixture();
        let error = attach_sale_customer_with_session(
            attach("sale-own", "customer-other-store"),
            &cashier_session(),
            &mut connection,
        )
        .unwrap_err();

        assert!(error.contains("not available in this store"));
        let (customer, audits): (Option<String>, i64) = connection
            .query_row(
                "SELECT customer_id,(SELECT COUNT(*) FROM audit_events) FROM sales WHERE id='sale-own'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((customer, audits), (None, 0));
    }

    #[test]
    fn cashier_cannot_change_existing_customer_or_link_to_refunded_sale() {
        let mut connection = fixture();
        let session = cashier_session();

        let replace_error = attach_sale_customer_with_session(
            attach("sale-linked", "customer-a"),
            &session,
            &mut connection,
        )
        .unwrap_err();
        assert!(replace_error.contains("Ask a manager"));

        let refunded_error = attach_sale_customer_with_session(
            attach("sale-refunded", "customer-a"),
            &session,
            &mut connection,
        )
        .unwrap_err();
        assert!(refunded_error.contains("completed sale"));

        let (linked_customer, audits): (String, i64) = connection
            .query_row(
                "SELECT customer_id,(SELECT COUNT(*) FROM audit_events) FROM sales WHERE id='sale-linked'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((linked_customer.as_str(), audits), ("customer-linked", 0));
    }
}
