// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

const MAX_COUNTED_QUANTITY: i64 = 1_000_000_000;

#[derive(Debug, PartialEq)]
enum StockReconciliationError {
    ArithmeticOverflow,
    NegativeResult,
    QuantityTooLarge,
}

fn reconciled_stock(
    counted_quantity: i64,
    movement_delta_since_count: i64,
    current_quantity: i64,
) -> Result<(i64, i64), StockReconciliationError> {
    let final_quantity = counted_quantity
        .checked_add(movement_delta_since_count)
        .ok_or(StockReconciliationError::ArithmeticOverflow)?;
    if final_quantity < 0 {
        return Err(StockReconciliationError::NegativeResult);
    }
    if final_quantity > MAX_COUNTED_QUANTITY {
        return Err(StockReconciliationError::QuantityTooLarge);
    }
    let adjustment = final_quantity
        .checked_sub(current_quantity)
        .ok_or(StockReconciliationError::ArithmeticOverflow)?;
    Ok((final_quantity, adjustment))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCountSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub revision: i64,
    pub started_at: String,
    pub posted_at: Option<String>,
    pub item_count: i64,
    pub counted_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCountLine {
    pub product_id: String,
    pub sku: String,
    pub product_name: String,
    pub expected_quantity: i64,
    pub counted_quantity: Option<i64>,
    pub counted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCountDetails {
    pub id: String,
    pub title: String,
    pub status: String,
    pub revision: i64,
    pub started_at: String,
    pub posted_at: Option<String>,
    pub lines: Vec<StockCountLine>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCountList {
    pub counts: Vec<StockCountSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartStockCountInput {
    pub title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockCountLineInput {
    pub product_id: String,
    pub counted_quantity: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveStockCountInput {
    pub count_id: String,
    pub expected_revision: i64,
    pub lines: Vec<StockCountLineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostStockCountInput {
    pub count_id: String,
    pub expected_revision: i64,
}

fn details(
    connection: &Connection,
    store_id: &str,
    count_id: &str,
) -> Result<StockCountDetails, String> {
    let (title, status, revision, started_at, posted_at) = connection
        .query_row(
            "SELECT title,status,revision,started_at,posted_at FROM stock_counts WHERE id=?1 AND store_id=?2",
            params![count_id, store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That stock count is not available in this store.".to_owned())?;
    let mut statement = connection
        .prepare(
            "SELECT l.product_id,p.sku,p.name,l.expected_quantity,l.counted_quantity,l.counted_at
             FROM stock_count_lines l JOIN products p ON p.id=l.product_id
             WHERE l.count_id=?1 AND p.store_id=?2 ORDER BY p.name COLLATE NOCASE,p.sku COLLATE NOCASE",
        )
        .map_err(|error| error.to_string())?;
    let lines = statement
        .query_map(params![count_id, store_id], |row| {
            Ok(StockCountLine {
                product_id: row.get(0)?,
                sku: row.get(1)?,
                product_name: row.get(2)?,
                expected_quantity: row.get(3)?,
                counted_quantity: row.get(4)?,
                counted_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(StockCountDetails {
        id: count_id.to_owned(),
        title,
        status,
        revision,
        started_at,
        posted_at,
        lines,
    })
}

#[tauri::command]
pub fn list_stock_counts(state: State<'_, AppState>) -> Result<StockCountList, String> {
    let active = active_session(&state, "inventory:write")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = connection
        .prepare(
            "SELECT c.id,c.title,c.status,c.revision,c.started_at,c.posted_at,
                    COUNT(l.product_id),SUM(CASE WHEN l.counted_quantity IS NOT NULL THEN 1 ELSE 0 END)
             FROM stock_counts c LEFT JOIN stock_count_lines l ON l.count_id=c.id
             WHERE c.store_id=?1 GROUP BY c.id ORDER BY c.started_at DESC,c.id DESC",
        )
        .map_err(|error| error.to_string())?;
    let counts = statement
        .query_map([&active.user.store_id], |row| {
            Ok(StockCountSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                revision: row.get(3)?,
                started_at: row.get(4)?,
                posted_at: row.get(5)?,
                item_count: row.get(6)?,
                counted_count: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(StockCountList { counts })
}

#[tauri::command]
pub fn get_stock_count(
    count_id: String,
    state: State<'_, AppState>,
) -> Result<StockCountDetails, String> {
    let active = active_session(&state, "inventory:write")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    details(&connection, &active.user.store_id, &count_id)
}

#[tauri::command]
pub fn start_stock_count(
    input: StartStockCountInput,
    state: State<'_, AppState>,
) -> Result<StockCountDetails, String> {
    let active = active_session(&state, "inventory:write")?;
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return Err("Enter a stock count name from 1 to 120 characters.".to_owned());
    }
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let product_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM products WHERE store_id=?1 AND is_active=1",
            [&active.user.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if product_count == 0 {
        return Err("Add active products before starting a stock count.".to_owned());
    }
    let count_id = Uuid::new_v4().to_string();
    let started_at = now();
    transaction.execute(
        "INSERT INTO stock_counts(id,business_id,store_id,title,status,revision,started_by,started_at)
         VALUES(?1,?2,?3,?4,'draft',1,?5,?6)",
        params![count_id,active.user.business_id,active.user.store_id,title,active.user.user_id,started_at],
    ).map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO stock_count_lines(count_id,product_id,expected_quantity)
         SELECT ?1,id,stock_quantity FROM products WHERE store_id=?2 AND is_active=1",
            params![count_id, active.user.store_id],
        )
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &active.user,
        "stock_count_started",
        "stock_count",
        Some(&count_id),
        serde_json::json!({"title":title,"itemCount":product_count}),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    details(&connection, &active.user.store_id, &count_id)
}

#[tauri::command]
pub fn save_stock_count_counts(
    input: SaveStockCountInput,
    state: State<'_, AppState>,
) -> Result<StockCountDetails, String> {
    let active = active_session(&state, "inventory:write")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let (status, revision): (String, i64) = transaction
        .query_row(
            "SELECT status,revision FROM stock_counts WHERE id=?1 AND store_id=?2",
            params![input.count_id, active.user.store_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That stock count is not available in this store.".to_owned())?;
    if status != "draft" {
        return Err("A posted stock count can no longer be edited.".to_owned());
    }
    if revision != input.expected_revision {
        return Err(
            "This stock count changed on another screen. Reload it before saving.".to_owned(),
        );
    }
    let expected_len: usize = transaction
        .query_row(
            "SELECT COUNT(*) FROM stock_count_lines WHERE count_id=?1",
            [&input.count_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())? as usize;
    let mut seen = std::collections::HashSet::with_capacity(input.lines.len());
    if input.lines.len() != expected_len
        || input
            .lines
            .iter()
            .any(|line| !seen.insert(line.product_id.as_str()))
    {
        return Err(
            "The count form does not match this stock count. Reload it and try again.".to_owned(),
        );
    }
    for line in &input.lines {
        if line
            .counted_quantity
            .is_some_and(|quantity| !(0..=MAX_COUNTED_QUANTITY).contains(&quantity))
        {
            return Err("Enter whole stock counts from 0 to 1 billion.".to_owned());
        }
        let changed = if let Some(quantity) = line.counted_quantity {
            let (expected_quantity, cursor): (i64, i64) = transaction.query_row(
                "SELECT p.stock_quantity,COALESCE((SELECT MAX(sequence) FROM stock_movement_events WHERE store_id=?1 AND product_id=?2),0)
                 FROM products p WHERE p.id=?2 AND p.store_id=?1",
                params![active.user.store_id,line.product_id],
                |row| Ok((row.get(0)?,row.get(1)?)),
            ).optional().map_err(|error| error.to_string())?.ok_or_else(|| "A product in this count is no longer available in this store.".to_owned())?;
            transaction.execute(
                "UPDATE stock_count_lines SET expected_quantity=?1,counted_quantity=?2,movement_cursor=?3,counted_at=?4 WHERE count_id=?5 AND product_id=?6",
                params![expected_quantity,quantity,cursor,now(),input.count_id,line.product_id],
            ).map_err(|error| error.to_string())?
        } else {
            transaction.execute(
                "UPDATE stock_count_lines SET counted_quantity=NULL,movement_cursor=NULL,counted_at=NULL WHERE count_id=?1 AND product_id=?2",
                params![input.count_id,line.product_id],
            ).map_err(|error| error.to_string())?
        };
        if changed != 1 {
            return Err("A product in this count is not part of the active count.".to_owned());
        }
    }
    let next_revision = revision
        .checked_add(1)
        .ok_or_else(|| "This stock count needs to be closed and replaced.".to_owned())?;
    transaction.execute("UPDATE stock_counts SET revision=?1 WHERE id=?2 AND store_id=?3 AND revision=?4 AND status='draft'", params![next_revision,input.count_id,active.user.store_id,revision]).map_err(|error| error.to_string())?;
    add_audit_event(
        &transaction,
        &active.user,
        "stock_count_progress_saved",
        "stock_count",
        Some(&input.count_id),
        serde_json::json!({"revision":next_revision,"countedLines":input.lines.iter().filter(|line|line.counted_quantity.is_some()).count()}),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    details(&connection, &active.user.store_id, &input.count_id)
}

#[tauri::command]
pub fn post_stock_count(
    input: PostStockCountInput,
    state: State<'_, AppState>,
) -> Result<StockCountDetails, String> {
    let active = active_session(&state, "inventory:write")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let (title, status, revision): (String, String, i64) = transaction
        .query_row(
            "SELECT title,status,revision FROM stock_counts WHERE id=?1 AND store_id=?2",
            params![input.count_id, active.user.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That stock count is not available in this store.".to_owned())?;
    if status != "draft" {
        return Err("This stock count has already been posted.".to_owned());
    }
    if revision != input.expected_revision {
        return Err(
            "This stock count changed on another screen. Reload it before posting.".to_owned(),
        );
    }
    let mut statement = transaction
        .prepare(
            "SELECT l.product_id,p.name,p.stock_quantity,l.counted_quantity,l.movement_cursor
         FROM stock_count_lines l JOIN products p ON p.id=l.product_id
         WHERE l.count_id=?1 AND p.store_id=?2 ORDER BY p.id",
        )
        .map_err(|error| error.to_string())?;
    let lines = statement
        .query_map(params![input.count_id, active.user.store_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    if lines.is_empty()
        || lines
            .iter()
            .any(|line| line.3.is_none() || line.4.is_none())
    {
        return Err("Count every listed product before posting this stock count.".to_owned());
    }
    let timestamp = now();
    let mut changes = Vec::new();
    for (product_id, name, current_quantity, counted_quantity, movement_cursor) in lines {
        let cursor = movement_cursor.expect("counted stock rows have a movement cursor");
        let counted = counted_quantity.expect("validated counted quantity");
        let later_movement: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(quantity_delta),0) FROM stock_movement_events WHERE store_id=?1 AND product_id=?2 AND sequence>?3",
            params![active.user.store_id,product_id,cursor],|row|row.get(0)
        ).map_err(|error|error.to_string())?;
        let (final_quantity, delta) = reconciled_stock(counted, later_movement, current_quantity)
            .map_err(|reason| match reason {
                StockReconciliationError::ArithmeticOverflow => {
                    format!("The stock calculation for {name} is outside the supported range.")
                }
                StockReconciliationError::NegativeResult => format!(
                    "{name} has more stock movements after it was counted than its physical count allows. Review its later sales or adjustments, save a fresh count, then try again."
                ),
                StockReconciliationError::QuantityTooLarge => {
                    format!("The calculated stock for {name} exceeds the supported quantity.")
                }
            })?;
        if delta != 0 {
            changes.push((product_id, name, current_quantity, final_quantity, delta));
        }
    }
    for (product_id, name, _old_quantity, new_quantity, delta) in &changes {
        transaction
            .execute(
                "UPDATE products SET stock_quantity=?1,updated_at=?2 WHERE id=?3 AND store_id=?4",
                params![new_quantity, timestamp, product_id, active.user.store_id],
            )
            .map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT INTO stock_movements(id,store_id,product_id,user_id,quantity_delta,reason,reference_id,note,created_at)
             VALUES(?1,?2,?3,?4,?5,'adjustment',?6,?7,?8)",
            params![Uuid::new_v4().to_string(),active.user.store_id,product_id,active.user.user_id,delta,input.count_id,format!("Stock count · {title} · {name}"),timestamp],
        ).map_err(|error|error.to_string())?;
    }
    let posted_revision = revision
        .checked_add(1)
        .ok_or_else(|| "This stock count cannot be posted at its current revision.".to_owned())?;
    let updated = transaction.execute(
        "UPDATE stock_counts SET status='posted',revision=?1,posted_at=?2 WHERE id=?3 AND store_id=?4 AND revision=?5 AND status='draft'",
        params![posted_revision,timestamp,input.count_id,active.user.store_id,revision],
    ).map_err(|error|error.to_string())?;
    if updated != 1 {
        return Err(
            "This stock count changed before it could be posted. Reload it and try again."
                .to_owned(),
        );
    }
    let adjusted_units = changes.iter().fold(0_u64, |total, change| {
        total.saturating_add(change.4.unsigned_abs())
    });
    add_audit_event(
        &transaction,
        &active.user,
        "stock_count_posted",
        "stock_count",
        Some(&input.count_id),
        serde_json::json!({"title":title,"adjustedProducts":changes.len(),"adjustedUnits":adjusted_units}),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    details(&connection, &active.user.store_id, &input.count_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciliation_preserves_movements_after_the_physical_count() {
        assert_eq!(reconciled_stock(10, -2, 7), Ok((8, 1)));
        assert_eq!(reconciled_stock(4, 3, 7), Ok((7, 0)));
    }

    #[test]
    fn reconciliation_rejects_negative_or_unbounded_results() {
        assert_eq!(
            reconciled_stock(0, -1, 0),
            Err(StockReconciliationError::NegativeResult)
        );
        assert_eq!(
            reconciled_stock(MAX_COUNTED_QUANTITY, 1, 0),
            Err(StockReconciliationError::QuantityTooLarge)
        );
    }

    #[test]
    fn stock_count_migration_backfills_and_advances_durable_movement_sequences() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
                 CREATE TABLE businesses(id TEXT PRIMARY KEY);
                 CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT REFERENCES businesses(id));
                 CREATE TABLE users(id TEXT PRIMARY KEY);
                 CREATE TABLE products(id TEXT PRIMARY KEY,store_id TEXT REFERENCES stores(id));
                 CREATE TABLE stock_movements(id TEXT PRIMARY KEY,store_id TEXT REFERENCES stores(id),
                    product_id TEXT REFERENCES products(id),user_id TEXT REFERENCES users(id),
                    quantity_delta INTEGER,reason TEXT,reference_id TEXT,note TEXT,created_at TEXT);
                 INSERT INTO businesses VALUES('business-a');
                 INSERT INTO stores VALUES('store-a','business-a');
                 INSERT INTO users VALUES('user-a');
                 INSERT INTO products VALUES('product-a','store-a');
                 INSERT INTO stock_movements VALUES('movement-a','store-a','product-a','user-a',5,
                    'receiving',NULL,'Opening stock','2026-10-30T00:00:00.000Z');",
            )
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/0035_stock_counts.sql"))
            .unwrap();
        let first_sequence: i64 = connection
            .query_row(
                "SELECT sequence FROM stock_movement_events WHERE movement_id='movement-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO stock_movements VALUES('movement-b','store-a','product-a','user-a',-1,
                    'sale','sale-a','Sale','2026-10-30T00:00:00.000Z')",
                [],
            )
            .unwrap();
        let second_sequence: i64 = connection
            .query_row(
                "SELECT sequence FROM stock_movement_events WHERE movement_id='movement-b'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(second_sequence > first_sequence);
    }
}
