// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashLocation {
    pub id: String,
    pub name: String,
    pub location_type: String,
    pub station_key: Option<String>,
    pub is_active: bool,
    pub opening_recorded: bool,
    pub balance_minor: i64,
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashLocationWorkspace {
    pub locations: Vec<CashLocation>,
    pub pending_variances: Vec<CashLocationVariance>,
    pub tracked_cash_minor: i64,
    pub ledger_cash_minor: i64,
    pub variance_minor: i64,
    pub all_locations_initialized: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashLocationVariance {
    pub event_id: String,
    pub location_name: String,
    pub variance_type: String,
    pub direction: String,
    pub amount_minor: i64,
    pub note: String,
    pub effective_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashLocationOption {
    pub id: String,
    pub name: String,
    pub location_type: String,
    pub station_key: Option<String>,
    pub balance_minor: i64,
    pub opening_recorded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCashLocationInput {
    pub name: String,
    pub location_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashLocationOpeningInput {
    pub location_id: String,
    pub amount_minor: i64,
    pub effective_at: String,
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashLocationTransferInput {
    pub from_location_id: String,
    pub to_location_id: String,
    pub amount_minor: i64,
    pub note: String,
    pub idempotency_key: String,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn bounded_key(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 100 {
        return Err("A retry key between 1 and 100 characters is required.".to_owned());
    }
    Ok(value)
}

fn ensure_register_locations(connection: &Connection, session: &SessionUser) -> Result<(), String> {
    connection.execute(
        "INSERT OR IGNORE INTO cash_locations(id,business_id,store_id,name,location_type,currency_code,station_key,is_active,created_by,created_at)
         SELECT lower(hex(randomblob(16))),rs.business_id,rs.store_id,rs.display_name,'register',b.currency_code,rs.station_key,rs.is_active,?3,?4
         FROM register_stations rs JOIN businesses b ON b.id=rs.business_id
         WHERE rs.business_id=?1 AND rs.store_id=?2",
        params![session.business_id, session.store_id, session.user_id, now()],
    ).map_err(|error| format!("Could not prepare register cash locations: {error}"))?;
    Ok(())
}

pub(crate) fn opening_recorded(connection: &Connection, location_id: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_location_events WHERE cash_location_id=?1 AND event_type='opening')",
            [location_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

/// Initialize an uncounted register location from the physical count already
/// entered by a manager, administrator, or owner in the open-drawer flow.
/// The opening event and drawer shift are committed by the caller's transaction.
pub(crate) fn initialize_register_opening_from_drawer(
    connection: &Connection,
    session: &SessionUser,
    location_id: &str,
    amount_minor: i64,
    effective_at: &str,
) -> Result<bool, String> {
    if opening_recorded(connection, location_id)? {
        return Ok(false);
    }
    let may_initialize = session.is_owner
        || matches!(session.role_key.as_str(), "admin" | "manager")
        || session
            .permissions
            .iter()
            .any(|permission| permission == "settings:write");
    if !may_initialize {
        return Err(
            "A manager, administrator, or owner must enter this register's first cash count."
                .to_owned(),
        );
    }
    let valid_register: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_locations
             WHERE id=?1 AND business_id=?2 AND store_id=?3 AND is_active=1 AND location_type='register')",
            params![location_id, session.business_id, session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !valid_register {
        return Err("The assigned register cash location is unavailable.".to_owned());
    }
    let reason = "Initial physical count entered when opening the register";
    insert_event(
        connection,
        session,
        location_id,
        None,
        "opening",
        "in",
        amount_minor,
        "opening",
        location_id,
        None,
        Some(&format!("register-initial:{location_id}")),
        reason,
        "cleared",
        effective_at,
    )?;
    add_audit_event(
        connection,
        session,
        "cash_location_opening_recorded",
        "cash_location",
        Some(location_id),
        serde_json::json!({
            "amountMinor": amount_minor,
            "effectiveAt": effective_at,
            "reason": reason,
            "locationType": "register",
            "recordedFrom": "register_open"
        }),
    )?;
    Ok(true)
}

fn opening_exists(connection: &Connection, location_id: &str) -> Result<bool, String> {
    opening_recorded(connection, location_id)
}

pub(crate) fn location_balance_minor(
    connection: &Connection,
    location_id: &str,
) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COALESCE(SUM(CASE WHEN direction='in' THEN amount_minor ELSE -amount_minor END),0)
             FROM cash_location_events WHERE cash_location_id=?1",
            [location_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

pub(crate) fn location_for_station(
    connection: &Connection,
    store_id: &str,
    station_key: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT id FROM cash_locations WHERE store_id=?1 AND station_key=?2 AND location_type='register' AND is_active=1",
            params![store_id, station_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub(crate) fn resolve_source_location(
    connection: &Connection,
    device_id: &str,
    session: &SessionUser,
    cash_origin: &str,
    supplied_location_id: Option<&str>,
) -> Result<Option<String>, String> {
    match cash_origin {
        "not_applicable" => Ok(None),
        "drawer" => {
            let station_key =
                crate::stations::station_key_for_device(connection, device_id, session)?;
            let location_id: String = connection.query_row(
                "SELECT cash_location_id FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open' AND cash_location_id IS NOT NULL",
                params![session.store_id,station_key], |row| row.get(0),
            ).optional().map_err(|error|error.to_string())?.flatten()
                .ok_or_else(||"Open a tracked register drawer before paying this cash expense.".to_owned())?;
            Ok(Some(location_id))
        }
        "other_cash" => {
            let id = supplied_location_id.ok_or_else(|| {
                "Choose the safe or petty cash location used for this payment.".to_owned()
            })?;
            let valid: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM cash_locations WHERE id=?1 AND store_id=?2 AND business_id=?3 AND is_active=1 AND location_type<>'register' AND EXISTS(SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=cash_locations.id AND e.event_type='opening'))",
                params![id,session.store_id,session.business_id], |row| row.get(0),
            ).map_err(|error|error.to_string())?;
            if !valid {
                return Err(
                    "Choose an active safe or petty cash location with an opening count."
                        .to_owned(),
                );
            }
            Ok(Some(id.to_owned()))
        }
        _ => Err("Cash origin does not match its payment method.".to_owned()),
    }
}

fn insert_event(
    connection: &Connection,
    session: &SessionUser,
    location_id: &str,
    shift_id: Option<&str>,
    event_type: &str,
    direction: &str,
    amount_minor: i64,
    source_type: &str,
    source_id: &str,
    transfer_id: Option<&str>,
    idempotency_key: Option<&str>,
    note: &str,
    review_status: &str,
    effective_at: &str,
) -> Result<(), String> {
    if amount_minor < 0 || (event_type != "opening" && amount_minor == 0) {
        return Err("Cash location events must have a positive amount.".to_owned());
    }
    connection.execute(
        "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,shift_id,event_type,direction,amount_minor,source_type,source_id,transfer_id,idempotency_key,note,review_status,actor_user_id,effective_at,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![Uuid::new_v4().to_string(),session.business_id,session.store_id,location_id,shift_id,event_type,direction,amount_minor,source_type,source_id,transfer_id,idempotency_key,note,review_status,session.user_id,effective_at,now()],
    ).map_err(|error| format!("Could not record the cash location event: {error}"))?;
    Ok(())
}

pub(crate) fn record_cash_sale(
    connection: &Connection,
    session: &SessionUser,
    sale_id: &str,
    shift_id: Option<&str>,
    amount_minor: i64,
    effective_at: &str,
) -> Result<(), String> {
    let Some(shift_id) = shift_id else {
        return Ok(());
    };
    let location_id: Option<String> = connection
        .query_row(
            "SELECT cash_location_id FROM cash_shifts WHERE id=?1 AND store_id=?2",
            params![shift_id, session.store_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    let Some(location_id) = location_id else {
        return Ok(());
    };
    insert_event(
        connection,
        session,
        &location_id,
        Some(shift_id),
        "sale",
        "in",
        amount_minor,
        "sale",
        sale_id,
        None,
        None,
        "Cash sale",
        "cleared",
        effective_at,
    )
}

pub(crate) fn record_shift_open_variance(
    connection: &Connection,
    session: &SessionUser,
    location_id: &str,
    shift_id: &str,
    counted_minor: i64,
    tracked_minor: i64,
    effective_at: &str,
) -> Result<(), String> {
    let difference = counted_minor
        .checked_sub(tracked_minor)
        .ok_or_else(|| "Cash count difference exceeded the supported range.".to_owned())?;
    if difference == 0 {
        return Ok(());
    }
    insert_event(
        connection,
        session,
        location_id,
        Some(shift_id),
        "shift_open_variance",
        if difference > 0 { "in" } else { "out" },
        difference.abs(),
        "shift",
        shift_id,
        None,
        None,
        "Register opening count variance",
        "pending",
        effective_at,
    )
}

pub(crate) fn record_cash_close_variance(
    connection: &Connection,
    session: &SessionUser,
    location_id: &str,
    shift_id: &str,
    difference_minor: i64,
    effective_at: &str,
) -> Result<(), String> {
    if difference_minor == 0 {
        return Ok(());
    }
    insert_event(
        connection,
        session,
        location_id,
        Some(shift_id),
        "close_variance",
        if difference_minor > 0 { "in" } else { "out" },
        difference_minor.abs(),
        "z_close",
        shift_id,
        None,
        None,
        "Register close count variance",
        "pending",
        effective_at,
    )
}

pub(crate) fn record_manual_drawer_outflow(
    connection: &Connection,
    session: &SessionUser,
    location_id: &str,
    shift_id: &str,
    movement_id: &str,
    amount_minor: i64,
    note: &str,
    effective_at: &str,
) -> Result<(), String> {
    insert_event(
        connection,
        session,
        location_id,
        Some(shift_id),
        "manual_outflow",
        "out",
        amount_minor,
        "cash_movement",
        movement_id,
        None,
        None,
        note,
        "cleared",
        effective_at,
    )
}

pub(crate) fn record_cash_refund(
    connection: &Connection,
    session: &SessionUser,
    refund_id: &str,
    shift_id: Option<&str>,
    amount_minor: i64,
    effective_at: &str,
) -> Result<(), String> {
    let Some(shift_id) = shift_id else {
        return Ok(());
    };
    let location_id: Option<String> = connection
        .query_row(
            "SELECT cash_location_id FROM cash_shifts WHERE id=?1 AND store_id=?2",
            params![shift_id, session.store_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    let Some(location_id) = location_id else {
        return Ok(());
    };
    insert_event(
        connection,
        session,
        &location_id,
        Some(shift_id),
        "refund",
        "out",
        amount_minor,
        "refund",
        refund_id,
        None,
        None,
        "Cash refund",
        "cleared",
        effective_at,
    )
}

pub(crate) fn record_source_cash_outflow(
    connection: &Connection,
    session: &SessionUser,
    source_type: &str,
    source_id: &str,
    shift_id: Option<&str>,
    amount_minor: i64,
    note: &str,
    effective_at: &str,
) -> Result<(), String> {
    if !matches!(source_type, "expense" | "supplier_payment") {
        return Err("Unsupported cash source.".to_owned());
    }
    let table = if source_type == "expense" {
        "expenses"
    } else {
        "supplier_payments"
    };
    let location_id: Option<String> = connection.query_row(
        &format!("SELECT cash_location_id FROM {table} WHERE id=?1 AND store_id=?2 AND payment_method='cash'"),
        params![source_id,session.store_id], |row| row.get(0),
    ).optional().map_err(|error| error.to_string())?.flatten();
    let location_id = location_id
        .ok_or_else(|| "Choose a tracked cash location for this cash outflow.".to_owned())?;
    if let Some(shift_id) = shift_id {
        let shift_location: Option<String> = connection
            .query_row(
                "SELECT cash_location_id FROM cash_shifts WHERE id=?1 AND store_id=?2",
                params![shift_id, session.store_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten();
        if shift_location.as_deref() != Some(location_id.as_str()) {
            return Err("The cash source does not match the open register location.".to_owned());
        }
    }
    insert_event(
        connection,
        session,
        &location_id,
        shift_id,
        source_type,
        "out",
        amount_minor,
        source_type,
        source_id,
        None,
        None,
        note,
        "cleared",
        effective_at,
    )
}

pub(crate) fn record_expense_reimbursement_cash_inflow(
    connection: &Connection,
    session: &SessionUser,
    reimbursement_id: &str,
    location_id: &str,
    shift_id: Option<&str>,
    amount_minor: i64,
    note: &str,
    effective_at: &str,
) -> Result<(), String> {
    if amount_minor <= 0 {
        return Err("Cash reimbursement must be greater than zero.".to_owned());
    }
    insert_event(
        connection,
        session,
        location_id,
        shift_id,
        "expense_reimbursement",
        "in",
        amount_minor,
        "expense_reimbursement",
        reimbursement_id,
        None,
        None,
        note,
        "cleared",
        effective_at,
    )
}

pub(crate) fn transfer_between_locations(
    connection: &Connection,
    session: &SessionUser,
    from_location_id: &str,
    to_location_id: &str,
    amount_minor: i64,
    note: &str,
    idempotency_key: &str,
    source_type: &str,
    source_id: &str,
    shift_id: Option<&str>,
    effective_at: &str,
) -> Result<(String, bool), String> {
    let key = bounded_key(idempotency_key)?;
    let note = note.trim();
    if from_location_id == to_location_id {
        return Err("Choose two different cash locations.".to_owned());
    }
    if amount_minor <= 0 || amount_minor > 9_000_000_000_000 {
        return Err(
            "Enter a transfer amount greater than zero and within the supported range.".to_owned(),
        );
    }
    if note.is_empty() || note.chars().count() > 200 {
        return Err("Add a short reason of up to 200 characters.".to_owned());
    }
    if !matches!(source_type, "manual" | "drawer_movement" | "z_close") {
        return Err("The cash transfer source is invalid.".to_owned());
    }
    let existing: Option<(String,String,String,i64,String,String)> = connection.query_row(
        "SELECT id,from_location_id,to_location_id,amount_minor,note,source_id FROM cash_location_transfers WHERE store_id=?1 AND idempotency_key=?2",
        params![session.store_id,key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
    ).optional().map_err(|error| error.to_string())?;
    if let Some((id, from, to, amount, saved_note, saved_source_id)) = existing {
        if from == from_location_id
            && to == to_location_id
            && amount == amount_minor
            && saved_note == note
            && saved_source_id == source_id
        {
            return Ok((id, true));
        }
        return Err("This retry key was already used for a different cash transfer.".to_owned());
    }
    for location_id in [from_location_id, to_location_id] {
        let active: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cash_locations WHERE id=?1 AND store_id=?2 AND business_id=?3 AND is_active=1)",
            params![location_id,session.store_id,session.business_id], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        if !active {
            return Err("Choose active cash locations from this store.".to_owned());
        }
        if !opening_exists(connection, location_id)? {
            return Err(
                "Record an opening count for each cash location before transferring funds."
                    .to_owned(),
            );
        }
    }
    let available = location_balance_minor(connection, from_location_id)?;
    if available < amount_minor {
        return Err("The source cash location does not have enough available funds.".to_owned());
    }

    let transfer_id = Uuid::new_v4().to_string();
    let timestamp = now();
    connection.execute(
        "INSERT INTO cash_location_transfers(id,business_id,store_id,from_location_id,to_location_id,amount_minor,idempotency_key,note,source_type,source_id,created_by,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![transfer_id,session.business_id,session.store_id,from_location_id,to_location_id,amount_minor,key,note,source_type,source_id,session.user_id,timestamp],
    ).map_err(|error| format!("Could not save the cash transfer: {error}"))?;
    let (from_shift, to_shift) = if let Some(shift) = shift_id {
        (Some(shift), Some(shift))
    } else {
        (None, None)
    };
    insert_event(
        connection,
        session,
        from_location_id,
        from_shift,
        "transfer",
        "out",
        amount_minor,
        "transfer",
        &transfer_id,
        Some(&transfer_id),
        Some(&format!("{key}:out")),
        note,
        "cleared",
        effective_at,
    )?;
    insert_event(
        connection,
        session,
        to_location_id,
        to_shift,
        "transfer",
        "in",
        amount_minor,
        "transfer",
        &transfer_id,
        Some(&transfer_id),
        Some(&format!("{key}:in")),
        note,
        "cleared",
        effective_at,
    )?;
    Ok((transfer_id, false))
}

fn read_locations(
    connection: &Connection,
    session: &SessionUser,
) -> Result<(Vec<CashLocation>, Vec<CashLocationVariance>, i64, i64, bool), String> {
    let mut statement = connection.prepare(
        "SELECT l.id,l.name,l.location_type,l.station_key,l.is_active,l.currency_code,
             EXISTS(SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=l.id AND e.event_type='opening'),
             COALESCE((SELECT SUM(CASE WHEN e.direction='in' THEN e.amount_minor ELSE -e.amount_minor END) FROM cash_location_events e WHERE e.cash_location_id=l.id),0)
         FROM cash_locations l WHERE l.business_id=?1 AND l.store_id=?2 ORDER BY l.location_type,l.name,l.id",
    ).map_err(|error| error.to_string())?;
    let locations = statement
        .query_map(params![session.business_id, session.store_id], |row| {
            Ok(CashLocation {
                id: row.get(0)?,
                name: row.get(1)?,
                location_type: row.get(2)?,
                station_key: row.get(3)?,
                is_active: row.get::<_, i64>(4)? != 0,
                currency_code: row.get(5)?,
                opening_recorded: row.get::<_, i64>(6)? != 0,
                balance_minor: row.get(7)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut variance_statement = connection
        .prepare(
            "SELECT e.id,l.name,e.event_type,e.direction,e.amount_minor,e.note,e.effective_at
         FROM cash_location_events e JOIN cash_locations l ON l.id=e.cash_location_id
         LEFT JOIN cash_location_event_reviews r ON r.event_id=e.id
         WHERE e.business_id=?1 AND e.store_id=?2 AND e.review_status='pending' AND r.id IS NULL
         ORDER BY e.effective_at,e.created_at,e.id",
        )
        .map_err(|error| error.to_string())?;
    let pending_variances = variance_statement
        .query_map(params![session.business_id, session.store_id], |row| {
            Ok(CashLocationVariance {
                event_id: row.get(0)?,
                location_name: row.get(1)?,
                variance_type: row.get(2)?,
                direction: row.get(3)?,
                amount_minor: row.get(4)?,
                note: row.get(5)?,
                effective_at: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(variance_statement);
    let tracked = locations
        .iter()
        .filter(|location| location.opening_recorded)
        .try_fold(0_i64, |sum, location| {
            sum.checked_add(location.balance_minor)
                .ok_or_else(|| "Tracked cash total is too large.".to_owned())
        })?;
    let all_initialized = locations
        .iter()
        .filter(|location| location.is_active)
        .all(|location| location.opening_recorded);
    let ledger_cash: i64 = connection.query_row(
        "SELECT COALESCE(SUM(l.debit_minor-l.credit_minor),0) FROM accounting_journal_lines l JOIN accounting_journals j ON j.id=l.journal_id AND j.business_id=l.business_id JOIN accounting_accounts a ON a.id=l.account_id AND a.business_id=l.business_id WHERE j.business_id=?1 AND j.store_id=?2 AND a.code='1000'",
        params![session.business_id,session.store_id],|row|row.get(0),
    ).map_err(|error|error.to_string())?;
    Ok((
        locations,
        pending_variances,
        tracked,
        ledger_cash,
        all_initialized,
    ))
}

#[tauri::command]
pub fn get_cash_location_workspace(
    state: tauri::State<'_, AppState>,
) -> Result<CashLocationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    ensure_register_locations(&tx, &session)?;
    let (locations, pending_variances, tracked, ledger, all_initialized) =
        read_locations(&tx, &session)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(CashLocationWorkspace {
        locations,
        pending_variances,
        tracked_cash_minor: tracked,
        ledger_cash_minor: ledger,
        variance_minor: tracked - ledger,
        all_locations_initialized: all_initialized,
    })
}

#[tauri::command]
pub fn get_cash_location_options(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CashLocationOption>, String> {
    let session = active_session(&state, "")?;
    if !session.permissions.iter().any(|permission| {
        matches!(
            permission.as_str(),
            "pos:use" | "expenses:write" | "suppliers:settle" | "settings:write"
        )
    }) {
        return Err("Your account cannot view cash locations.".to_owned());
    }
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    ensure_register_locations(&tx, &session)?;
    let mut statement=tx.prepare("SELECT l.id,l.name,l.location_type,l.station_key,EXISTS(SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=l.id AND e.event_type='opening'),COALESCE((SELECT SUM(CASE WHEN e.direction='in' THEN e.amount_minor ELSE -e.amount_minor END) FROM cash_location_events e WHERE e.cash_location_id=l.id),0) FROM cash_locations l WHERE l.business_id=?1 AND l.store_id=?2 AND l.is_active=1 ORDER BY l.location_type,l.name").map_err(|e|e.to_string())?;
    let values = statement
        .query_map(params![session.business_id, session.store_id], |r| {
            Ok(CashLocationOption {
                id: r.get(0)?,
                name: r.get(1)?,
                location_type: r.get(2)?,
                station_key: r.get(3)?,
                opening_recorded: r.get::<_, i64>(4)? != 0,
                balance_minor: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(statement);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(values)
}

#[tauri::command]
pub fn create_cash_location(
    input: CreateCashLocationInput,
    state: tauri::State<'_, AppState>,
) -> Result<CashLocationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("Enter a cash location name up to 80 characters.".to_owned());
    }
    if !matches!(
        input.location_type.as_str(),
        "safe" | "petty_cash" | "other"
    ) {
        return Err("Choose a safe, petty cash box, or other cash location.".to_owned());
    }
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO cash_locations(id,business_id,store_id,name,location_type,currency_code,station_key,is_active,created_by,created_at) VALUES(?1,?2,?3,?4,?5,?6,NULL,1,?7,?8)",params![id,session.business_id,session.store_id,name,input.location_type,session.currency_code,session.user_id,now()]).map_err(|_|"A cash location with that name already exists in this store.".to_owned())?;
    add_audit_event(
        &tx,
        &session,
        "cash_location_created",
        "cash_location",
        Some(&id),
        serde_json::json!({"name":name,"locationType":input.location_type}),
    )?;
    let (locations, pending_variances, tracked, ledger, all_initialized) =
        read_locations(&tx, &session)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(CashLocationWorkspace {
        locations,
        pending_variances,
        tracked_cash_minor: tracked,
        ledger_cash_minor: ledger,
        variance_minor: tracked - ledger,
        all_locations_initialized: all_initialized,
    })
}

#[tauri::command]
pub fn record_cash_location_opening(
    input: CashLocationOpeningInput,
    state: tauri::State<'_, AppState>,
) -> Result<CashLocationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let key = bounded_key(&input.idempotency_key)?;
    if input.amount_minor < 0 || input.amount_minor > 9_000_000_000_000 {
        return Err(
            "Opening cash count must be zero or greater and within the supported range.".to_owned(),
        );
    }
    let reason = input.reason.trim();
    if reason.chars().count() < 3 || reason.chars().count() > 200 {
        return Err("Give an opening-count reason between 3 and 200 characters.".to_owned());
    }
    let effective_at = crate::sales::canonical_timestamp(&input.effective_at)?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let replay:Option<(String,i64,String,String)>=tx.query_row("SELECT cash_location_id,amount_minor,effective_at,note FROM cash_location_events WHERE store_id=?1 AND idempotency_key=?2",params![session.store_id,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e|e.to_string())?;
    if let Some((location, amount, date, saved_reason)) = replay {
        if location == input.location_id
            && amount == input.amount_minor
            && date == effective_at
            && saved_reason == reason
        {
            let (locations, pending_variances, tracked, ledger, all_initialized) =
                read_locations(&tx, &session)?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(CashLocationWorkspace {
                locations,
                pending_variances,
                tracked_cash_minor: tracked,
                ledger_cash_minor: ledger,
                variance_minor: tracked - ledger,
                all_locations_initialized: all_initialized,
            });
        }
        return Err(
            "This retry key was already used for a different cash opening count.".to_owned(),
        );
    }
    let location_type:String=tx.query_row("SELECT location_type FROM cash_locations WHERE id=?1 AND store_id=?2 AND business_id=?3 AND is_active=1",params![input.location_id,session.store_id,session.business_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?.ok_or_else(||"Choose an active cash location from this store.".to_owned())?;
    if opening_exists(&tx, &input.location_id)? {
        return Err("An opening count is already recorded for this cash location.".to_owned());
    }
    insert_event(
        &tx,
        &session,
        &input.location_id,
        None,
        "opening",
        "in",
        input.amount_minor,
        "opening",
        &input.location_id,
        None,
        Some(key),
        reason,
        "cleared",
        &effective_at,
    )?;
    add_audit_event(
        &tx,
        &session,
        "cash_location_opening_recorded",
        "cash_location",
        Some(&input.location_id),
        serde_json::json!({"amountMinor":input.amount_minor,"effectiveAt":effective_at,"reason":reason,"locationType":location_type}),
    )?;
    let (locations, pending_variances, tracked, ledger, all_initialized) =
        read_locations(&tx, &session)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(CashLocationWorkspace {
        locations,
        pending_variances,
        tracked_cash_minor: tracked,
        ledger_cash_minor: ledger,
        variance_minor: tracked - ledger,
        all_locations_initialized: all_initialized,
    })
}

#[tauri::command]
pub fn transfer_cash_locations(
    input: CashLocationTransferInput,
    state: tauri::State<'_, AppState>,
) -> Result<CashLocationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (transfer_id, replayed) = transfer_between_locations(
        &tx,
        &session,
        &input.from_location_id,
        &input.to_location_id,
        input.amount_minor,
        &input.note,
        &input.idempotency_key,
        "manual",
        &input.idempotency_key,
        None,
        &now(),
    )?;
    if !replayed {
        add_audit_event(
            &tx,
            &session,
            "cash_location_transfer_recorded",
            "cash_location_transfer",
            Some(&transfer_id),
            serde_json::json!({"fromLocationId":input.from_location_id,"toLocationId":input.to_location_id,"amountMinor":input.amount_minor,"note":input.note.trim()}),
        )?;
    }
    let (locations, pending_variances, tracked, ledger, all_initialized) =
        read_locations(&tx, &session)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(CashLocationWorkspace {
        locations,
        pending_variances,
        tracked_cash_minor: tracked,
        ledger_cash_minor: ledger,
        variance_minor: tracked - ledger,
        all_locations_initialized: all_initialized,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCashVarianceInput {
    pub event_id: String,
    pub note: String,
}

#[tauri::command]
pub fn review_cash_location_variance(
    input: ReviewCashVarianceInput,
    state: tauri::State<'_, AppState>,
) -> Result<CashLocationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let note = input.note.trim();
    if note.chars().count() < 3 || note.chars().count() > 200 {
        return Err("Enter a review note between 3 and 200 characters.".to_owned());
    }
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    tx.execute(
        "INSERT INTO cash_location_event_reviews(id,business_id,store_id,event_id,reviewed_by,note,reviewed_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![Uuid::new_v4().to_string(),session.business_id,session.store_id,input.event_id,session.user_id,note,now()],
    ).map_err(|error| {
        if error.to_string().contains("UNIQUE constraint failed") {
            "This cash count variance has already been reviewed.".to_owned()
        } else {
            format!("Could not save the variance review: {error}")
        }
    })?;
    add_audit_event(
        &tx,
        &session,
        "cash_location_variance_reviewed",
        "cash_location_event",
        Some(&input.event_id),
        serde_json::json!({"note":note}),
    )?;
    let (locations, pending_variances, tracked, ledger, all_initialized) =
        read_locations(&tx, &session)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(CashLocationWorkspace {
        locations,
        pending_variances,
        tracked_cash_minor: tracked,
        ledger_cash_minor: ledger,
        variance_minor: tracked - ledger,
        all_locations_initialized: all_initialized,
    })
}
