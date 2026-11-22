// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::active_session, db::AppState, models::ExpenseReportRow, sales::canonical_timestamp,
};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseQueryInput {
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
    #[serde(default)]
    pub reimbursement_snapshot_row_id: Option<i64>,
    #[serde(default)]
    pub reclassification_snapshot_row_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseQueryResult {
    pub rows: Vec<ExpenseReportRow>,
    pub offset: u32,
    pub page_size: u16,
    pub matched_count: i64,
    pub total_minor: i64,
    pub snapshot_row_id: i64,
    pub reimbursement_snapshot_row_id: i64,
    pub reclassification_snapshot_row_id: i64,
}

const EXPENSE_EVENTS_CTE: &str = r#"
WITH attributed_expenses AS (
    SELECT e.rowid AS source_row_id,e.id,e.store_id,e.user_id,e.category,e.description,
           e.amount_minor,e.payment_method,e.cash_origin,e.cash_source_note,e.occurred_at,e.created_at,
           CASE WHEN rc.id IS NULL THEN e.cost_center_kind ELSE rc.next_kind END AS cost_center_kind,
           CASE WHEN rc.id IS NULL THEN e.cost_center_station_key ELSE rc.next_station_key END AS cost_center_station_key,
           CASE WHEN rc.id IS NULL THEN e.cost_center_station_name_snapshot ELSE rc.next_station_name_snapshot END AS cost_center_station_name_snapshot,
           COALESCE(rc.revision,0) AS cost_center_revision
    FROM expenses e
    LEFT JOIN expense_cost_center_reclassifications rc ON rc.store_id=e.store_id AND rc.expense_id=e.id
      AND rc.rowid=(SELECT MAX(latest.rowid) FROM expense_cost_center_reclassifications latest
                    WHERE latest.store_id=e.store_id AND latest.expense_id=e.id AND latest.rowid<=?4)
    WHERE e.store_id=?1
), events AS (
    SELECT e.source_row_id,e.id,e.id AS expense_id,'expense' AS entry_type,
           e.category,e.description,e.amount_minor,e.payment_method,e.cash_origin,e.cash_source_note,
           e.cost_center_kind,e.cost_center_station_key,e.cost_center_station_name_snapshot,e.cost_center_revision,
           e.occurred_at,e.created_at,u.full_name AS recorded_by,
           NULL AS correction_reason,NULL AS external_reference,
           CASE WHEN e.amount_minor>COALESCE((SELECT SUM(r.amount_minor) FROM expense_reimbursements r WHERE r.expense_id=e.id AND r.store_id=e.store_id),0) THEN 1 ELSE 0 END AS can_reverse,
           e.amount_minor-COALESCE((SELECT SUM(r.amount_minor) FROM expense_reimbursements r WHERE r.expense_id=e.id AND r.store_id=e.store_id),0) AS remaining_minor
    FROM attributed_expenses e JOIN users u ON u.id=e.user_id
    UNION ALL
    SELECT r.rowid AS source_row_id,r.id,r.expense_id,'reimbursement' AS entry_type,
           e.category,e.description,-r.amount_minor AS amount_minor,r.return_method,
           CASE WHEN r.return_method='cash' AND l.location_type='register' THEN 'drawer'
                WHEN r.return_method='cash' THEN 'other_cash' ELSE 'not_applicable' END AS cash_origin,
           l.name AS cash_source_note,e.cost_center_kind,e.cost_center_station_key,
           e.cost_center_station_name_snapshot,e.cost_center_revision,r.returned_at AS occurred_at,
           r.created_at,u.full_name AS recorded_by,r.reason AS correction_reason,
           r.return_reference AS external_reference,0 AS can_reverse,0 AS remaining_minor
    FROM expense_reimbursements r JOIN attributed_expenses e ON e.id=r.expense_id AND e.store_id=r.store_id
    LEFT JOIN cash_locations l ON l.id=r.cash_location_id AND l.store_id=r.store_id
    JOIN users u ON u.id=r.recorded_by WHERE r.store_id=?1
)
"#;

fn safe_integer(value: i64) -> Result<(), String> {
    if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value) {
        return Err("An expense report value exceeds this device's safe number range.".to_owned());
    }
    Ok(())
}

fn run_query(
    connection: &mut Connection,
    store_id: &str,
    input: &ExpenseQueryInput,
) -> Result<ExpenseQueryResult, String> {
    let bounds = match (&input.from_utc, &input.to_utc_exclusive) {
        (None, None) => None,
        (Some(from), Some(to)) => {
            let from = canonical_timestamp(from)?;
            let to = canonical_timestamp(to)?;
            if from >= to {
                return Err("The expense report start must be earlier than its end.".to_owned());
            }
            Some((from, to))
        }
        _ => return Err("Provide both expense report date bounds or neither.".to_owned()),
    };
    let search = input.search.as_deref().unwrap_or("").trim();
    if search.chars().count() > 200 {
        return Err("Keep the expense report search under 200 characters.".to_owned());
    }
    if input.offset > 10_000_000 {
        return Err("The expense report page is too large. Refresh the report.".to_owned());
    }
    let page_size = match input.page_size {
        0 => 50,
        size => size.min(100),
    };

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| error.to_string())?;
    let (current_high_water, current_reimbursement_high_water, current_reclassification_high_water): (i64, i64, i64) = transaction
        .query_row(
            "SELECT COALESCE((SELECT MAX(rowid) FROM expenses WHERE store_id=?1),0),
                    COALESCE((SELECT MAX(rowid) FROM expense_reimbursements WHERE store_id=?1),0),
                    COALESCE((SELECT MAX(rowid) FROM expense_cost_center_reclassifications WHERE store_id=?1),0)",
            [store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| error.to_string())?;
    let snapshot_row_id = match input.snapshot_row_id {
        Some(value) if value < 0 => {
            return Err("The expense snapshot is invalid. Refresh the report.".to_owned());
        }
        Some(value) => value.min(current_high_water),
        None => current_high_water,
    };
    let reimbursement_snapshot_row_id = match input.reimbursement_snapshot_row_id {
        Some(value) if value < 0 => {
            return Err("The expense snapshot is invalid. Refresh the report.".to_owned());
        }
        Some(value) => value.min(current_reimbursement_high_water),
        None => current_reimbursement_high_water,
    };
    let reclassification_snapshot_row_id = match input.reclassification_snapshot_row_id {
        Some(value) if value < 0 => {
            return Err("The expense snapshot is invalid. Refresh the report.".to_owned())
        }
        Some(value) => value.min(current_reclassification_high_water),
        None => current_reclassification_high_water,
    };
    let from = bounds.as_ref().map(|(from, _)| from.as_str());
    let to = bounds.as_ref().map(|(_, to)| to.as_str());
    let (matched_count, total_minor): (i64, i64) = transaction
        .query_row(
            &format!("{EXPENSE_EVENTS_CTE}
             SELECT COUNT(*),COALESCE(SUM(amount_minor),0) FROM events
             WHERE source_row_id<=CASE entry_type WHEN 'expense' THEN ?2 ELSE ?3 END
               AND (?5 IS NULL OR occurred_at>=?5)
               AND (?6 IS NULL OR occurred_at<?6)
               AND (?7='' OR instr(lower(category||' '||COALESCE(description,'')||' '||entry_type||' '||payment_method||' '||cash_origin||' '||COALESCE(cash_source_note,'')||' '||COALESCE(correction_reason,'')||' '||COALESCE(external_reference,'')||' '||recorded_by),lower(?7))>0)"),
            params![store_id, snapshot_row_id, reimbursement_snapshot_row_id, reclassification_snapshot_row_id, from, to, search],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    safe_integer(matched_count)?;
    safe_integer(total_minor)?;
    let mut statement = transaction
        .prepare(&format!(
            "{EXPENSE_EVENTS_CTE}
             SELECT id,expense_id,entry_type,category,description,amount_minor,payment_method,cash_origin,
                    cash_source_note,cost_center_kind,cost_center_station_key,cost_center_station_name_snapshot,
                    cost_center_revision,occurred_at,created_at,recorded_by,correction_reason,external_reference,can_reverse,remaining_minor
             FROM events
             WHERE source_row_id<=CASE entry_type WHEN 'expense' THEN ?2 ELSE ?3 END
               AND (?5 IS NULL OR occurred_at>=?5)
               AND (?6 IS NULL OR occurred_at<?6)
               AND (?7='' OR instr(lower(category||' '||COALESCE(description,'')||' '||entry_type||' '||payment_method||' '||cash_origin||' '||COALESCE(cash_source_note,'')||' '||COALESCE(correction_reason,'')||' '||COALESCE(external_reference,'')||' '||recorded_by),lower(?7))>0)
             ORDER BY occurred_at DESC,created_at DESC,id DESC LIMIT ?8 OFFSET ?9"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                store_id,
                snapshot_row_id,
                reimbursement_snapshot_row_id,
                reclassification_snapshot_row_id,
                from,
                to,
                search,
                page_size,
                input.offset
            ],
            |row| {
                Ok(ExpenseReportRow {
                    id: row.get(0)?,
                    expense_id: row.get(1)?,
                    entry_type: row.get(2)?,
                    category: row.get(3)?,
                    description: row.get(4)?,
                    amount_minor: row.get(5)?,
                    payment_method: row.get(6)?,
                    cash_origin: row.get(7)?,
                    cash_source_note: row.get(8)?,
                    cost_center_kind: row.get(9)?,
                    cost_center_station_key: row.get(10)?,
                    cost_center_station_name_snapshot: row.get(11)?,
                    cost_center_revision: row.get(12)?,
                    occurred_at: row.get(13)?,
                    created_at: row.get(14)?,
                    recorded_by: row.get(15)?,
                    correction_reason: row.get(16)?,
                    external_reference: row.get(17)?,
                    can_reverse: row.get::<_, i64>(18)? != 0,
                    remaining_minor: row.get(19)?,
                })
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(ExpenseQueryResult {
        rows,
        offset: input.offset,
        page_size,
        matched_count,
        total_minor,
        snapshot_row_id,
        reimbursement_snapshot_row_id,
        reclassification_snapshot_row_id,
    })
}

#[tauri::command]
pub fn query_expenses(
    input: ExpenseQueryInput,
    state: State<'_, AppState>,
) -> Result<ExpenseQueryResult, String> {
    let session = active_session(&state, "expenses:read")?;
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
             CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,user_id TEXT,category TEXT,description TEXT,amount_minor INTEGER,payment_method TEXT,occurred_at TEXT,created_at TEXT,cash_origin TEXT NOT NULL DEFAULT 'legacy_unknown',cash_source_note TEXT,cost_center_kind TEXT NOT NULL DEFAULT 'legacy_unassigned',cost_center_station_key TEXT,cost_center_station_name_snapshot TEXT);
             CREATE TABLE expense_reimbursements(id TEXT PRIMARY KEY,store_id TEXT,expense_id TEXT,amount_minor INTEGER,return_method TEXT,return_reference TEXT,reason TEXT,returned_at TEXT,created_at TEXT,recorded_by TEXT,cash_location_id TEXT);
             CREATE TABLE cash_locations(id TEXT,store_id TEXT,location_type TEXT,name TEXT);
             CREATE TABLE expense_cost_center_reclassifications(id TEXT,store_id TEXT,expense_id TEXT,revision INTEGER,next_kind TEXT,next_station_key TEXT,next_station_name_snapshot TEXT);
             INSERT INTO users VALUES('u1','Cashier One'),('u2','Cashier Two');
             INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at,cash_origin) VALUES
               ('e1','s1','u1','Utilities','Power',100,'cash','2026-10-01T10:00:00.000Z','2026-10-01T10:00:00.000Z','legacy_unknown'),
               ('e2','s1','u2','Rent','Monthly',200,'bank_transfer','2026-10-02T10:00:00.000Z','2026-10-02T10:00:00.000Z','not_applicable'),
               ('e3','s1','u1','Utilities','Water',300,'cash','2026-10-03T10:00:00.000Z','2026-10-03T10:00:00.000Z','legacy_unknown'),
               ('e4','s2','u2','Utilities','Other store',900,'cash','2026-10-03T10:00:00.000Z','2026-10-03T10:00:00.000Z','legacy_unknown');",
        )
        .unwrap();
        db
    }

    #[test]
    fn query_scopes_store_filters_and_keeps_pages_at_one_snapshot() {
        let mut db = fixture();
        let input = ExpenseQueryInput {
            from_utc: Some("2026-10-01T00:00:00Z".into()),
            to_utc_exclusive: Some("2026-10-04T00:00:00Z".into()),
            search: Some("utilities".into()),
            offset: 0,
            page_size: 1,
            snapshot_row_id: None,
            reimbursement_snapshot_row_id: None,
            reclassification_snapshot_row_id: None,
        };
        let first = run_query(&mut db, "s1", &input).unwrap();
        assert_eq!(first.matched_count, 2);
        assert_eq!(first.total_minor, 400);
        assert_eq!(first.rows[0].id, "e3");
        db.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at) VALUES('e5','s1','u1','Utilities','Later',50,'cash','2026-10-03T11:00:00.000Z','2026-10-03T11:00:00.000Z')",
            [],
        ).unwrap();
        let next = run_query(
            &mut db,
            "s1",
            &ExpenseQueryInput {
                offset: 1,
                snapshot_row_id: Some(first.snapshot_row_id),
                reimbursement_snapshot_row_id: Some(first.reimbursement_snapshot_row_id),
                reclassification_snapshot_row_id: Some(first.reclassification_snapshot_row_id),
                ..input.clone()
            },
        )
        .unwrap();
        assert_eq!(next.matched_count, 2);
        assert_eq!(next.rows[0].id, "e1");
        db.execute(
            "INSERT INTO expense_cost_center_reclassifications VALUES('rc-e1','s1','e1',1,'station_direct','station-a','Main')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO expense_reimbursements(id,store_id,expense_id,amount_minor,return_method,return_reference,reason,returned_at,created_at,recorded_by)
             VALUES('r-e1','s1','e1',20,'bank_transfer','BANK-RETURN-E1','Part of the expense was returned','2026-10-03T12:00:00.000Z','2026-10-03T12:00:00.000Z','u1')",
            [],
        )
        .unwrap();
        let stable_reclass = run_query(
            &mut db,
            "s1",
            &ExpenseQueryInput {
                reclassification_snapshot_row_id: Some(first.reclassification_snapshot_row_id),
                ..ExpenseQueryInput {
                    offset: 1,
                    snapshot_row_id: Some(first.snapshot_row_id),
                    reimbursement_snapshot_row_id: Some(first.reimbursement_snapshot_row_id),
                    ..input.clone()
                }
            },
        )
        .unwrap();
        assert_eq!(stable_reclass.rows[0].cost_center_kind, "legacy_unassigned");
        let refreshed = run_query(
            &mut db,
            "s1",
            &ExpenseQueryInput {
                search: Some("power".into()),
                offset: 0,
                page_size: 10,
                ..input.clone()
            },
        )
        .unwrap();
        assert_eq!(refreshed.rows[0].cost_center_kind, "station_direct");
        assert_eq!(refreshed.rows[0].cost_center_revision, 1);
        assert_eq!(refreshed.matched_count, 2);
        assert!(refreshed.rows.iter().all(|row| {
            row.expense_id == "e1"
                && row.cost_center_kind == "station_direct"
                && row.cost_center_station_key.as_deref() == Some("station-a")
                && row.cost_center_revision == 1
        }));
    }

    #[test]
    fn query_rejects_partial_or_invalid_periods() {
        let mut db = fixture();
        let partial = ExpenseQueryInput {
            from_utc: Some("2026-10-01T00:00:00Z".into()),
            to_utc_exclusive: None,
            search: None,
            offset: 0,
            page_size: 50,
            snapshot_row_id: None,
            reimbursement_snapshot_row_id: None,
            reclassification_snapshot_row_id: None,
        };
        assert!(run_query(&mut db, "s1", &partial)
            .unwrap_err()
            .contains("both"));
    }

    #[test]
    fn query_includes_signed_reimbursements_and_keeps_their_page_snapshot_stable() {
        let mut db = fixture();
        let input = ExpenseQueryInput {
            from_utc: Some("2026-10-01T00:00:00Z".into()),
            to_utc_exclusive: Some("2026-10-04T00:00:00Z".into()),
            search: Some("monthly".into()),
            offset: 0,
            page_size: 10,
            snapshot_row_id: None,
            reimbursement_snapshot_row_id: None,
            reclassification_snapshot_row_id: None,
        };
        db.execute(
            "INSERT INTO expense_reimbursements(id,store_id,expense_id,amount_minor,return_method,return_reference,reason,returned_at,created_at,recorded_by)
             VALUES('r1','s1','e2',200,'bank_transfer','BANK-RETURN-1','Duplicate payment returned','2026-10-03T11:00:00.000Z','2026-10-03T11:00:00.000Z','u1')",
            [],
        ).unwrap();
        let page = run_query(&mut db, "s1", &input).unwrap();
        assert_eq!(page.matched_count, 2);
        assert_eq!(page.total_minor, 0);
        assert_eq!(page.rows[0].id, "r1");
        assert_eq!(page.rows[0].entry_type, "reimbursement");
        assert_eq!(page.rows[0].amount_minor, -200);
        assert_eq!(
            page.rows[0].correction_reason.as_deref(),
            Some("Duplicate payment returned")
        );
        assert!(!page.rows[0].can_reverse);
        assert_eq!(page.rows[1].id, "e2");
        assert!(!page.rows[1].can_reverse);

        let mut later = input;
        later.snapshot_row_id = Some(page.snapshot_row_id);
        later.reimbursement_snapshot_row_id = Some(page.reimbursement_snapshot_row_id);
        later.reclassification_snapshot_row_id = Some(page.reclassification_snapshot_row_id);
        db.execute(
            "INSERT INTO expense_reimbursements(id,store_id,expense_id,amount_minor,return_method,return_reference,reason,returned_at,created_at,recorded_by)
             VALUES('r2','s1','e3',1,'bank_transfer','BANK-RETURN-2','Later correction','2026-10-03T12:00:00.000Z','2026-10-03T12:00:00.000Z','u1')",
            [],
        ).unwrap();
        let stable = run_query(&mut db, "s1", &later).unwrap();
        assert_eq!(stable.matched_count, 2);
        assert_eq!(stable.total_minor, 0);
        assert_eq!(
            stable.reimbursement_snapshot_row_id,
            page.reimbursement_snapshot_row_id
        );
        assert_eq!(
            stable.reclassification_snapshot_row_id,
            page.reclassification_snapshot_row_id
        );
    }
}
