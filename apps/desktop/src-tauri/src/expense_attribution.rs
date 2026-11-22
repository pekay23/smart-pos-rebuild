// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclassifyExpenseCostCenterInput {
    pub expense_id: String,
    pub expected_revision: i64,
    pub cost_center_kind: String,
    pub station_key: Option<String>,
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseCostCenterReclassification {
    pub id: String,
    pub expense_id: String,
    pub revision: i64,
    pub previous_kind: String,
    pub previous_station_key: Option<String>,
    pub previous_station_name_snapshot: Option<String>,
    pub next_kind: String,
    pub next_station_key: Option<String>,
    pub next_station_name_snapshot: Option<String>,
    pub reason: String,
    pub recorded_at: String,
    pub actor_user_id: String,
    pub actor_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseCostCenterHistory {
    pub current_revision: i64,
    pub current_kind: String,
    pub current_station_key: Option<String>,
    pub current_station_name_snapshot: Option<String>,
    pub rows: Vec<ExpenseCostCenterReclassification>,
    pub reimbursements: Vec<ExpenseReturnHistoryRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseReturnHistoryRow {
    pub amount_minor: i64,
    pub returned_at: String,
    pub recorded_by: String,
}

fn latest_event(
    connection: &Connection,
    store_id: &str,
    expense_id: &str,
) -> Result<Option<ExpenseCostCenterReclassification>, String> {
    connection
        .query_row(
            "SELECT r.id,r.expense_id,r.revision,r.previous_kind,r.previous_station_key,
                    r.previous_station_name_snapshot,r.next_kind,r.next_station_key,
                    r.next_station_name_snapshot,r.reason,r.recorded_at,r.actor_user_id,u.full_name
             FROM expense_cost_center_reclassifications r
             JOIN users u ON u.id=r.actor_user_id
             WHERE r.store_id=?1 AND r.expense_id=?2 ORDER BY r.revision DESC LIMIT 1",
            params![store_id, expense_id],
            |row| {
                Ok(ExpenseCostCenterReclassification {
                    id: row.get(0)?,
                    expense_id: row.get(1)?,
                    revision: row.get(2)?,
                    previous_kind: row.get(3)?,
                    previous_station_key: row.get(4)?,
                    previous_station_name_snapshot: row.get(5)?,
                    next_kind: row.get(6)?,
                    next_station_key: row.get(7)?,
                    next_station_name_snapshot: row.get(8)?,
                    reason: row.get(9)?,
                    recorded_at: row.get(10)?,
                    actor_user_id: row.get(11)?,
                    actor_name: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn saved_by_key(
    connection: &Connection,
    store_id: &str,
    idempotency_key: &str,
) -> Result<Option<(ExpenseCostCenterReclassification, i64, String)>, String> {
    connection
        .query_row(
            "SELECT r.id,r.expense_id,r.revision,r.previous_kind,r.previous_station_key,
                    r.previous_station_name_snapshot,r.next_kind,r.next_station_key,
                    r.next_station_name_snapshot,r.reason,r.recorded_at,r.actor_user_id,u.full_name,
                    r.expected_revision,r.idempotency_key
             FROM expense_cost_center_reclassifications r
             JOIN users u ON u.id=r.actor_user_id
             WHERE r.store_id=?1 AND r.idempotency_key=?2",
            params![store_id, idempotency_key],
            |row| {
                Ok((
                    ExpenseCostCenterReclassification {
                        id: row.get(0)?,
                        expense_id: row.get(1)?,
                        revision: row.get(2)?,
                        previous_kind: row.get(3)?,
                        previous_station_key: row.get(4)?,
                        previous_station_name_snapshot: row.get(5)?,
                        next_kind: row.get(6)?,
                        next_station_key: row.get(7)?,
                        next_station_name_snapshot: row.get(8)?,
                        reason: row.get(9)?,
                        recorded_at: row.get(10)?,
                        actor_user_id: row.get(11)?,
                        actor_name: row.get(12)?,
                    },
                    row.get(13)?,
                    row.get(14)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn request_matches(
    saved: &ExpenseCostCenterReclassification,
    expected_revision: i64,
    input: &ReclassifyExpenseCostCenterInput,
) -> bool {
    saved.expense_id == input.expense_id
        && expected_revision.checked_add(1) == Some(saved.revision)
        && saved.next_kind == input.cost_center_kind
        && saved.next_station_key == input.station_key
        && saved.reason == input.reason
}

#[tauri::command]
pub fn reclassify_expense_cost_center(
    input: ReclassifyExpenseCostCenterInput,
    state: State<'_, AppState>,
) -> Result<ExpenseCostCenterReclassification, String> {
    reclassify_from_authenticated_state(input, &state)
}

fn reclassify_from_authenticated_state(
    input: ReclassifyExpenseCostCenterInput,
    state: &AppState,
) -> Result<ExpenseCostCenterReclassification, String> {
    let session = active_session(&state, "expenses:reclassify")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    reclassify_with_session(input, &session, &mut connection)
}

fn reclassify_with_session(
    mut input: ReclassifyExpenseCostCenterInput,
    session: &SessionUser,
    connection: &mut Connection,
) -> Result<ExpenseCostCenterReclassification, String> {
    input.expense_id = input.expense_id.trim().to_owned();
    input.cost_center_kind = input.cost_center_kind.trim().to_owned();
    input.station_key = input
        .station_key
        .take()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());
    input.reason = input.reason.trim().to_owned();
    input.idempotency_key = input.idempotency_key.trim().to_owned();
    if input.expense_id.is_empty() || input.expense_id.len() > 80 {
        return Err("The expense identifier is invalid.".to_owned());
    }
    if input.expected_revision < 0 {
        return Err("Refresh the expense before correcting its cost center.".to_owned());
    }
    if !matches!(
        input.cost_center_kind.as_str(),
        "station_direct" | "store_overhead" | "legacy_unassigned"
    ) {
        return Err("Choose a register, store overhead, or not allocated.".to_owned());
    }
    if (input.cost_center_kind == "station_direct") != input.station_key.is_some() {
        return Err(
            "Choose an active station for direct costs, or choose a store cost center.".to_owned(),
        );
    }
    if input.station_key.as_ref().is_some_and(|key| key.len() > 80) {
        return Err("Choose a valid register station.".to_owned());
    }
    if input.reason.chars().count() < 5 || input.reason.chars().count() > 500 {
        return Err("Explain this correction in 5 to 500 characters.".to_owned());
    }
    if input.idempotency_key.is_empty() || input.idempotency_key.len() > 100 {
        return Err("The correction retry key is invalid.".to_owned());
    }

    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some((saved, expected_revision, saved_key)) =
        saved_by_key(&tx, &session.store_id, &input.idempotency_key)?
    {
        if saved_key == input.idempotency_key
            && expected_revision == input.expected_revision
            && request_matches(&saved, input.expected_revision, &input)
        {
            return Ok(saved);
        }
        return Err("This correction retry key was already used for different details.".to_owned());
    }

    let source: Option<(String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT cost_center_kind,cost_center_station_key,cost_center_station_name_snapshot
             FROM expenses WHERE id=?1 AND store_id=?2",
            params![input.expense_id, session.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (previous_kind, previous_station_key, previous_station_name_snapshot) =
        source.ok_or_else(|| "That expense is not available in this store.".to_owned())?;
    let latest = latest_event(&tx, &session.store_id, &input.expense_id)?;
    let current_revision = latest.as_ref().map_or(0, |event| event.revision);
    if current_revision != input.expected_revision {
        return Err(
            "This expense attribution changed in another session. Refresh and try again."
                .to_owned(),
        );
    }
    let (previous_kind, previous_station_key, previous_station_name_snapshot) = latest.map_or(
        (
            previous_kind,
            previous_station_key,
            previous_station_name_snapshot,
        ),
        |event| {
            (
                event.next_kind,
                event.next_station_key,
                event.next_station_name_snapshot,
            )
        },
    );
    if previous_kind == input.cost_center_kind && previous_station_key == input.station_key {
        return Err("Choose a different cost center to record a correction.".to_owned());
    }
    let next_station_name_snapshot = if let Some(station_key) = input.station_key.as_deref() {
        Some(
            tx.query_row(
                "SELECT display_name FROM register_stations
                 WHERE business_id=?1 AND store_id=?2 AND station_key=?3 AND is_active=1",
                params![session.business_id, session.store_id, station_key],
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
    let revision = current_revision
        .checked_add(1)
        .ok_or_else(|| "This expense attribution has reached its revision limit.".to_owned())?;
    let recorded_at = now();
    tx.execute(
        "INSERT INTO expense_cost_center_reclassifications
         (id,business_id,store_id,expense_id,revision,expected_revision,previous_kind,previous_station_key,
          previous_station_name_snapshot,next_kind,next_station_key,next_station_name_snapshot,reason,
          idempotency_key,actor_user_id,recorded_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            id, session.business_id, session.store_id, input.expense_id, revision, input.expected_revision,
            previous_kind, previous_station_key, previous_station_name_snapshot,
            input.cost_center_kind, input.station_key, next_station_name_snapshot, input.reason,
            input.idempotency_key, session.user_id, recorded_at,
        ],
    )
    .map_err(|error| format!("Could not record expense attribution correction: {error}"))?;
    add_audit_event(
        &tx,
        session,
        "expense_cost_center_reclassified",
        "expense",
        Some(&input.expense_id),
        serde_json::json!({
            "reclassificationId": id,
            "revision": revision,
            "previousKind": previous_kind,
            "previousStationKey": previous_station_key,
            "previousStationName": previous_station_name_snapshot,
            "nextKind": input.cost_center_kind,
            "nextStationKey": input.station_key,
            "nextStationName": next_station_name_snapshot,
            "reason": input.reason,
            "effectiveAt": "original expense and reimbursement dates",
        }),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(ExpenseCostCenterReclassification {
        id,
        expense_id: input.expense_id,
        revision,
        previous_kind,
        previous_station_key,
        previous_station_name_snapshot,
        next_kind: input.cost_center_kind,
        next_station_key: input.station_key,
        next_station_name_snapshot,
        reason: input.reason,
        recorded_at,
        actor_user_id: session.user_id.clone(),
        actor_name: session.full_name.clone(),
    })
}

#[tauri::command]
pub fn expense_cost_center_history(
    expense_id: String,
    state: State<'_, AppState>,
) -> Result<ExpenseCostCenterHistory, String> {
    let session = active_session(&state, "expenses:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let expense_id = expense_id.trim();
    let source: Option<(String, Option<String>, Option<String>)> = connection
        .query_row(
            "SELECT cost_center_kind,cost_center_station_key,cost_center_station_name_snapshot
             FROM expenses WHERE id=?1 AND store_id=?2",
            params![expense_id, session.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (source_kind, source_key, source_name) =
        source.ok_or_else(|| "That expense is not available in this store.".to_owned())?;
    let latest = latest_event(&connection, &session.store_id, expense_id)?;
    let (current_kind, current_station_key, current_station_name_snapshot, current_revision) =
        latest
            .as_ref()
            .map_or((source_kind, source_key, source_name, 0), |event| {
                (
                    event.next_kind.clone(),
                    event.next_station_key.clone(),
                    event.next_station_name_snapshot.clone(),
                    event.revision,
                )
            });
    let mut statement = connection
        .prepare(
            "SELECT r.id,r.expense_id,r.revision,r.previous_kind,r.previous_station_key,
                    r.previous_station_name_snapshot,r.next_kind,r.next_station_key,
                    r.next_station_name_snapshot,r.reason,r.recorded_at,r.actor_user_id,u.full_name
             FROM expense_cost_center_reclassifications r JOIN users u ON u.id=r.actor_user_id
             WHERE r.store_id=?1 AND r.expense_id=?2 ORDER BY r.revision",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![session.store_id, expense_id], |row| {
            Ok(ExpenseCostCenterReclassification {
                id: row.get(0)?,
                expense_id: row.get(1)?,
                revision: row.get(2)?,
                previous_kind: row.get(3)?,
                previous_station_key: row.get(4)?,
                previous_station_name_snapshot: row.get(5)?,
                next_kind: row.get(6)?,
                next_station_key: row.get(7)?,
                next_station_name_snapshot: row.get(8)?,
                reason: row.get(9)?,
                recorded_at: row.get(10)?,
                actor_user_id: row.get(11)?,
                actor_name: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT r.amount_minor,r.returned_at,u.full_name
             FROM expense_reimbursements r JOIN users u ON u.id=r.recorded_by
             WHERE r.store_id=?1 AND r.expense_id=?2 ORDER BY r.returned_at,r.id",
        )
        .map_err(|error| error.to_string())?;
    let reimbursements = statement
        .query_map(params![session.store_id, expense_id], |row| {
            Ok(ExpenseReturnHistoryRow {
                amount_minor: row.get(0)?,
                returned_at: row.get(1)?,
                recorded_by: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(ExpenseCostCenterHistory {
        current_revision,
        current_kind,
        current_station_key,
        current_station_name_snapshot,
        rows,
        reimbursements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::{Mutex, RwLock},
    };

    fn session() -> SessionUser {
        SessionUser {
            user_id: "owner-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Owner One".into(),
            username: "owner".into(),
            role_key: "owner".into(),
            role_name: "Owner".into(),
            workspace_mode: "management".into(),
            is_owner: true,
            permissions: vec!["expenses:reclassify".into()],
            business_name: "Test".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        }
    }

    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch(
            "CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE businesses(id TEXT PRIMARY KEY,name TEXT,currency_code TEXT,owner_user_id TEXT);
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT,name TEXT,is_active INTEGER);
             CREATE TABLE users(id TEXT PRIMARY KEY,business_id TEXT,role_id TEXT,full_name TEXT,username TEXT,is_active INTEGER);
             CREATE TABLE roles(id TEXT PRIMARY KEY,business_id TEXT,role_key TEXT,display_name TEXT,permissions_json TEXT,workspace_mode TEXT);
             CREATE TABLE register_stations(business_id TEXT,store_id TEXT,station_key TEXT,display_name TEXT,is_active INTEGER);
             CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,cost_center_kind TEXT,cost_center_station_key TEXT,cost_center_station_name_snapshot TEXT);
             CREATE TABLE audit_events(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,user_id TEXT,action TEXT,entity_type TEXT,entity_id TEXT,details_json TEXT,created_at TEXT);
             CREATE TABLE expense_reimbursements(id TEXT PRIMARY KEY,store_id TEXT,expense_id TEXT,amount_minor INTEGER,returned_at TEXT,recorded_by TEXT);
             CREATE TABLE accounting_journals(id TEXT PRIMARY KEY,store_id TEXT);
             CREATE TABLE cash_location_events(id TEXT PRIMARY KEY,store_id TEXT);
             CREATE TABLE cash_movements(id TEXT PRIMARY KEY,store_id TEXT);
             CREATE TABLE cash_shifts(id TEXT PRIMARY KEY,z_report_json TEXT);
             INSERT INTO businesses VALUES('business-a','Test','GHS','owner-a');
             INSERT INTO stores VALUES('store-a','business-a','Store',1);
             INSERT INTO users VALUES('owner-a','business-a','owner-role','Owner One','owner',1);
             INSERT INTO roles VALUES('owner-role','business-a','owner','Owner','[]','management');
             INSERT INTO register_stations VALUES('business-a','store-a','station-a','Main',1);
             INSERT INTO register_stations VALUES('business-a','store-a','station-b','Back counter',1);
             INSERT INTO expenses VALUES('expense-a','store-a','station_direct','station-a','Main');
             INSERT INTO expense_reimbursements VALUES('return-a','store-a','expense-a',200,'2026-10-01T12:00:00Z','owner-a');
             INSERT INTO accounting_journals VALUES('journal-sentinel','store-a');
             INSERT INTO cash_location_events VALUES('location-sentinel','store-a');
             INSERT INTO cash_movements VALUES('movement-sentinel','store-a');
             INSERT INTO cash_shifts VALUES('shift-sentinel','z-unchanged');",
        )
        .unwrap();
        db.execute_batch(include_str!(
            "../migrations/0032_expense_cost_center_reclassifications.sql"
        ))
        .unwrap();
        db
    }

    fn command_state(connection: Connection, cached: SessionUser) -> AppState {
        AppState {
            data_dir: PathBuf::new(),
            device_id: "test-device".into(),
            db: Mutex::new(connection),
            active_session: Mutex::new(Some(cached)),
            session_gate: RwLock::new(()),
            passkey_operation: Mutex::new(()),
        }
    }

    fn input(
        expected_revision: i64,
        kind: &str,
        station_key: Option<&str>,
        idempotency_key: &str,
    ) -> ReclassifyExpenseCostCenterInput {
        ReclassifyExpenseCostCenterInput {
            expense_id: "expense-a".into(),
            expected_revision,
            cost_center_kind: kind.into(),
            station_key: station_key.map(str::to_owned),
            reason: "Corrected register attribution".into(),
            idempotency_key: idempotency_key.into(),
        }
    }

    #[test]
    fn command_rejects_cashier_using_persisted_role_even_if_cached_permissions_are_stale() {
        let db = fixture();
        db.execute(
            "UPDATE roles SET role_key='cashier',display_name='Cashier',permissions_json='[]' WHERE id='owner-role'",
            [],
        )
        .unwrap();
        let mut cached = session();
        cached.role_key = "cashier".into();
        let state = command_state(db, cached);

        let error = reclassify_from_authenticated_state(
            input(0, "store_overhead", None, "cashier-attempt"),
            &state,
        )
        .unwrap_err();
        assert!(error.contains("does not have permission"));

        let connection = state.db.lock().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM expense_cost_center_reclassifications",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn command_rejects_expense_from_another_store_without_writes() {
        let db = fixture();
        db.execute(
            "INSERT INTO stores VALUES('store-b','business-a','Other store',1)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO expenses VALUES('expense-b','store-b','station_direct','station-a','Main')",
            [],
        )
        .unwrap();
        let state = command_state(db, session());
        let mut request = input(0, "store_overhead", None, "cross-store-attempt");
        request.expense_id = "expense-b".into();

        let error = reclassify_from_authenticated_state(request, &state).unwrap_err();
        assert!(error.contains("not available in this store"));

        let connection = state.db.lock().unwrap();
        let corrections: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM expense_cost_center_reclassifications",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let audits: i64 = connection
            .query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!((corrections, audits), (0, 0));
    }

    #[test]
    fn command_rejects_inactive_station_without_writes() {
        let db = fixture();
        db.execute(
            "UPDATE register_stations SET is_active=0 WHERE store_id='store-a' AND station_key='station-b'",
            [],
        )
        .unwrap();
        let state = command_state(db, session());

        let error = reclassify_from_authenticated_state(
            input(
                0,
                "station_direct",
                Some("station-b"),
                "inactive-station-attempt",
            ),
            &state,
        )
        .unwrap_err();
        assert!(error.contains("active register station"));

        let connection = state.db.lock().unwrap();
        let corrections: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM expense_cost_center_reclassifications",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let audits: i64 = connection
            .query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!((corrections, audits), (0, 0));
    }

    #[test]
    fn command_allows_custom_role_only_when_database_permission_is_explicitly_granted() {
        let db = fixture();
        db.execute(
            "UPDATE roles SET role_key='expense-reviewer',display_name='Expense reviewer',permissions_json='[\"expenses:read\",\"expenses:reclassify\"]' WHERE id='owner-role'",
            [],
        )
        .unwrap();
        db.execute("UPDATE businesses SET owner_user_id=NULL", [])
            .unwrap();
        let mut cached = session();
        cached.role_key = "expense-reviewer".into();
        cached.permissions.clear();
        cached.is_owner = false;
        let state = command_state(db, cached);

        let event = reclassify_from_authenticated_state(
            input(0, "store_overhead", None, "custom-role-correction"),
            &state,
        )
        .unwrap();
        assert_eq!(event.actor_user_id, "owner-a");
        assert_eq!(event.next_kind, "store_overhead");

        let connection = state.db.lock().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM expense_cost_center_reclassifications WHERE actor_user_id='owner-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn reclassification_is_append_only_retry_safe_and_moves_the_whole_expense_family() {
        let mut db = fixture();
        let first = reclassify_with_session(
            input(0, "station_direct", Some("station-b"), "reclass-one"),
            &session(),
            &mut db,
        )
        .unwrap();
        let replay = reclassify_with_session(
            input(0, "station_direct", Some("station-b"), "reclass-one"),
            &session(),
            &mut db,
        )
        .unwrap();
        assert_eq!(first.id, replay.id);
        assert_eq!(
            (
                first.revision,
                first.previous_station_key.as_deref(),
                first.next_station_key.as_deref()
            ),
            (1, Some("station-a"), Some("station-b"))
        );

        let mut changed_retry = input(0, "station_direct", Some("station-b"), "reclass-one");
        changed_retry.reason = "Different correction reason".into();
        assert!(reclassify_with_session(changed_retry, &session(), &mut db)
            .unwrap_err()
            .contains("different details"));
        assert!(reclassify_with_session(
            input(0, "store_overhead", None, "reclass-stale"),
            &session(),
            &mut db,
        )
        .unwrap_err()
        .contains("changed in another session"));

        let (expense_center, returns, history, audit, journals, location_events, movements, z_snapshot):
            (String, i64, i64, i64, i64, i64, i64, String) = db
            .query_row(
                "SELECT e.cost_center_kind,
                        (SELECT COUNT(*) FROM expense_reimbursements WHERE expense_id=e.id),
                        (SELECT COUNT(*) FROM expense_cost_center_reclassifications WHERE expense_id=e.id),
                        (SELECT COUNT(*) FROM audit_events WHERE entity_id=e.id),
                        (SELECT COUNT(*) FROM accounting_journals),
                        (SELECT COUNT(*) FROM cash_location_events),
                        (SELECT COUNT(*) FROM cash_movements),
                        (SELECT z_report_json FROM cash_shifts WHERE id='shift-sentinel')
                 FROM expenses e WHERE e.id='expense-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
            )
            .unwrap();
        assert_eq!(expense_center, "station_direct");
        assert_eq!((returns, history, audit), (1, 1, 1));
        assert_eq!((journals, location_events, movements), (1, 1, 1));
        assert_eq!(z_snapshot, "z-unchanged");
        assert!(db
            .execute(
                "DELETE FROM expense_cost_center_reclassifications WHERE expense_id='expense-a'",
                [],
            )
            .is_err());
    }

    #[test]
    fn reclassification_rejects_invalid_station_and_allows_revisioned_overhead_correction() {
        let mut db = fixture();
        assert!(reclassify_with_session(
            input(0, "station_direct", Some("missing"), "bad-station"),
            &session(),
            &mut db,
        )
        .unwrap_err()
        .contains("active register station"));

        let first = reclassify_with_session(
            input(0, "store_overhead", None, "to-overhead"),
            &session(),
            &mut db,
        )
        .unwrap();
        let second = reclassify_with_session(
            input(1, "legacy_unassigned", None, "to-unassigned"),
            &session(),
            &mut db,
        )
        .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(
            (
                second.revision,
                second.previous_kind.as_str(),
                second.next_kind.as_str()
            ),
            (2, "store_overhead", "legacy_unassigned")
        );
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM expense_cost_center_reclassifications",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
}
