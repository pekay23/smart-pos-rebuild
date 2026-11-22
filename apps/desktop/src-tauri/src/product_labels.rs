// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductLabel {
    pub id: String,
    pub name: String,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductLabelInput {
    pub id: Option<String>,
    pub name: String,
    pub is_active: bool,
}

fn list_labels(
    connection: &Connection,
    session: &SessionUser,
    table: &str,
) -> Result<Vec<ProductLabel>, String> {
    let sql = match table {
        "product_brands" => "SELECT id,name,is_active FROM product_brands WHERE business_id=?1 ORDER BY is_active DESC,name COLLATE NOCASE",
        "product_tag_catalog" => "SELECT id,name,is_active FROM product_tag_catalog WHERE business_id=?1 ORDER BY is_active DESC,name COLLATE NOCASE",
        _ => return Err("Unsupported product label type.".to_owned()),
    };
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session.business_id.as_str()], |row| {
            Ok(ProductLabel {
                id: row.get(0)?,
                name: row.get(1)?,
                is_active: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_product_brands(state: State<'_, AppState>) -> Result<Vec<ProductLabel>, String> {
    let active = active_session(&state, "products:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    list_labels(&connection, &active.user, "product_brands")
}

#[tauri::command]
pub fn list_product_tags(state: State<'_, AppState>) -> Result<Vec<ProductLabel>, String> {
    let active = active_session(&state, "products:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    list_labels(&connection, &active.user, "product_tag_catalog")
}

fn save_label(
    input: ProductLabelInput,
    session: &SessionUser,
    connection: &Transaction<'_>,
    table: &str,
    kind: &str,
) -> Result<ProductLabel, String> {
    let (entity, event) = match kind {
        "brand" => ("product_brand", "product_brand_saved"),
        "tag" => ("product_tag", "product_tag_saved"),
        _ => return Err("Unsupported product label type.".to_owned()),
    };
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("Product brand and tag names must be from 1 to 80 characters.".to_owned());
    }
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp = now();
    let sql = match table {
        "product_brands" => "INSERT INTO product_brands(id,business_id,name,is_active,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)
            ON CONFLICT(id) DO UPDATE SET name=excluded.name,is_active=excluded.is_active,updated_at=excluded.updated_at WHERE product_brands.business_id=excluded.business_id",
        "product_tag_catalog" => "INSERT INTO product_tag_catalog(id,business_id,name,is_active,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)
            ON CONFLICT(id) DO UPDATE SET name=excluded.name,is_active=excluded.is_active,updated_at=excluded.updated_at WHERE product_tag_catalog.business_id=excluded.business_id",
        _ => return Err("Unsupported product label type.".to_owned()),
    };
    connection
        .execute(
            sql,
            params![
                id,
                session.business_id,
                name,
                input.is_active as i64,
                timestamp
            ],
        )
        .map_err(|error| {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                format!("A {kind} with that name already exists in this business.")
            } else {
                error.to_string()
            }
        })?;
    let saved = connection
        .query_row(
            &format!("SELECT id,name,is_active FROM {table} WHERE id=?1 AND business_id=?2"),
            params![id, session.business_id],
            |row| {
                Ok(ProductLabel {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    is_active: row.get::<_, i64>(2)? != 0,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That product label is not available in this business.".to_owned())?;
    add_audit_event(
        connection,
        session,
        event,
        entity,
        Some(&id),
        serde_json::json!({"name":saved.name,"isActive":saved.is_active}),
    )?;
    Ok(saved)
}

#[tauri::command]
pub fn save_product_brand(
    input: ProductLabelInput,
    state: State<'_, AppState>,
) -> Result<ProductLabel, String> {
    let active = active_session(&state, "products:write")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let saved = save_label(input, &active.user, &transaction, "product_brands", "brand")?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(saved)
}

#[tauri::command]
pub fn save_product_tag(
    input: ProductLabelInput,
    state: State<'_, AppState>,
) -> Result<ProductLabel, String> {
    let active = active_session(&state, "products:write")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let saved = save_label(
        input,
        &active.user,
        &transaction,
        "product_tag_catalog",
        "tag",
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(saved)
}
