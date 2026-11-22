// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::active_session, db::AppState, models::PurchaseSummary, sales::canonical_timestamp,
};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseQueryInput {
    #[serde(default)]
    pub from_utc: Option<String>,
    #[serde(default)]
    pub to_utc_exclusive: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub page_size: u16,
    #[serde(default)]
    pub snapshot_row_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseQueryResult {
    pub rows: Vec<PurchaseSummary>,
    pub offset: u32,
    pub page_size: u16,
    pub matched_count: i64,
    pub total_minor: i64,
    pub snapshot_row_id: i64,
}

const MATCHED_PURCHASES: &str = "SELECT p.rowid AS snapshot_key,p.id,p.reference,s.name AS supplier_name,p.received_at,p.total_minor,
    COALESCE(SUM(pi.quantity),0) AS item_count,COALESCE(GROUP_CONCAT(pi.product_name || ' × ' || pi.quantity, ', '),'') AS items_summary,u.full_name AS recorded_by
    FROM purchases p LEFT JOIN suppliers s ON s.id=p.supplier_id JOIN users u ON u.id=p.user_id
    LEFT JOIN purchase_items pi ON pi.purchase_id=p.id
    WHERE p.store_id=?1 AND p.rowid<=?2 AND (?3 IS NULL OR p.received_at>=?3) AND (?4 IS NULL OR p.received_at<?4)
    GROUP BY p.id
    HAVING ?5='' OR instr(lower(COALESCE(p.reference,'') || ' ' || COALESCE(s.name,'') || ' ' ||
      COALESCE(GROUP_CONCAT(pi.product_name || ' × ' || pi.quantity, ', '),'') || ' ' || u.full_name),lower(?5))>0";

fn safe_integer(value: i64) -> Result<(), String> {
    if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value) {
        return Err("A purchase report value exceeds this device's safe number range.".to_owned());
    }
    Ok(())
}

fn run_query(
    connection: &mut Connection,
    store_id: &str,
    input: &PurchaseQueryInput,
) -> Result<PurchaseQueryResult, String> {
    let bounds = match (&input.from_utc, &input.to_utc_exclusive) {
        (None, None) => None,
        (Some(from), Some(to)) => {
            let from = canonical_timestamp(from)?;
            let to = canonical_timestamp(to)?;
            if from >= to {
                return Err("The purchase report start must be earlier than its end.".to_owned());
            }
            Some((from, to))
        }
        _ => return Err("Provide both purchase report date bounds or neither.".to_owned()),
    };
    let search = input.search.as_deref().unwrap_or("").trim();
    if search.chars().count() > 200 {
        return Err("Keep the purchase report search under 200 characters.".to_owned());
    }
    if input.offset > 10_000_000 {
        return Err("The purchase report page is too large. Refresh the report.".to_owned());
    }
    let page_size = match input.page_size {
        0 => 50,
        size => size.min(100),
    };

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| error.to_string())?;
    let current_high_water: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM purchases WHERE store_id=?1",
            [store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let snapshot_row_id = match input.snapshot_row_id {
        Some(value) if value < 0 => {
            return Err("The purchase snapshot is invalid. Refresh the report.".to_owned());
        }
        Some(value) => value.min(current_high_water),
        None => current_high_water,
    };
    let from = bounds.as_ref().map(|(from, _)| from.as_str());
    let to = bounds.as_ref().map(|(_, to)| to.as_str());
    let aggregate_sql = format!(
        "SELECT COUNT(*),COALESCE(SUM(total_minor),0) FROM ({MATCHED_PURCHASES}) AS matched"
    );
    let (matched_count, total_minor): (i64, i64) = transaction
        .query_row(
            &aggregate_sql,
            params![store_id, snapshot_row_id, from, to, search],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    safe_integer(matched_count)?;
    safe_integer(total_minor)?;

    let rows_sql = format!(
        "SELECT id,reference,supplier_name,received_at,total_minor,item_count,items_summary,recorded_by
       FROM ({MATCHED_PURCHASES}) AS matched
       ORDER BY received_at DESC,id DESC LIMIT ?6 OFFSET ?7"
    );
    let mut statement = transaction
        .prepare(&rows_sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                store_id,
                snapshot_row_id,
                from,
                to,
                search,
                page_size,
                input.offset
            ],
            |row| {
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
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(PurchaseQueryResult {
        rows,
        offset: input.offset,
        page_size,
        matched_count,
        total_minor,
        snapshot_row_id,
    })
}

#[tauri::command]
pub fn query_purchases(
    input: PurchaseQueryInput,
    state: State<'_, AppState>,
) -> Result<PurchaseQueryResult, String> {
    let session = active_session(&state, "purchases:read")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    run_query(&mut connection, &session.store_id, &input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE users(id TEXT PRIMARY KEY,full_name TEXT);
             CREATE TABLE suppliers(id TEXT PRIMARY KEY,name TEXT);
             CREATE TABLE purchases(id TEXT PRIMARY KEY,store_id TEXT,supplier_id TEXT,user_id TEXT,reference TEXT,total_minor INTEGER,received_at TEXT,created_at TEXT);
             CREATE TABLE purchase_items(id TEXT PRIMARY KEY,purchase_id TEXT,product_name TEXT,quantity INTEGER);
             INSERT INTO users VALUES('u1','Cashier One'),('u2','Cashier Two');
             INSERT INTO suppliers VALUES('sp1','North Supply');
             INSERT INTO purchases VALUES
               ('p1','s1','sp1','u1','INV-1',100,'2026-10-01T10:00:00.000Z','2026-10-01T10:00:00.000Z'),
               ('p2','s1',NULL,'u2','INV-2',200,'2026-10-02T10:00:00.000Z','2026-10-02T10:00:00.000Z'),
               ('p3','s1','sp1','u1','INV-3',300,'2026-10-03T10:00:00.000Z','2026-10-03T10:00:00.000Z'),
               ('p4','s2','sp1','u2','INV-4',900,'2026-10-03T10:00:00.000Z','2026-10-03T10:00:00.000Z');
             INSERT INTO purchase_items VALUES
               ('i1','p1','Flour',2),('i2','p2','Rice',3),('i3','p3','Flour',4),('i4','p4','Other store',9);",
        )
        .unwrap();
        db
    }

    #[test]
    fn query_scopes_store_filters_items_and_keeps_pages_at_one_snapshot() {
        let mut db = fixture();
        let input = PurchaseQueryInput {
            from_utc: Some("2026-10-01T00:00:00Z".into()),
            to_utc_exclusive: Some("2026-10-04T00:00:00Z".into()),
            search: Some("flour".into()),
            offset: 0,
            page_size: 1,
            snapshot_row_id: None,
        };
        let first = run_query(&mut db, "s1", &input).unwrap();
        assert_eq!(first.matched_count, 2);
        assert_eq!(first.total_minor, 400);
        assert_eq!(first.rows[0].id, "p3");
        db.execute_batch(
            "INSERT INTO purchases VALUES('p5','s1','sp1','u1','INV-5',50,'2026-10-03T11:00:00.000Z','2026-10-03T11:00:00.000Z');
             INSERT INTO purchase_items VALUES('i5','p5','Flour',1);",
        )
        .unwrap();
        let next = run_query(
            &mut db,
            "s1",
            &PurchaseQueryInput {
                offset: 1,
                snapshot_row_id: Some(first.snapshot_row_id),
                ..input
            },
        )
        .unwrap();
        assert_eq!(next.matched_count, 2);
        assert_eq!(next.rows[0].id, "p1");
    }

    #[test]
    fn query_rejects_partial_periods_and_caps_page_size() {
        let mut db = fixture();
        let partial = PurchaseQueryInput {
            from_utc: Some("2026-10-01T00:00:00Z".into()),
            to_utc_exclusive: None,
            search: None,
            offset: 0,
            page_size: 500,
            snapshot_row_id: None,
        };
        assert!(run_query(&mut db, "s1", &partial)
            .unwrap_err()
            .contains("both"));
        let valid = PurchaseQueryInput {
            from_utc: None,
            ..partial
        };
        assert_eq!(run_query(&mut db, "s1", &valid).unwrap().page_size, 100);
    }
}
