// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    accounting,
    commands::{active_session, add_audit_event},
    db::AppState,
    models::{ExpenseReimbursement, ExpenseReimbursementInput, SessionUser},
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use tauri::State;
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn can_record_reimbursement(session: &SessionUser) -> bool {
    session.is_owner || matches!(session.role_key.as_str(), "admin" | "manager" | "owner")
}

fn saved_reimbursement(
    connection: &Connection,
    store_id: &str,
    idempotency_key: &str,
) -> Result<Option<ExpenseReimbursement>, String> {
    connection
        .query_row(
            "SELECT r.id,r.expense_id,r.amount_minor,r.return_method,r.return_reference,r.reason,
                r.cash_location_id,r.cash_shift_id,r.returned_at,u.full_name
         FROM expense_reimbursements r JOIN users u ON u.id=r.recorded_by
         WHERE r.store_id=?1 AND r.idempotency_key=?2",
            params![store_id, idempotency_key],
            |row| {
                Ok(ExpenseReimbursement {
                    id: row.get(0)?,
                    expense_id: row.get(1)?,
                    amount_minor: row.get(2)?,
                    return_method: row.get(3)?,
                    return_reference: row.get(4)?,
                    reason: row.get(5)?,
                    cash_location_id: row.get(6)?,
                    cash_shift_id: row.get(7)?,
                    returned_at: row.get(8)?,
                    recorded_by: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn request_matches(saved: &ExpenseReimbursement, input: &ExpenseReimbursementInput) -> bool {
    saved.expense_id == input.expense_id
        && saved.amount_minor == input.amount_minor
        && saved.return_method == input.return_method
        && saved.return_reference == input.return_reference
        && saved.reason == input.reason
        && saved.cash_location_id == input.cash_location_id
        && saved.cash_shift_id == input.cash_shift_id
}

#[tauri::command]
pub fn record_expense_reimbursement(
    input: ExpenseReimbursementInput,
    state: State<'_, AppState>,
) -> Result<ExpenseReimbursement, String> {
    let session = active_session(&state, "expenses:write")?;
    if !can_record_reimbursement(&session) {
        return Err(
            "Only a manager, administrator, or owner can record an expense reimbursement."
                .to_owned(),
        );
    }
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    record_reimbursement_with_session(input, &session, &state.device_id, &mut connection)
}

fn record_reimbursement_with_session(
    mut input: ExpenseReimbursementInput,
    session: &SessionUser,
    device_id: &str,
    connection: &mut Connection,
) -> Result<ExpenseReimbursement, String> {
    input.expense_id = input.expense_id.trim().to_owned();
    input.return_method = input.return_method.trim().to_ascii_lowercase();
    input.return_reference = input
        .return_reference
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    input.reason = input.reason.trim().to_owned();
    input.cash_location_id = input
        .cash_location_id
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    input.cash_shift_id = input
        .cash_shift_id
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    input.idempotency_key = input.idempotency_key.trim().to_owned();

    if input.expense_id.is_empty() || input.expense_id.len() > 80 {
        return Err("The expense identifier is invalid.".to_owned());
    }
    if input.amount_minor <= 0 || input.amount_minor > 9_000_000_000_000 {
        return Err(
            "Enter a reimbursement amount greater than zero and within the supported range."
                .to_owned(),
        );
    }
    if !matches!(
        input.return_method.as_str(),
        "cash" | "card" | "mobile_money" | "bank_transfer" | "other"
    ) {
        return Err("Choose a supported method for the returned payment.".to_owned());
    }
    if input.reason.chars().count() < 5 || input.reason.chars().count() > 500 {
        return Err("Explain the reimbursement in 5 to 500 characters.".to_owned());
    }
    if input
        .return_reference
        .as_ref()
        .is_some_and(|value| value.chars().count() > 120)
    {
        return Err("Keep the returned-payment reference under 120 characters.".to_owned());
    }
    if input.idempotency_key.is_empty() || input.idempotency_key.len() > 100 {
        return Err("The reimbursement retry key is invalid.".to_owned());
    }
    if input.return_method == "cash" {
        if input.cash_location_id.is_none() {
            return Err(
                "Choose the tracked location that physically received the cash.".to_owned(),
            );
        }
    } else if input.return_reference.is_none() {
        return Err(
            "Enter the returned-payment reference for this non-cash reimbursement.".to_owned(),
        );
    } else if input.cash_location_id.is_some() || input.cash_shift_id.is_some() {
        return Err(
            "Only a cash reimbursement can use a cash location or register shift.".to_owned(),
        );
    }

    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;

    // A retry after shift close must return its original record before checking current shift state.
    if let Some(saved) = saved_reimbursement(&tx, &session.store_id, &input.idempotency_key)? {
        if request_matches(&saved, &input) {
            return Ok(saved);
        }
        return Err(
            "This reimbursement retry key was already used for different details.".to_owned(),
        );
    }

    let (source_amount, source_method, category): (i64, String, String) = tx
        .query_row(
            "SELECT amount_minor,payment_method,category FROM expenses WHERE id=?1 AND store_id=?2",
            params![input.expense_id, session.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That expense is not available in this store.".to_owned())?;
    if source_amount <= 0 {
        return Err("Only a positive recorded expense can be reimbursed.".to_owned());
    }
    let already_returned: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM expense_reimbursements WHERE expense_id=?1 AND store_id=?2",
            params![input.expense_id, session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let remaining = source_amount.checked_sub(already_returned).ok_or_else(|| {
        "Recorded reimbursements exceed the original expense. Contact support.".to_owned()
    })?;
    if input.amount_minor > remaining {
        return Err(format!(
            "The reimbursement is larger than the remaining amount of {} minor units.",
            remaining.max(0)
        ));
    }

    let mut location_name = None;
    let mut location_type = None;
    if input.return_method == "cash" {
        let location_id = input
            .cash_location_id
            .as_deref()
            .expect("validated cash location");
        let location: Option<(String, String, Option<String>, i64)> = tx
            .query_row(
                "SELECT name,location_type,station_key,is_active FROM cash_locations
                 WHERE id=?1 AND store_id=?2 AND business_id=?3",
                params![location_id, session.store_id, session.business_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((name, kind, station_key, is_active)) = location else {
            return Err("Choose a cash location belonging to this store.".to_owned());
        };
        if is_active == 0 {
            return Err("That cash location is inactive.".to_owned());
        }
        let opening_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM cash_location_events WHERE cash_location_id=?1 AND event_type='opening')",
                [location_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !opening_exists {
            return Err(
                "Record an opening count for this cash location before receiving money.".to_owned(),
            );
        }
        if kind == "register" {
            let assigned_station =
                crate::stations::station_key_for_device(&tx, device_id, session)?;
            if station_key.as_deref() != Some(assigned_station.as_str()) {
                return Err(
                    "Returned cash can only be recorded at the register assigned to this device."
                        .to_owned(),
                );
            }
            let shift_id = input.cash_shift_id.as_deref().ok_or_else(|| {
                "Open the receiving register shift and refresh its destination before recording returned cash.".to_owned()
            })?;
            let shift_open: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM cash_shifts WHERE id=?1 AND store_id=?2 AND station_key=?3 AND cash_location_id=?4 AND status='open')",
                    params![shift_id,session.store_id,assigned_station,location_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !shift_open {
                return Err("The receiving register shift changed or closed. Refresh the cash destination before retrying.".to_owned());
            }
        } else if input.cash_shift_id.is_some() {
            return Err(
                "Safe and petty-cash receipts are not attached to a register shift.".to_owned(),
            );
        }
        location_name = Some(name);
        location_type = Some(kind);
    }

    // Ensure the original expense has a journal before posting an actual-tender reimbursement.
    accounting::post_expense_for_session(&tx, session, &input.expense_id)?;
    let original_journal_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM accounting_journals WHERE business_id=?1 AND store_id=?2 AND source_type='expense' AND source_id=?3)",
            params![session.business_id,session.store_id,input.expense_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !original_journal_exists {
        return Err("The original expense accounting entry is missing.".to_owned());
    }

    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    tx.execute(
        "INSERT INTO expense_reimbursements(id,business_id,store_id,expense_id,amount_minor,return_method,return_reference,reason,cash_location_id,cash_shift_id,idempotency_key,recorded_by,returned_at,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
        params![id,session.business_id,session.store_id,input.expense_id,input.amount_minor,input.return_method,input.return_reference,input.reason,input.cash_location_id,input.cash_shift_id,input.idempotency_key,session.user_id,timestamp],
    )
    .map_err(|error| {
        if error.to_string().contains("idx_expense_reimbursements_reference")
            || error.to_string().contains("expense_reimbursements.store_id, expense_reimbursements.return_method, expense_reimbursements.return_reference")
        {
            "That returned-payment reference is already recorded for this store.".to_owned()
        } else {
            format!("Could not record the expense reimbursement: {error}")
        }
    })?;

    if input.return_method == "cash" {
        let note = format!("Returned expense · {category}");
        if let Some(shift_id) = input.cash_shift_id.as_deref() {
            crate::cash_drawer::record_expense_reimbursement_paid_in(
                &tx,
                session,
                &id,
                shift_id,
                input.amount_minor,
                &note,
                &timestamp,
            )?;
        }
        crate::cash_locations::record_expense_reimbursement_cash_inflow(
            &tx,
            session,
            &id,
            input
                .cash_location_id
                .as_deref()
                .expect("validated cash location"),
            input.cash_shift_id.as_deref(),
            input.amount_minor,
            &note,
            &timestamp,
        )?;
    }
    accounting::post_expense_reimbursement_for_session(&tx, session, &id)?;
    add_audit_event(
        &tx,
        session,
        "expense_reimbursed",
        "expense_reimbursement",
        Some(&id),
        serde_json::json!({
            "expenseId": input.expense_id,
            "amountMinor": input.amount_minor,
            "originalPaymentMethod": source_method,
            "returnMethod": input.return_method,
            "returnReference": input.return_reference,
            "cashLocationId": input.cash_location_id,
            "cashLocationName": location_name,
            "cashLocationType": location_type,
            "cashShiftId": input.cash_shift_id,
            "reason": input.reason,
            "idempotencyKey": input.idempotency_key,
        }),
    )?;
    tx.commit().map_err(|error| error.to_string())?;

    Ok(ExpenseReimbursement {
        id,
        expense_id: input.expense_id,
        amount_minor: input.amount_minor,
        return_method: input.return_method,
        return_reference: input.return_reference,
        reason: input.reason,
        cash_location_id: input.cash_location_id,
        cash_shift_id: input.cash_shift_id,
        returned_at: timestamp,
        recorded_by: session.full_name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionUser {
        SessionUser {
            user_id: "user-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Owner One".into(),
            username: "owner".into(),
            role_key: "owner".into(),
            role_name: "Owner".into(),
            workspace_mode: "management".into(),
            is_owner: true,
            permissions: vec!["expenses:write".into()],
            business_name: "Test".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        }
    }

    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE businesses(id TEXT PRIMARY KEY,name TEXT,currency_code TEXT);
             INSERT INTO businesses VALUES('business-a','Test','GHS');
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT);
             INSERT INTO stores VALUES('store-a','business-a');
             CREATE TABLE users(id TEXT PRIMARY KEY,full_name TEXT);
             INSERT INTO users VALUES('user-a','Owner One');
             CREATE TABLE sales(id TEXT PRIMARY KEY,store_id TEXT,subtotal_minor INTEGER,discount_minor INTEGER,tax_minor INTEGER,total_minor INTEGER,created_at TEXT);
             CREATE TABLE payments(id TEXT PRIMARY KEY,sale_id TEXT,method TEXT,amount_minor INTEGER);
             CREATE TABLE sale_items(id TEXT PRIMARY KEY,sale_id TEXT,quantity INTEGER,unit_cost_minor_snapshot INTEGER);
             CREATE TABLE purchases(id TEXT PRIMARY KEY,store_id TEXT,total_minor INTEGER,received_at TEXT,reference TEXT);
             CREATE TABLE purchase_items(id TEXT PRIMARY KEY,purchase_id TEXT,quantity INTEGER,unit_cost_minor INTEGER,line_total_minor INTEGER);
             CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,user_id TEXT,category TEXT,description TEXT,amount_minor INTEGER,payment_method TEXT,occurred_at TEXT,created_at TEXT);
             CREATE TABLE audit_events(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,user_id TEXT,action TEXT,entity_type TEXT,entity_id TEXT,details_json TEXT,created_at TEXT);
             CREATE TABLE expense_reimbursements(
                 id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,expense_id TEXT,amount_minor INTEGER,
                 return_method TEXT,return_reference TEXT,reason TEXT,cash_location_id TEXT,cash_shift_id TEXT,
                 idempotency_key TEXT,recorded_by TEXT,returned_at TEXT,created_at TEXT,
                 UNIQUE(store_id,idempotency_key));
             CREATE TABLE register_stations(business_id TEXT,store_id TEXT,station_key TEXT,display_name TEXT,is_active INTEGER,created_at TEXT,updated_at TEXT,PRIMARY KEY(store_id,station_key));
             CREATE TABLE station_devices(device_id TEXT,business_id TEXT,store_id TEXT,station_key TEXT,assigned_by TEXT,assigned_at TEXT,updated_at TEXT,PRIMARY KEY(device_id,store_id));
             CREATE TABLE cash_locations(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,name TEXT,location_type TEXT,station_key TEXT,is_active INTEGER);
             CREATE TABLE cash_location_events(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,cash_location_id TEXT,shift_id TEXT,event_type TEXT,direction TEXT,amount_minor INTEGER,source_type TEXT,source_id TEXT,transfer_id TEXT,idempotency_key TEXT,note TEXT,review_status TEXT,actor_user_id TEXT,effective_at TEXT,created_at TEXT);
             CREATE TABLE cash_shifts(id TEXT PRIMARY KEY,store_id TEXT,station_key TEXT,cash_location_id TEXT,status TEXT);
             CREATE TABLE cash_movements(id TEXT PRIMARY KEY,shift_id TEXT,store_id TEXT,user_id TEXT,movement_type TEXT,amount_minor INTEGER,note TEXT,created_at TEXT,source_type TEXT,source_id TEXT);",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0020_accounting_journals.sql"))
            .unwrap();
        db.execute_batch(include_str!("../migrations/0021_accounting_purchases.sql"))
            .unwrap();
        db.execute(
            "INSERT INTO expenses VALUES('expense-a','store-a','user-a','Utilities','Power',1000,'bank_transfer','2026-10-30T09:00:00.000Z','2026-10-30T09:00:00.000Z')",
            [],
        )
        .unwrap();
        db
    }

    fn input(amount_minor: i64, idempotency_key: &str) -> ExpenseReimbursementInput {
        ExpenseReimbursementInput {
            expense_id: "expense-a".into(),
            amount_minor,
            return_method: "cash".into(),
            return_reference: None,
            reason: "Duplicate utility payment returned".into(),
            cash_location_id: None,
            cash_shift_id: None,
            idempotency_key: idempotency_key.into(),
        }
    }

    fn configure_register(db: &Connection, shift_status: &str, station_key: &str) {
        db.execute(
            "INSERT INTO register_stations VALUES('business-a','store-a','main','Main register',1,'2026-10-30T00:00:00Z','2026-10-30T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO station_devices VALUES('device-a','business-a','store-a','main','user-a','2026-10-30T00:00:00Z','2026-10-30T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_locations VALUES('register-a','business-a','store-a','Main register','register',?1,1)",
            [station_key],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_location_events(id,business_id,store_id,cash_location_id,event_type,direction,amount_minor,source_type,source_id,actor_user_id,effective_at,created_at) VALUES('register-opening','business-a','store-a','register-a','opening','in',500,'opening','register-a','user-a','2026-10-30T00:00:00Z','2026-10-30T00:00:00Z')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO cash_shifts VALUES('shift-a','store-a',?1,'register-a',?2)",
            params![station_key, shift_status],
        )
        .unwrap();
    }

    #[test]
    fn reimbursement_authority_is_limited_to_managers_administrators_and_owners() {
        let mut user = session();
        assert!(can_record_reimbursement(&user));
        user.role_key = "manager".into();
        user.is_owner = false;
        assert!(can_record_reimbursement(&user));
        user.role_key = "admin".into();
        assert!(can_record_reimbursement(&user));
        user.role_key = "cashier".into();
        assert!(!can_record_reimbursement(&user));
    }

    #[test]
    fn rejects_cash_without_an_explicit_location_before_any_writes() {
        let mut db = fixture();
        let error = record_reimbursement_with_session(
            input(300, "return-key"),
            &session(),
            "device-a",
            &mut db,
        )
        .unwrap_err();
        assert!(error.contains("tracked location"));
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM expense_reimbursements", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        let journals: i64 = db
            .query_row("SELECT COUNT(*) FROM accounting_journals", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(journals, 0);
    }

    #[test]
    fn non_cash_return_replays_same_request_and_rejects_over_return() {
        let mut db = fixture();
        let mut request = input(300, "return-key");
        request.return_method = "bank_transfer".into();
        request.return_reference = Some("BANK-RETURN-300".into());
        let first =
            record_reimbursement_with_session(request.clone(), &session(), "device-a", &mut db)
                .unwrap();
        let replay =
            record_reimbursement_with_session(request, &session(), "device-a", &mut db).unwrap();
        assert_eq!(first.id, replay.id);
        let mut mismatched_retry = input(301, "return-key");
        mismatched_retry.return_method = "bank_transfer".into();
        mismatched_retry.return_reference = Some("BANK-RETURN-300".into());
        assert!(record_reimbursement_with_session(
            mismatched_retry,
            &session(),
            "device-a",
            &mut db
        )
        .unwrap_err()
        .contains("different details"));
        let over = {
            let mut value = input(701, "over-key");
            value.return_method = "bank_transfer".into();
            value.return_reference = Some("BANK-RETURN-OVER".into());
            record_reimbursement_with_session(value, &session(), "device-a", &mut db)
        };
        assert!(over.unwrap_err().contains("remaining amount"));
        let records: i64 = db
            .query_row("SELECT COUNT(*) FROM expense_reimbursements", [], |row| {
                row.get(0)
            })
            .unwrap();
        let journals: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM accounting_journals WHERE source_type='reversal'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((records, journals), (1, 1));
    }

    #[test]
    fn register_cash_reimbursement_updates_only_the_current_open_shift() {
        let mut db = fixture();
        configure_register(&db, "open", "main");
        let mut request = input(300, "register-return");
        request.cash_location_id = Some("register-a".into());
        request.cash_shift_id = Some("shift-a".into());

        let returned =
            record_reimbursement_with_session(request.clone(), &session(), "device-a", &mut db)
                .unwrap();
        assert_eq!(returned.cash_shift_id.as_deref(), Some("shift-a"));
        let movement: (i64, String) = db.query_row(
            "SELECT amount_minor,source_id FROM cash_movements WHERE shift_id='shift-a' AND movement_type='paid_in'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(movement, (300, returned.id.clone()));
        let location_event: (String, i64) = db.query_row(
            "SELECT shift_id,amount_minor FROM cash_location_events WHERE event_type='expense_reimbursement' AND source_id=?1",
            [&returned.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(location_event, ("shift-a".into(), 300));

        // A lost response may cause a retry after the shift has closed. Replay
        // the committed reimbursement without requiring the old shift to reopen.
        db.execute(
            "UPDATE cash_shifts SET status='closed' WHERE id='shift-a'",
            [],
        )
        .unwrap();
        let replay =
            record_reimbursement_with_session(request, &session(), "device-a", &mut db).unwrap();
        assert_eq!(replay.id, returned.id);
        let linked_effects: i64 = db
            .query_row(
                "SELECT (SELECT COUNT(*) FROM cash_movements WHERE shift_id='shift-a' AND movement_type='paid_in' AND source_id=?1) + (SELECT COUNT(*) FROM cash_location_events WHERE event_type='expense_reimbursement' AND source_id=?1)",
                [&returned.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked_effects, 2);
    }

    #[test]
    fn register_cash_reimbursement_rejects_closed_shift_without_posting() {
        let mut db = fixture();
        configure_register(&db, "closed", "main");
        let mut request = input(300, "closed-register-return");
        request.cash_location_id = Some("register-a".into());
        request.cash_shift_id = Some("shift-a".into());

        let error = record_reimbursement_with_session(request, &session(), "device-a", &mut db)
            .unwrap_err();
        assert!(error.contains("shift changed or closed"));
        let reimbursement_count: i64 = db
            .query_row("SELECT COUNT(*) FROM expense_reimbursements", [], |row| {
                row.get(0)
            })
            .unwrap();
        let cash_count: i64 = db
            .query_row("SELECT COUNT(*) FROM cash_movements", [], |row| row.get(0))
            .unwrap();
        let journal_count: i64 = db
            .query_row("SELECT COUNT(*) FROM accounting_journals", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((reimbursement_count, cash_count, journal_count), (0, 0, 0));
    }

    #[test]
    fn register_cash_reimbursement_rejects_a_different_device_station() {
        let mut db = fixture();
        configure_register(&db, "open", "other-station");
        let mut request = input(300, "wrong-station-return");
        request.cash_location_id = Some("register-a".into());
        request.cash_shift_id = Some("shift-a".into());

        let error = record_reimbursement_with_session(request, &session(), "device-a", &mut db)
            .unwrap_err();
        assert!(error.contains("assigned to this device"));
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM expense_reimbursements", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
