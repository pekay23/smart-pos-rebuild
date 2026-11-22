// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreProfile {
    pub business_name: String,
    pub store_name: String,
    pub address: String,
    pub phone: String,
    pub currency_code: String,
    pub tax_rate_bps: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreProfileInput {
    pub business_name: String,
    pub store_name: String,
    pub address: String,
    pub phone: String,
    pub currency_code: String,
    pub tax_rate_bps: i64,
}

fn bounded(value: &str, label: &str, max: usize, required: bool) -> Result<String, String> {
    let value = value.trim();
    if (required && value.is_empty()) || value.chars().count() > max {
        return Err(format!(
            "{label} is required and must be no longer than {max} characters."
        ));
    }
    Ok(value.to_owned())
}

fn validate(input: StoreProfileInput) -> Result<StoreProfile, String> {
    let currency_code = input.currency_code.trim().to_ascii_uppercase();
    if currency_code.len() != 3
        || !currency_code
            .chars()
            .all(|value| value.is_ascii_alphabetic())
    {
        return Err("Currency must be a three-letter ISO code, such as GHS.".to_owned());
    }
    if !(0..=10_000).contains(&input.tax_rate_bps) {
        return Err("Tax rate must be between 0% and 100%.".to_owned());
    }
    Ok(StoreProfile {
        business_name: bounded(&input.business_name, "Business name", 120, true)?,
        store_name: bounded(&input.store_name, "Store name", 120, true)?,
        address: bounded(&input.address, "Store address", 240, false)?,
        phone: bounded(&input.phone, "Store phone", 40, false)?,
        currency_code,
        tax_rate_bps: input.tax_rate_bps,
    })
}

fn read_profile(
    connection: &rusqlite::Connection,
    session: &SessionUser,
) -> Result<StoreProfile, String> {
    connection
        .query_row(
            "SELECT b.name,s.name,COALESCE(s.address,''),COALESCE(s.phone,''),b.currency_code,
            COALESCE(sp.tax_rate_bps,0)
         FROM businesses b JOIN stores s ON s.business_id=b.id
         LEFT JOIN store_preferences sp ON sp.store_id=s.id
         WHERE b.id=?1 AND s.id=?2 AND s.is_active=1",
            params![session.business_id, session.store_id],
            |row| {
                Ok(StoreProfile {
                    business_name: row.get(0)?,
                    store_name: row.get(1)?,
                    address: row.get(2)?,
                    phone: row.get(3)?,
                    currency_code: row.get(4)?,
                    tax_rate_bps: row.get(5)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_store_profile(state: State<'_, AppState>) -> Result<StoreProfile, String> {
    let session = active_session(&state, "settings:write")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    read_profile(&connection, &session)
}

fn has_financial_activity(
    connection: &rusqlite::Connection,
    session: &SessionUser,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sales s JOIN stores st ON st.id=s.store_id WHERE st.business_id=?1)
            OR EXISTS(SELECT 1 FROM products p JOIN stores st ON st.id=p.store_id WHERE st.business_id=?1)
            OR EXISTS(SELECT 1 FROM purchases p JOIN stores st ON st.id=p.store_id WHERE st.business_id=?1)
            OR EXISTS(SELECT 1 FROM expenses e JOIN stores st ON st.id=e.store_id WHERE st.business_id=?1)
            OR EXISTS(SELECT 1 FROM supplier_bills WHERE business_id=?1)
            OR EXISTS(SELECT 1 FROM supplier_payments WHERE business_id=?1)
            OR EXISTS(SELECT 1 FROM accounting_journals WHERE business_id=?1)",
            [&session.business_id],
            |row| row.get::<_, i64>(0).map(|value| value != 0),
        )
        .map_err(|error| {
            format!("Could not verify whether the business currency can change: {error}")
        })
}

#[tauri::command]
pub fn save_store_profile(
    input: StoreProfileInput,
    state: State<'_, AppState>,
) -> Result<SessionUser, String> {
    let active = active_session(&state, "settings:write")?;
    let input = validate(input)?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let previous = read_profile(&transaction, &active)?;
    if previous.currency_code != input.currency_code
        && has_financial_activity(&transaction, &active)?
    {
        return Err("Currency cannot be changed after products or financial activity have been recorded. Add another store with the new currency instead.".to_owned());
    }
    transaction
        .execute(
            "UPDATE businesses SET name=?1,currency_code=?2 WHERE id=?3",
            params![input.business_name, input.currency_code, active.business_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.execute(
        "UPDATE stores SET name=?1,address=?2,phone=?3 WHERE id=?4 AND business_id=?5 AND is_active=1",
        params![input.store_name, input.address, input.phone, active.store_id, active.business_id],
    ).map_err(|error| error.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    transaction
        .execute(
            "INSERT OR IGNORE INTO store_preferences(store_id) VALUES(?1)",
            [&active.store_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE store_preferences SET tax_rate_bps=?1,updated_at=?2 WHERE store_id=?3",
            params![input.tax_rate_bps, now, active.store_id],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &active,
        "store.profile.updated",
        "store",
        Some(&active.store_id),
        serde_json::json!({
            "businessNameChanged": previous.business_name != input.business_name,
            "storeNameChanged": previous.store_name != input.store_name,
            "addressChanged": previous.address != input.address,
            "phoneChanged": previous.phone != input.phone,
            "currencyChanged": previous.currency_code != input.currency_code,
            "taxRateChanged": previous.tax_rate_bps != input.tax_rate_bps,
        }),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;

    let mut updated = active.user.clone();
    updated.business_name = input.business_name;
    updated.store_name = input.store_name;
    updated.currency_code = input.currency_code;
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.")? = Some(updated.clone());
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(currency_code: &str, tax_rate_bps: i64) -> StoreProfileInput {
        StoreProfileInput {
            business_name: "Demo business".into(),
            store_name: "Main store".into(),
            address: String::new(),
            phone: String::new(),
            currency_code: currency_code.into(),
            tax_rate_bps,
        }
    }

    #[test]
    fn validates_currency_and_tax_rate_before_saving() {
        assert_eq!(validate(input("ghs", 1_500)).unwrap().currency_code, "GHS");
        assert!(validate(input("GHC1", 0)).is_err());
        assert!(validate(input("GHS", -1)).is_err());
        assert!(validate(input("GHS", 10_001)).is_err());
    }

    #[test]
    fn required_profile_names_are_trimmed_and_bounded() {
        let mut valid = input("GHS", 0);
        valid.store_name = " Store 1 ".into();
        assert_eq!(validate(valid).unwrap().store_name, "Store 1");
        let mut missing = input("GHS", 0);
        missing.business_name = " ".into();
        assert!(validate(missing).is_err());
    }

    #[test]
    fn store_tax_migration_preserves_legacy_rate_and_removes_business_wide_value() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE businesses(id TEXT PRIMARY KEY);
             CREATE TABLE stores(id TEXT PRIMARY KEY, business_id TEXT);
             CREATE TABLE store_preferences(store_id TEXT PRIMARY KEY, updated_at TEXT NOT NULL DEFAULT 'now');
             CREATE TABLE app_settings(business_id TEXT,setting_key TEXT,setting_value TEXT,updated_at TEXT,
                 PRIMARY KEY(business_id,setting_key));
             INSERT INTO businesses VALUES('business-a');
             INSERT INTO stores VALUES('store-a','business-a');
             INSERT INTO store_preferences(store_id) VALUES('store-a');
             INSERT INTO app_settings VALUES('business-a','tax_rate_bps','1500','now');",
        ).unwrap();
        connection
            .execute_batch(include_str!("../migrations/0024_store_tax_settings.sql"))
            .unwrap();
        let store_tax: i64 = connection
            .query_row(
                "SELECT tax_rate_bps FROM store_preferences WHERE store_id='store-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let old_setting_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM app_settings WHERE setting_key='tax_rate_bps'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(store_tax, 1_500);
        assert_eq!(old_setting_count, 0);
    }

    #[test]
    fn currency_change_lock_covers_activity_across_the_business() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT);
             CREATE TABLE sales(store_id TEXT);
             CREATE TABLE products(store_id TEXT);
             CREATE TABLE purchases(store_id TEXT);
             CREATE TABLE expenses(store_id TEXT);
             CREATE TABLE supplier_bills(business_id TEXT);
             CREATE TABLE supplier_payments(business_id TEXT);
             CREATE TABLE accounting_journals(business_id TEXT);
             INSERT INTO stores VALUES('another-store','business-a'),('foreign-store','business-b');
             INSERT INTO products VALUES('foreign-store');",
            )
            .unwrap();
        let session = SessionUser {
            user_id: "owner".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Owner".into(),
            username: "owner".into(),
            role_key: "owner".into(),
            role_name: "Owner".into(),
            workspace_mode: "management".into(),
            is_owner: true,
            permissions: vec![],
            business_name: "Business".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        };
        assert!(!has_financial_activity(&connection, &session).unwrap());
        connection
            .execute("INSERT INTO products VALUES('another-store')", [])
            .unwrap();
        assert!(has_financial_activity(&connection, &session).unwrap());
    }

    #[test]
    fn receipt_settings_migration_adds_safe_thermal_defaults() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE store_preferences(store_id TEXT PRIMARY KEY, tax_rate_bps INTEGER NOT NULL DEFAULT 0,
                 updated_at TEXT NOT NULL DEFAULT 'now');
             INSERT INTO store_preferences(store_id) VALUES('store-a');",
        ).unwrap();
        connection
            .execute_batch(include_str!(
                "../migrations/0025_receipt_print_settings.sql"
            ))
            .unwrap();
        let (header, footer, width): (String, String, u16) = connection.query_row(
            "SELECT receipt_header_text,receipt_footer_text,receipt_paper_width_mm FROM store_preferences WHERE store_id='store-a'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(header, "");
        assert_eq!(footer, "Thank you for shopping with us.");
        assert_eq!(width, 80);
    }
}
