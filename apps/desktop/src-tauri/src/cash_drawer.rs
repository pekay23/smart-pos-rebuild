// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event, require_pos_transaction},
    db::AppState,
    models::SessionUser,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerShift {
    pub id: String,
    pub station_key: String,
    pub station_name: String,
    pub cashier_name: String,
    pub opened_at: String,
    pub opening_cash_minor: i64,
    pub cash_sales_minor: i64,
    pub paid_in_minor: i64,
    pub paid_out_minor: i64,
    pub cash_drops_minor: i64,
    pub expected_cash_minor: i64,
    pub movement_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerReport {
    pub shift_id: String,
    pub station_key: String,
    #[serde(default)]
    pub station_name: String,
    pub cashier_name: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub opening_cash_minor: i64,
    pub cash_sales_minor: i64,
    pub paid_in_minor: i64,
    #[serde(default)]
    pub expense_reimbursement_paid_in_minor: i64,
    pub paid_out_minor: i64,
    #[serde(default)]
    pub expense_paid_out_minor: i64,
    #[serde(default)]
    pub supplier_payment_paid_out_minor: i64,
    pub cash_drops_minor: i64,
    pub expected_cash_minor: i64,
    pub counted_cash_minor: Option<i64>,
    pub difference_minor: Option<i64>,
    #[serde(default)]
    pub closing_transfer_id: Option<String>,
    #[serde(default)]
    pub closing_transfer_minor: i64,
    #[serde(default)]
    pub retained_cash_minor: i64,
    pub movement_count: i64,
    pub completed_sale_count: i64,
    pub is_closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerHistoryPage {
    pub rows: Vec<DrawerReport>,
    pub matched_count: i64,
    pub offset: i64,
    pub snapshot_row_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerHistoryQuery {
    pub offset: i64,
    pub page_size: i64,
    pub snapshot_row_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerAmountInput {
    pub amount_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerMovementInput {
    pub movement_type: String,
    pub amount_minor: i64,
    pub note: String,
    pub other_location_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseDrawerInput {
    pub shift_id: String,
    pub counted_cash_minor: i64,
    pub closing_destination_location_id: Option<String>,
    pub retained_cash_minor: Option<i64>,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn open_shift_id(
    connection: &Connection,
    store_id: &str,
    station_key: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT id FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open'",
            params![store_id, station_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub(crate) fn record_source_paid_out(
    connection: &Connection,
    device_id: &str,
    session: &SessionUser,
    source_type: &str,
    source_id: &str,
    amount_minor: i64,
    note: &str,
    timestamp: &str,
) -> Result<(), String> {
    if !matches!(source_type, "expense" | "supplier_payment") || amount_minor <= 0 {
        return Err("The drawer cash source is invalid.".to_owned());
    }
    let station_key = crate::stations::station_key_for_device(connection, device_id, session)?;
    let shift_id =
        open_shift_id(connection, &session.store_id, &station_key)?.ok_or_else(|| {
            "Open this register drawer before recording cash paid from it.".to_owned()
        })?;
    connection
        .execute(
            "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at,source_type,source_id)
             VALUES(?1,?2,?3,?4,'paid_out',?5,?6,?7,?8,?9)",
            params![Uuid::new_v4().to_string(),shift_id,session.store_id,session.user_id,amount_minor,note,timestamp,source_type,source_id],
        )
        .map_err(|error| format!("Could not link the cash payment to the open drawer: {error}"))?;
    Ok(())
}

pub(crate) fn record_expense_reimbursement_paid_in(
    connection: &Connection,
    session: &SessionUser,
    reimbursement_id: &str,
    shift_id: &str,
    amount_minor: i64,
    note: &str,
    timestamp: &str,
) -> Result<(), String> {
    if amount_minor <= 0 {
        return Err("Cash reimbursement must be greater than zero.".to_owned());
    }
    connection
        .execute(
            "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at,source_type,source_id)
             VALUES(?1,?2,?3,?4,'paid_in',?5,?6,?7,'expense_reimbursement',?8)",
            params![Uuid::new_v4().to_string(),shift_id,session.store_id,session.user_id,amount_minor,note,timestamp,reimbursement_id],
        )
        .map_err(|error| format!("Could not link returned cash to the open drawer: {error}"))?;
    Ok(())
}

fn station_name_for_key(
    connection: &Connection,
    store_id: &str,
    station_key: &str,
) -> Result<String, String> {
    connection
        .query_row(
            "SELECT display_name FROM register_stations WHERE store_id=?1 AND station_key=?2 AND is_active=1",
            params![store_id, station_key],
            |row| row.get(0),
        )
        .map_err(|error| format!("The assigned register station is unavailable: {error}"))
}

fn report_for_shift(
    connection: &Connection,
    store_id: &str,
    shift_id: &str,
) -> Result<DrawerReport, String> {
    let shift = connection
        .query_row(
            "SELECT cs.id, cs.station_key, cs.station_name_snapshot, u.full_name, cs.opened_at, cs.closed_at,
                    cs.opening_cash_minor, cs.status, cs.closing_cash_minor
             FROM cash_shifts cs JOIN users u ON u.id=cs.user_id
             WHERE cs.id=?1 AND cs.store_id=?2",
            params![shift_id, store_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let (
        _,
        station_key,
        station_name,
        cashier_name,
        opened_at,
        closed_at,
        opening,
        status,
        stored_count,
    ) = shift;
    if status == "closed" {
        let saved: Option<String> = connection
            .query_row(
                "SELECT report_json FROM cash_close_reports WHERE shift_id=?1 AND close_type='z'",
                [shift_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(saved) = saved {
            let mut report: DrawerReport = serde_json::from_str(&saved)
                .map_err(|error| format!("Saved Z report could not be read: {error}"))?;
            if report.station_name.is_empty() {
                report.station_name = station_name;
            }
            return Ok(report);
        }
    }
    let cash_sales: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(p.amount_minor),0) FROM payments p JOIN sales s ON s.id=p.sale_id
             WHERE s.cash_shift_id=?1 AND s.store_id=?2 AND p.method='cash'",
            params![shift_id, store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let movements: (i64, i64, i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT COALESCE(SUM(CASE WHEN movement_type='paid_in' THEN amount_minor ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN movement_type='paid_out' THEN amount_minor ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN movement_type='cash_drop' THEN amount_minor ELSE 0 END),0),
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN source_type='expense' THEN amount_minor ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN source_type='supplier_payment' THEN amount_minor ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN source_type='expense_reimbursement' THEN amount_minor ELSE 0 END),0)
             FROM cash_movements WHERE shift_id=?1 AND store_id=?2",
            params![shift_id, store_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let (
        paid_in,
        paid_out,
        cash_drops,
        movement_count,
        expense_paid_out,
        supplier_payment_paid_out,
        expense_reimbursement_paid_in,
    ) = movements;
    let expected = opening
        .checked_add(cash_sales)
        .and_then(|value| value.checked_add(paid_in))
        .and_then(|value| value.checked_sub(paid_out))
        .and_then(|value| value.checked_sub(cash_drops))
        .ok_or_else(|| "Drawer total exceeded the supported amount range.".to_owned())?;
    let sale_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sales WHERE cash_shift_id=?1 AND store_id=?2 AND status IN ('completed','partially_refunded')",
            params![shift_id, store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let is_closed = status == "closed";
    let counted_cash = if is_closed { stored_count } else { None };
    Ok(DrawerReport {
        shift_id: shift_id.to_owned(),
        station_key,
        station_name,
        cashier_name,
        opened_at,
        closed_at,
        opening_cash_minor: opening,
        cash_sales_minor: cash_sales,
        paid_in_minor: paid_in,
        expense_reimbursement_paid_in_minor: expense_reimbursement_paid_in,
        paid_out_minor: paid_out,
        expense_paid_out_minor: expense_paid_out,
        supplier_payment_paid_out_minor: supplier_payment_paid_out,
        cash_drops_minor: cash_drops,
        expected_cash_minor: expected,
        counted_cash_minor: counted_cash,
        difference_minor: counted_cash.map(|count| count - expected),
        closing_transfer_id: None,
        closing_transfer_minor: 0,
        retained_cash_minor: counted_cash.unwrap_or_default(),
        movement_count,
        completed_sale_count: sale_count,
        is_closed,
    })
}

#[tauri::command]
pub fn get_drawer_workspace(state: State<'_, AppState>) -> Result<Option<DrawerShift>, String> {
    let session = active_session(&state, "pos:use")?;
    let connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    let station_key =
        crate::stations::station_key_for_device(&connection, &state.device_id, &session)?;
    let Some(id) = open_shift_id(&connection, &session.store_id, &station_key)? else {
        return Ok(None);
    };
    let report = report_for_shift(&connection, &session.store_id, &id)?;
    Ok(Some(DrawerShift {
        id: report.shift_id,
        station_key: report.station_key,
        station_name: report.station_name,
        cashier_name: report.cashier_name,
        opened_at: report.opened_at,
        opening_cash_minor: report.opening_cash_minor,
        cash_sales_minor: report.cash_sales_minor,
        paid_in_minor: report.paid_in_minor,
        paid_out_minor: report.paid_out_minor,
        cash_drops_minor: report.cash_drops_minor,
        expected_cash_minor: report.expected_cash_minor,
        movement_count: report.movement_count,
    }))
}

fn closed_history_page(
    db: &mut Connection,
    store_id: &str,
    user_id: &str,
    can_view_store: bool,
    input: DrawerHistoryQuery,
) -> Result<DrawerHistoryPage, String> {
    if input.offset < 0 || !(1..=100).contains(&input.page_size) {
        return Err("Drawer history page is outside the supported range.".to_owned());
    }
    if input.snapshot_row_id.is_some_and(|row_id| row_id < 0) {
        return Err("Drawer history snapshot is invalid.".to_owned());
    }
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| error.to_string())?;
    let snapshot_row_id = if let Some(row_id) = input.snapshot_row_id {
        row_id
    } else if can_view_store {
        tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM cash_shifts WHERE store_id=?1 AND status='closed'",
            [store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?
    } else {
        tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM cash_shifts WHERE store_id=?1 AND user_id=?2 AND status='closed'",
            params![store_id, user_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?
    };
    let (matched_count, ids) = if can_view_store {
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM cash_shifts WHERE store_id=?1 AND status='closed' AND rowid<=?2",
            params![store_id, snapshot_row_id],
            |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let mut statement = tx.prepare(
            "SELECT id FROM cash_shifts WHERE store_id=?1 AND status='closed' AND rowid<=?2 ORDER BY closed_at DESC, id DESC LIMIT ?3 OFFSET ?4",
        ).map_err(|error| error.to_string())?;
        let ids = statement
            .query_map(
                params![store_id, snapshot_row_id, input.page_size, input.offset],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        (count, ids)
    } else {
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM cash_shifts WHERE store_id=?1 AND user_id=?2 AND status='closed' AND rowid<=?3",
            params![store_id, user_id, snapshot_row_id],
            |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let mut statement = tx.prepare(
            "SELECT id FROM cash_shifts WHERE store_id=?1 AND user_id=?2 AND status='closed' AND rowid<=?3 ORDER BY closed_at DESC, id DESC LIMIT ?4 OFFSET ?5",
        ).map_err(|error| error.to_string())?;
        let ids = statement
            .query_map(
                params![
                    store_id,
                    user_id,
                    snapshot_row_id,
                    input.page_size,
                    input.offset
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        (count, ids)
    };
    let rows = ids
        .iter()
        .map(|id| report_for_shift(&tx, store_id, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DrawerHistoryPage {
        rows,
        matched_count,
        offset: input.offset,
        snapshot_row_id,
    })
}

#[tauri::command]
pub fn get_closed_drawer_history(
    input: DrawerHistoryQuery,
    state: State<'_, AppState>,
) -> Result<DrawerHistoryPage, String> {
    let session = active_session(&state, "pos:use")?;
    let can_view_store = session
        .permissions
        .iter()
        .any(|permission| permission == "reports:read");
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    closed_history_page(
        &mut db,
        &session.store_id,
        &session.user_id,
        can_view_store,
        input,
    )
}

#[tauri::command]
pub fn open_drawer(
    input: DrawerAmountInput,
    state: State<'_, AppState>,
) -> Result<DrawerShift, String> {
    let session = active_session(&state, "pos:use")?;
    require_pos_transaction(&session)?;
    if input.amount_minor < 0 {
        return Err("Opening cash cannot be negative.".to_owned());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let station_key = crate::stations::station_key_for_device(&tx, &state.device_id, &session)?;
    let station_name = station_name_for_key(&tx, &session.store_id, &station_key)?;
    if open_shift_id(&tx, &session.store_id, &station_key)?.is_some() {
        return Err("The register already has an open drawer session.".to_owned());
    }
    let cash_location_id =
        crate::cash_locations::location_for_station(&tx, &session.store_id, &station_key)?
            .ok_or_else(|| {
                "Set up a cash location for this register before opening its drawer.".to_owned()
            })?;
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    crate::cash_locations::initialize_register_opening_from_drawer(
        &tx,
        &session,
        &cash_location_id,
        input.amount_minor,
        &timestamp,
    )?;
    let tracked_before = crate::cash_locations::location_balance_minor(&tx, &cash_location_id)?;
    tx.execute(
        "INSERT INTO cash_shifts(id,store_id,user_id,opening_cash_minor,opened_at,status,station_key,station_name_snapshot,cash_location_id)
         VALUES(?1,?2,?3,?4,?5,'open',?6,?7,?8)",
        params![id, session.store_id, session.user_id, input.amount_minor, timestamp, station_key, station_name, cash_location_id],
    )
    .map_err(|error| format!("Could not open the drawer: {error}"))?;
    crate::cash_locations::record_shift_open_variance(
        &tx,
        &session,
        &cash_location_id,
        &id,
        input.amount_minor,
        tracked_before,
        &timestamp,
    )?;
    add_audit_event(
        &tx,
        &session,
        "cash_drawer_opened",
        "cash_shift",
        Some(&id),
        serde_json::json!({"openingCashMinor":input.amount_minor,"stationKey":station_key,"stationName":station_name}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    let report = report_for_shift(&db, &session.store_id, &id)?;
    Ok(DrawerShift {
        id: report.shift_id,
        station_key: report.station_key,
        station_name: report.station_name,
        cashier_name: report.cashier_name,
        opened_at: report.opened_at,
        opening_cash_minor: report.opening_cash_minor,
        cash_sales_minor: report.cash_sales_minor,
        paid_in_minor: report.paid_in_minor,
        paid_out_minor: report.paid_out_minor,
        cash_drops_minor: report.cash_drops_minor,
        expected_cash_minor: report.expected_cash_minor,
        movement_count: report.movement_count,
    })
}

#[tauri::command]
pub fn record_drawer_movement(
    input: DrawerMovementInput,
    state: State<'_, AppState>,
) -> Result<DrawerShift, String> {
    let session = active_session(&state, "pos:use")?;
    require_pos_transaction(&session)?;
    let note = input.note.trim();
    if !matches!(
        input.movement_type.as_str(),
        "paid_in" | "paid_out" | "cash_drop"
    ) {
        return Err("Choose cash in, cash paid out, or a cash drop.".to_owned());
    }
    if input.amount_minor <= 0 || input.amount_minor > 9_000_000_000_000 {
        return Err("Enter an amount greater than zero and within the supported range.".to_owned());
    }
    if note.is_empty() || note.chars().count() > 200 {
        return Err("Add a short note of up to 200 characters.".to_owned());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let station_key = crate::stations::station_key_for_device(&tx, &state.device_id, &session)?;
    let shift_id = open_shift_id(&tx, &session.store_id, &station_key)?
        .ok_or_else(|| "Open the drawer before recording a cash movement.".to_owned())?;
    let shift_location: Option<String> = tx
        .query_row(
            "SELECT cash_location_id FROM cash_shifts WHERE id=?1 AND store_id=?2",
            params![shift_id, session.store_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let paired_location = if matches!(input.movement_type.as_str(), "paid_in" | "cash_drop") {
        let other_id = input.other_location_id.as_deref().ok_or_else(|| {
            "Choose the safe or petty cash location paired with this register movement.".to_owned()
        })?;
        let valid:bool=tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_locations WHERE id=?1 AND store_id=?2 AND business_id=?3 AND is_active=1 AND location_type<>'register' AND EXISTS(SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=cash_locations.id AND e.event_type='opening'))",
            params![other_id,session.store_id,session.business_id],|row|row.get(0),
        ).map_err(|error|error.to_string())?;
        if !valid {
            return Err("Choose an active, counted safe or petty cash location.".to_owned());
        }
        Some(other_id.to_owned())
    } else {
        None
    };
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    tx.execute(
        "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, shift_id, session.store_id, session.user_id, input.movement_type, input.amount_minor, note, timestamp],
    )
    .map_err(|error| error.to_string())?;
    if let Some(register_location_id) = shift_location.as_deref() {
        match input.movement_type.as_str() {
            "paid_in" => {
                let other_id = paired_location
                    .as_deref()
                    .expect("validated paired cash location");
                let (transfer_id, _) = crate::cash_locations::transfer_between_locations(
                    &tx,
                    &session,
                    other_id,
                    register_location_id,
                    input.amount_minor,
                    note,
                    &format!("movement:{}", id),
                    "drawer_movement",
                    &id,
                    Some(&shift_id),
                    &timestamp,
                )?;
                tx.execute(
                    "UPDATE cash_movements SET transfer_id=?1 WHERE id=?2 AND store_id=?3",
                    params![transfer_id, id, session.store_id],
                )
                .map_err(|error| error.to_string())?;
            }
            "cash_drop" => {
                let other_id = paired_location
                    .as_deref()
                    .expect("validated paired cash location");
                let (transfer_id, _) = crate::cash_locations::transfer_between_locations(
                    &tx,
                    &session,
                    register_location_id,
                    other_id,
                    input.amount_minor,
                    note,
                    &format!("movement:{}", id),
                    "drawer_movement",
                    &id,
                    Some(&shift_id),
                    &timestamp,
                )?;
                tx.execute(
                    "UPDATE cash_movements SET transfer_id=?1 WHERE id=?2 AND store_id=?3",
                    params![transfer_id, id, session.store_id],
                )
                .map_err(|error| error.to_string())?;
            }
            "paid_out" => crate::cash_locations::record_manual_drawer_outflow(
                &tx,
                &session,
                register_location_id,
                &shift_id,
                &id,
                input.amount_minor,
                note,
                &timestamp,
            )?,
            _ => unreachable!(),
        }
    }
    add_audit_event(
        &tx,
        &session,
        "cash_drawer_movement",
        "cash_movement",
        Some(&id),
        serde_json::json!({"type":input.movement_type,"amountMinor":input.amount_minor,"note":note}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    let report = report_for_shift(&db, &session.store_id, &shift_id)?;
    Ok(DrawerShift {
        id: report.shift_id,
        station_key: report.station_key,
        station_name: report.station_name,
        cashier_name: report.cashier_name,
        opened_at: report.opened_at,
        opening_cash_minor: report.opening_cash_minor,
        cash_sales_minor: report.cash_sales_minor,
        paid_in_minor: report.paid_in_minor,
        paid_out_minor: report.paid_out_minor,
        cash_drops_minor: report.cash_drops_minor,
        expected_cash_minor: report.expected_cash_minor,
        movement_count: report.movement_count,
    })
}

#[tauri::command]
pub fn get_x_report(state: State<'_, AppState>) -> Result<DrawerReport, String> {
    let session = active_session(&state, "pos:use")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let station_key = crate::stations::station_key_for_device(&db, &state.device_id, &session)?;
    let id = open_shift_id(&db, &session.store_id, &station_key)?
        .ok_or_else(|| "Open the drawer before creating an X report.".to_owned())?;
    let report = report_for_shift(&db, &session.store_id, &id)?;
    let tx = db
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    add_audit_event(
        &tx,
        &session,
        "cash_drawer_x_report",
        "cash_shift",
        Some(&id),
        serde_json::json!({"expectedCashMinor":report.expected_cash_minor}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(report)
}

#[tauri::command]
pub fn close_drawer_z(
    input: CloseDrawerInput,
    state: State<'_, AppState>,
) -> Result<DrawerReport, String> {
    let session = active_session(&state, "pos:use")?;
    require_pos_transaction(&session)?;
    if input.counted_cash_minor < 0 {
        return Err("Counted cash cannot be negative.".to_owned());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let station_key = crate::stations::station_key_for_device(&tx, &state.device_id, &session)?;
    let owned_shift: Option<String> = tx
        .query_row(
            "SELECT id FROM cash_shifts WHERE id=?1 AND store_id=?2 AND station_key=?3",
            params![input.shift_id, session.store_id, station_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if owned_shift.is_none() {
        return Err("That drawer session is not available in this store.".to_owned());
    }
    let already_closed: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_close_reports WHERE shift_id=?1)",
            [&input.shift_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if already_closed {
        let report = report_for_shift(&tx, &session.store_id, &input.shift_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok(report);
    }
    let active_id = open_shift_id(&tx, &session.store_id, &station_key)?
        .ok_or_else(|| "This drawer is already closed.".to_owned())?;
    if active_id != input.shift_id {
        return Err("The drawer session changed. Reload the register before closing.".to_owned());
    }
    let mut report = report_for_shift(&tx, &session.store_id, &input.shift_id)?;
    let difference = input
        .counted_cash_minor
        .checked_sub(report.expected_cash_minor)
        .ok_or_else(|| "Cash difference exceeded the supported amount range.".to_owned())?;
    let retained = input
        .retained_cash_minor
        .unwrap_or(input.counted_cash_minor);
    if retained < 0 || retained > input.counted_cash_minor {
        return Err("Retained till cash must be between zero and the counted amount.".to_owned());
    }
    let closing_transfer_minor = input.counted_cash_minor - retained;
    let location_id: Option<String> = tx
        .query_row(
            "SELECT cash_location_id FROM cash_shifts WHERE id=?1 AND store_id=?2",
            params![input.shift_id, session.store_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let destination = if closing_transfer_minor > 0 {
        let destination = input
            .closing_destination_location_id
            .as_deref()
            .ok_or_else(|| {
                "Choose a counted safe or petty cash location for the closing sweep.".to_owned()
            })?;
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_locations WHERE id=?1 AND store_id=?2 AND business_id=?3 AND is_active=1 AND location_type<>'register' AND EXISTS(SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=cash_locations.id AND e.event_type='opening'))",
            params![destination, session.store_id, session.business_id], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        if !valid {
            return Err(
                "Choose an active, counted safe or petty cash location for the closing sweep."
                    .to_owned(),
            );
        }
        if location_id.is_none() {
            return Err(
                "This drawer is not linked to a tracked cash location and cannot sweep to a safe."
                    .to_owned(),
            );
        }
        Some(destination)
    } else {
        None
    };
    report.counted_cash_minor = Some(input.counted_cash_minor);
    report.difference_minor = Some(difference);
    report.closing_transfer_minor = closing_transfer_minor;
    report.retained_cash_minor = retained;
    report.is_closed = true;
    let closed_at = now();
    report.closed_at = Some(closed_at.clone());
    let report_json = serde_json::to_string(&report).map_err(|error| error.to_string())?;
    let close_report_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cash_close_reports(id,shift_id,store_id,closed_by,close_type,expected_cash_minor,counted_cash_minor,difference_minor,report_json,created_at,closing_transfer_minor,retained_cash_minor)
         VALUES(?1,?2,?3,?4,'z',?5,?6,?7,?8,?9,?10,?11)",
        params![close_report_id, input.shift_id, session.store_id, session.user_id, report.expected_cash_minor, input.counted_cash_minor, difference, report_json, closed_at, closing_transfer_minor, retained],
    )
    .map_err(|error| error.to_string())?;
    if let Some(location_id) = location_id {
        crate::cash_locations::record_cash_close_variance(
            &tx,
            &session,
            &location_id,
            &input.shift_id,
            difference,
            &closed_at,
        )?;
        if closing_transfer_minor > 0 {
            let destination_id =
                destination.expect("positive close sweep has a validated destination");
            let (transfer_id, _) = crate::cash_locations::transfer_between_locations(
                &tx,
                &session,
                &location_id,
                destination_id,
                closing_transfer_minor,
                "Cash sweep at Z close",
                &format!("z-close:{}", input.shift_id),
                "z_close",
                &input.shift_id,
                Some(&input.shift_id),
                &closed_at,
            )?;
            report.closing_transfer_id = Some(transfer_id.clone());
            let final_json = serde_json::to_string(&report).map_err(|error| error.to_string())?;
            tx.execute(
                "UPDATE cash_close_reports SET closing_transfer_id=?1,report_json=?2 WHERE id=?3 AND shift_id=?4",
                params![transfer_id, final_json, close_report_id, input.shift_id],
            ).map_err(|error| error.to_string())?;
        }
    }
    let changed = tx
        .execute(
            "UPDATE cash_shifts SET closing_cash_minor=?1,closed_at=?2,status='closed' WHERE id=?3 AND store_id=?4 AND status='open'",
            params![input.counted_cash_minor, closed_at, input.shift_id, session.store_id],
        )
        .map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err(
            "The drawer changed while it was being closed. Reload and try again.".to_owned(),
        );
    }
    add_audit_event(
        &tx,
        &session,
        "cash_drawer_z_closed",
        "cash_shift",
        Some(&input.shift_id),
        serde_json::json!({"expectedCashMinor":report.expected_cash_minor,"countedCashMinor":input.counted_cash_minor,"differenceMinor":difference,"closingTransferMinor":closing_transfer_minor,"retainedCashMinor":retained}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawer_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE stores(id TEXT PRIMARY KEY);
             CREATE TABLE users(id TEXT PRIMARY KEY, full_name TEXT NOT NULL);
             CREATE TABLE cash_shifts(id TEXT PRIMARY KEY, store_id TEXT, user_id TEXT,
                 opening_cash_minor INTEGER, closing_cash_minor INTEGER, opened_at TEXT,
                 closed_at TEXT, status TEXT, station_key TEXT,
                 station_name_snapshot TEXT NOT NULL DEFAULT 'Main register');
             CREATE TABLE sales(id TEXT PRIMARY KEY, store_id TEXT, status TEXT, cash_shift_id TEXT);
             CREATE TABLE payments(id TEXT PRIMARY KEY, sale_id TEXT, method TEXT, amount_minor INTEGER);
             INSERT INTO stores VALUES('store-a');
             INSERT INTO users VALUES('cashier-a','Cashier One');
             INSERT INTO cash_shifts(id,store_id,user_id,opening_cash_minor,closing_cash_minor,opened_at,closed_at,status,station_key)
                 VALUES('shift-a','store-a','cashier-a',5000,NULL,'2026-10-29T08:00:00Z',NULL,'open','main');
             INSERT INTO sales VALUES('sale-a','store-a','completed','shift-a');
             INSERT INTO sales VALUES('sale-b','store-a','completed','shift-a');
             INSERT INTO sales VALUES('sale-c','store-a','completed',NULL);
             INSERT INTO payments VALUES('pay-a','sale-a','cash',1200);
             INSERT INTO payments VALUES('pay-b','sale-b','mobile_money',800);
             INSERT INTO payments VALUES('pay-c','sale-c','cash',9000);
             CREATE TABLE cash_movements(id TEXT,shift_id TEXT,store_id TEXT,user_id TEXT,movement_type TEXT,amount_minor INTEGER,note TEXT,created_at TEXT,source_type TEXT,source_id TEXT);
             CREATE TABLE cash_close_reports(id TEXT PRIMARY KEY,shift_id TEXT,store_id TEXT,closed_by TEXT,close_type TEXT,expected_cash_minor INTEGER,counted_cash_minor INTEGER,difference_minor INTEGER,report_json TEXT,created_at TEXT);
             INSERT INTO cash_movements VALUES('move-a','shift-a','store-a','cashier-a','paid_in',300,'float top-up','2026-10-29T09:00:00Z',NULL,NULL);
             INSERT INTO cash_movements VALUES('move-b','shift-a','store-a','cashier-a','paid_out',100,'delivery','2026-10-29T09:05:00Z',NULL,NULL);
             INSERT INTO cash_movements VALUES('move-c','shift-a','store-a','cashier-a','cash_drop',500,'safe drop','2026-10-29T09:10:00Z',NULL,NULL);",
        )
        .unwrap();
        db
    }

    #[test]
    fn x_snapshot_reconciles_only_linked_cash_and_movements() {
        let db = drawer_db();
        let report = report_for_shift(&db, "store-a", "shift-a").unwrap();
        assert_eq!(report.opening_cash_minor, 5_000);
        assert_eq!(report.cash_sales_minor, 1_200);
        assert_eq!(report.paid_in_minor, 300);
        assert_eq!(report.paid_out_minor, 100);
        assert_eq!(report.cash_drops_minor, 500);
        assert_eq!(report.expected_cash_minor, 5_900);
        assert_eq!(report.completed_sale_count, 2);
        assert_eq!(report.movement_count, 3);
        assert!(!report.is_closed);
    }

    #[test]
    fn linked_expense_and_supplier_outflows_reduce_expected_cash_once_and_are_visible() {
        let db = drawer_db();
        db.execute(
            "INSERT INTO cash_movements VALUES('expense-out','shift-a','store-a','cashier-a','paid_out',100,'Expense · delivery','2026-10-29T09:20:00Z','expense','expense-a'),('supplier-out','shift-a','store-a','cashier-a','paid_out',200,'Supplier payment','2026-10-29T09:25:00Z','supplier_payment','payment-a')",
            [],
        )
        .unwrap();

        let report = report_for_shift(&db, "store-a", "shift-a").unwrap();
        assert_eq!(report.paid_out_minor, 400);
        assert_eq!(report.expense_paid_out_minor, 100);
        assert_eq!(report.supplier_payment_paid_out_minor, 200);
        assert_eq!(report.expected_cash_minor, 5_600);
    }

    #[test]
    fn expense_reimbursement_paid_in_changes_only_the_receiving_shift_snapshot() {
        let db = drawer_db();
        let session = SessionUser {
            user_id: "cashier-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Cashier One".into(),
            username: "cashier".into(),
            role_key: "cashier".into(),
            role_name: "Cashier".into(),
            workspace_mode: "cashier".into(),
            is_owner: false,
            permissions: vec!["pos:use".into()],
            business_name: "Test".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        };

        record_expense_reimbursement_paid_in(
            &db,
            &session,
            "reimbursement-a",
            "shift-a",
            400,
            "Returned expense · Utilities",
            "2026-10-29T10:00:00Z",
        )
        .unwrap();

        let report = report_for_shift(&db, "store-a", "shift-a").unwrap();
        assert_eq!(report.paid_in_minor, 700);
        assert_eq!(report.expense_reimbursement_paid_in_minor, 400);
        assert_eq!(report.expected_cash_minor, 6_300);
    }

    #[test]
    fn drawer_report_rejects_a_shift_from_another_store() {
        let db = drawer_db();
        assert!(report_for_shift(&db, "store-b", "shift-a").is_err());
    }

    #[test]
    fn closed_drawer_returns_the_saved_z_snapshot() {
        let db = drawer_db();
        let mut saved = report_for_shift(&db, "store-a", "shift-a").unwrap();
        saved.counted_cash_minor = Some(5_850);
        saved.difference_minor = Some(-50);
        saved.closed_at = Some("2026-10-29T17:00:00Z".to_owned());
        saved.is_closed = true;
        db.execute(
            "UPDATE cash_shifts SET status='closed',closed_at=?1,closing_cash_minor=?2 WHERE id='shift-a'",
            params![saved.closed_at, saved.counted_cash_minor],
        )
        .unwrap();
        db.execute(
            "INSERT INTO cash_close_reports VALUES('close-a','shift-a','store-a','cashier-a','z',5900,5850,-50,?1,'2026-10-29T17:00:00Z')",
            [serde_json::to_string(&saved).unwrap()],
        )
        .unwrap();
        db.execute("UPDATE sales SET status='refunded' WHERE id='sale-a'", [])
            .unwrap();

        // A later return belongs to the later shift and must not recalculate
        // the closed shift's saved Z snapshot.
        db.execute(
            "INSERT INTO cash_shifts(id,store_id,user_id,opening_cash_minor,closing_cash_minor,opened_at,closed_at,status,station_key) VALUES('shift-b','store-a','cashier-a',5000,NULL,'2026-10-30T08:00:00Z',NULL,'open','main')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO cash_movements VALUES('reimbursement-b','shift-b','store-a','cashier-a','paid_in',400,'Returned expense','2026-10-30T09:00:00Z','expense_reimbursement','return-b')",
            [],
        )
        .unwrap();

        let report = report_for_shift(&db, "store-a", "shift-a").unwrap();

        assert_eq!(report.cash_sales_minor, 1_200);
        assert_eq!(report.completed_sale_count, 2);
        assert_eq!(report.expected_cash_minor, 5_900);
        assert_eq!(report.difference_minor, Some(-50));
        assert!(report.is_closed);
    }

    #[test]
    fn closed_history_pages_remain_stable_when_a_new_shift_is_closed() {
        let mut db = drawer_db();
        db.execute(
            "UPDATE cash_shifts SET status='closed',closed_at='2026-10-29T17:00:00Z' WHERE id='shift-a'",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO cash_shifts(id,store_id,user_id,opening_cash_minor,closing_cash_minor,opened_at,closed_at,status,station_key) VALUES('shift-b','store-a','cashier-a',0,0,'2026-10-30T08:00:00Z','2026-10-30T17:00:00Z','closed','main')",
            [],
        )
        .unwrap();

        let first = closed_history_page(
            &mut db,
            "store-a",
            "cashier-a",
            false,
            DrawerHistoryQuery {
                offset: 0,
                page_size: 1,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        assert_eq!(first.rows[0].shift_id, "shift-b");
        assert_eq!(first.matched_count, 2);

        db.execute(
            "INSERT INTO cash_shifts(id,store_id,user_id,opening_cash_minor,closing_cash_minor,opened_at,closed_at,status,station_key) VALUES('shift-c','store-a','cashier-a',0,0,'2026-10-31T08:00:00Z','2026-10-31T17:00:00Z','closed','main')",
            [],
        )
        .unwrap();

        let second = closed_history_page(
            &mut db,
            "store-a",
            "cashier-a",
            false,
            DrawerHistoryQuery {
                offset: 1,
                page_size: 1,
                snapshot_row_id: Some(first.snapshot_row_id),
            },
        )
        .unwrap();
        assert_eq!(second.rows[0].shift_id, "shift-a");
        assert_eq!(second.matched_count, 2);
        assert!(!second.rows.iter().any(|row| row.shift_id == "shift-c"));
    }

    #[test]
    fn staff_history_is_limited_to_their_store_and_shifts() {
        let mut db = drawer_db();
        db.execute("INSERT INTO users VALUES('cashier-b','Cashier Two')", [])
            .unwrap();
        db.execute(
            "INSERT INTO cash_shifts(id,store_id,user_id,opening_cash_minor,closing_cash_minor,opened_at,closed_at,status,station_key) VALUES('other-store-shift','store-b','cashier-b',0,0,'2026-10-30T08:00:00Z','2026-10-30T17:00:00Z','closed','main'),('other-user-shift','store-a','cashier-b',0,0,'2026-10-30T08:00:00Z','2026-10-30T17:00:00Z','closed','main'),('mine','store-a','cashier-a',0,0,'2026-10-30T08:00:00Z','2026-10-30T17:00:00Z','closed','main')",
            [],
        )
        .unwrap();

        let staff = closed_history_page(
            &mut db,
            "store-a",
            "cashier-a",
            false,
            DrawerHistoryQuery {
                offset: 0,
                page_size: 10,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        let manager = closed_history_page(
            &mut db,
            "store-a",
            "cashier-a",
            true,
            DrawerHistoryQuery {
                offset: 0,
                page_size: 10,
                snapshot_row_id: None,
            },
        )
        .unwrap();

        assert_eq!(staff.matched_count, 1);
        assert_eq!(staff.rows[0].shift_id, "mine");
        assert_eq!(manager.matched_count, 2);
        assert!(!manager
            .rows
            .iter()
            .any(|row| row.shift_id == "other-store-shift"));
    }
}
