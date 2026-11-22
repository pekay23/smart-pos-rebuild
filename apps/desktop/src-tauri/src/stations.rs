// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterStation {
    pub station_key: String,
    pub display_name: String,
    pub is_active: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationWorkspace {
    pub current_station_key: String,
    pub current_display_name: String,
    pub stations: Vec<RegisterStation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateStationInput {
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameStationInput {
    pub station_key: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignStationInput {
    pub station_key: String,
}

fn timestamp() -> String {
    Utc::now().to_rfc3339()
}

fn validate_display_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 60 {
        return Err("Enter a station name between 1 and 60 characters.".to_owned());
    }
    Ok(value.to_owned())
}

fn ensure_default_station(connection: &Connection, session: &SessionUser) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO register_stations
             (business_id,store_id,station_key,display_name,is_active,created_at,updated_at)
             VALUES(?1,?2,'main','Main register',1,?3,?3)",
            params![session.business_id, session.store_id, timestamp()],
        )
        .map_err(|error| format!("Could not prepare the default register station: {error}"))?;
    Ok(())
}

pub(crate) fn station_name_for_key(
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
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The assigned register station is unavailable.".to_owned())
}

/// Resolve the current installation's station from persisted local assignment.
/// The cashier never supplies a station key in checkout or drawer commands.
pub(crate) fn station_key_for_device(
    connection: &Connection,
    device_id: &str,
    session: &SessionUser,
) -> Result<String, String> {
    ensure_default_station(connection, session)?;
    let assigned: Option<(String, bool)> = connection
        .query_row(
            "SELECT d.station_key,s.is_active
             FROM station_devices d JOIN register_stations s
               ON s.store_id=d.store_id AND s.station_key=d.station_key
             WHERE d.device_id=?1 AND d.business_id=?2 AND d.store_id=?3",
            params![device_id, session.business_id, session.store_id],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((station_key, true)) = assigned {
        return Ok(station_key);
    }
    if assigned.is_some() {
        return Err("This device is assigned to an inactive register station. Ask an administrator to reassign it.".to_owned());
    }

    // A portable business backup can contain a different installation's assignment.
    // This app stores one independent local database per installation, so an assignment
    // from the source machine is stale on the destination and can be rebound here.
    connection
        .execute(
            "DELETE FROM station_devices WHERE store_id=?1 AND device_id<>?2",
            params![session.store_id, device_id],
        )
        .map_err(|error| error.to_string())?;
    let station_key: String = connection
        .query_row(
            "SELECT station_key FROM register_stations WHERE business_id=?1 AND store_id=?2 AND is_active=1
             ORDER BY CASE WHEN station_key='main' THEN 0 ELSE 1 END,display_name COLLATE NOCASE,station_key LIMIT 1",
            params![session.business_id, session.store_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "This store has no active register station. Ask an administrator to add one.".to_owned())?;
    let now = timestamp();
    connection
        .execute(
            "INSERT INTO station_devices(device_id,business_id,store_id,station_key,assigned_by,assigned_at,updated_at)
             VALUES(?1,?2,?3,?4,NULL,?5,?5)",
            params![device_id, session.business_id, session.store_id, station_key, now],
        )
        .map_err(|error| format!("Could not assign this device to its register station: {error}"))?;
    Ok(station_key)
}

/// Establish one local station assignment for each store already in the database.
/// The installation UUID lives beside the database, outside portable database backups.
pub(crate) fn initialize_installation(
    connection: &Connection,
    device_id: &str,
) -> Result<(), String> {
    let stores = {
        let mut statement = connection
            .prepare("SELECT id,business_id FROM stores ORDER BY id")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    for (store_id, business_id) in stores {
        let session = SessionUser {
            user_id: String::new(),
            business_id,
            store_id,
            full_name: String::new(),
            username: String::new(),
            role_key: String::new(),
            role_name: String::new(),
            workspace_mode: String::new(),
            is_owner: false,
            permissions: Vec::new(),
            business_name: String::new(),
            store_name: String::new(),
            currency_code: String::new(),
        };
        station_key_for_device(connection, device_id, &session)?;
    }
    Ok(())
}

fn workspace_for(
    connection: &Connection,
    device_id: &str,
    session: &SessionUser,
) -> Result<StationWorkspace, String> {
    let current_station_key = station_key_for_device(connection, device_id, session)?;
    let mut stations = Vec::new();
    let mut statement = connection
        .prepare(
            "SELECT station_key,display_name,is_active FROM register_stations
             WHERE business_id=?1 AND store_id=?2 ORDER BY is_active DESC,display_name COLLATE NOCASE,station_key",
        )
        .map_err(|error| error.to_string())?;
    for row in statement
        .query_map(params![session.business_id, session.store_id], |row| {
            let station_key: String = row.get(0)?;
            Ok(RegisterStation {
                is_current: station_key == current_station_key,
                station_key,
                display_name: row.get(1)?,
                is_active: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(|error| error.to_string())?
    {
        stations.push(row.map_err(|error| error.to_string())?);
    }
    let current_display_name = stations
        .iter()
        .find(|station| station.station_key == current_station_key)
        .map(|station| station.display_name.clone())
        .ok_or_else(|| "The assigned register station is unavailable.".to_owned())?;
    Ok(StationWorkspace {
        current_station_key,
        current_display_name,
        stations,
    })
}

#[tauri::command]
pub fn get_station_workspace(state: State<'_, AppState>) -> Result<StationWorkspace, String> {
    let session = active_session(&state, "")?;
    if !session.is_owner
        && !session.permissions.iter().any(|permission| {
            matches!(
                permission.as_str(),
                "settings:write" | "sales:read" | "reports:read" | "expenses:write"
            )
        })
    {
        return Err("You do not have access to register station information.".to_owned());
    }
    let connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    let mut workspace = workspace_for(&connection, &state.device_id, &session)?;
    if !session.is_owner
        && !session
            .permissions
            .iter()
            .any(|permission| matches!(permission.as_str(), "settings:write" | "reports:read"))
    {
        workspace.stations.retain(|station| station.is_current);
    }
    Ok(workspace)
}

#[tauri::command]
pub fn create_station(
    input: CreateStationInput,
    state: State<'_, AppState>,
) -> Result<StationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let display_name = validate_display_name(&input.display_name)?;
    let mut connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let duplicate: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM register_stations WHERE store_id=?1 AND display_name=?2 COLLATE NOCASE)",
            params![session.store_id, display_name],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if duplicate {
        return Err("A register station with that name already exists in this store.".to_owned());
    }
    let station_key = format!("station-{}", Uuid::new_v4());
    let now = timestamp();
    tx.execute(
        "INSERT INTO register_stations(business_id,store_id,station_key,display_name,is_active,created_at,updated_at)
         VALUES(?1,?2,?3,?4,1,?5,?5)",
        params![session.business_id, session.store_id, station_key, display_name, now],
    )
    .map_err(|error| error.to_string())?;
    add_audit_event(
        &tx,
        &session,
        "register_station_created",
        "register_station",
        Some(&station_key),
        serde_json::json!({"displayName":display_name}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    drop(connection);
    let connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    workspace_for(&connection, &state.device_id, &session)
}

#[tauri::command]
pub fn rename_station(
    input: RenameStationInput,
    state: State<'_, AppState>,
) -> Result<StationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let display_name = validate_display_name(&input.display_name)?;
    let mut connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let current_name: Option<String> = tx.query_row(
        "SELECT display_name FROM register_stations WHERE business_id=?1 AND store_id=?2 AND station_key=?3",
        params![session.business_id, session.store_id, input.station_key],
        |row| row.get(0),
    ).optional().map_err(|error| error.to_string())?;
    if current_name.is_none() {
        return Err("That register station is not available in this store.".to_owned());
    }
    let duplicate: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM register_stations WHERE store_id=?1 AND display_name=?2 COLLATE NOCASE AND station_key<>?3)",
            params![session.store_id, display_name, input.station_key],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if duplicate {
        return Err("A register station with that name already exists in this store.".to_owned());
    }
    tx.execute(
        "UPDATE register_stations SET display_name=?1,updated_at=?2 WHERE business_id=?3 AND store_id=?4 AND station_key=?5",
        params![display_name, timestamp(), session.business_id, session.store_id, input.station_key],
    )
    .map_err(|error| error.to_string())?;
    add_audit_event(
        &tx,
        &session,
        "register_station_renamed",
        "register_station",
        Some(&input.station_key),
        serde_json::json!({"previousName":current_name,"displayName":display_name}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    drop(connection);
    let connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    workspace_for(&connection, &state.device_id, &session)
}

#[tauri::command]
pub fn assign_current_device_station(
    input: AssignStationInput,
    state: State<'_, AppState>,
) -> Result<StationWorkspace, String> {
    let session = active_session(&state, "settings:write")?;
    let mut connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let current_key = station_key_for_device(&tx, &state.device_id, &session)?;
    let target: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM register_stations WHERE business_id=?1 AND store_id=?2 AND station_key=?3 AND is_active=1)",
            params![session.business_id, session.store_id, input.station_key],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !target {
        return Err("Choose an active register station in this store.".to_owned());
    }
    if current_key != input.station_key {
        let has_open_shift: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM cash_shifts WHERE store_id=?1 AND station_key IN (?2,?3) AND status='open')",
                params![session.store_id, current_key, input.station_key],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if has_open_shift {
            return Err(
                "Close the open drawer before changing this device's register station.".to_owned(),
            );
        }
        let now = timestamp();
        tx.execute(
            "INSERT INTO station_devices(device_id,business_id,store_id,station_key,assigned_by,assigned_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?6)
             ON CONFLICT(device_id,store_id) DO UPDATE SET station_key=excluded.station_key,assigned_by=excluded.assigned_by,assigned_at=excluded.assigned_at,updated_at=excluded.updated_at",
            params![state.device_id, session.business_id, session.store_id, input.station_key, session.user_id, now],
        )
        .map_err(|error| {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                "That station is already assigned to another local installation. Add another station or select an unassigned one.".to_owned()
            } else {
                error.to_string()
            }
        })?;
        add_audit_event(
            &tx,
            &session,
            "register_device_station_assigned",
            "register_station",
            Some(&input.station_key),
            serde_json::json!({"previousStationKey":current_key,"stationKey":input.station_key,"deviceId":state.device_id}),
        )?;
    }
    tx.commit().map_err(|error| error.to_string())?;
    drop(connection);
    let connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    workspace_for(&connection, &state.device_id, &session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_session() -> SessionUser {
        SessionUser {
            user_id: "cashier-a".to_owned(),
            business_id: "business-a".to_owned(),
            store_id: "store-a".to_owned(),
            full_name: "Cashier".to_owned(),
            username: "cashier".to_owned(),
            role_key: "cashier".to_owned(),
            role_name: "Cashier".to_owned(),
            workspace_mode: "cashier".to_owned(),
            is_owner: false,
            permissions: Vec::new(),
            business_name: "Business".to_owned(),
            store_name: "Store".to_owned(),
            currency_code: "GHS".to_owned(),
        }
    }

    fn test_connection() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE register_stations(
                    business_id TEXT NOT NULL,store_id TEXT NOT NULL,station_key TEXT NOT NULL,
                    display_name TEXT NOT NULL,is_active INTEGER NOT NULL,created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,PRIMARY KEY(store_id,station_key));
                 CREATE TABLE station_devices(
                    device_id TEXT NOT NULL,business_id TEXT NOT NULL,store_id TEXT NOT NULL,
                    station_key TEXT NOT NULL,assigned_by TEXT,assigned_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,PRIMARY KEY(device_id,store_id));
                 CREATE UNIQUE INDEX idx_test_station_assignment ON station_devices(store_id,station_key);",
            )
            .unwrap();
        connection
    }

    #[test]
    fn installation_uses_stable_persisted_station_assignment() {
        let connection = test_connection();
        let session = test_session();
        assert_eq!(
            station_key_for_device(&connection, "device-a", &session).unwrap(),
            "main"
        );
        connection
            .execute(
                "INSERT INTO register_stations(business_id,store_id,station_key,display_name,is_active,created_at,updated_at)
                 VALUES('business-a','store-a','station-2','Back counter',1,'now','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE station_devices SET station_key='station-2' WHERE device_id='device-a' AND store_id='store-a'",
                [],
            )
            .unwrap();
        assert_eq!(
            station_key_for_device(&connection, "device-a", &session).unwrap(),
            "station-2"
        );
        assert_eq!(
            station_name_for_key(&connection, "store-a", "station-2").unwrap(),
            "Back counter"
        );
    }

    #[test]
    fn inactive_station_assignment_is_rejected_until_administrator_reassigns() {
        let connection = test_connection();
        let session = test_session();
        station_key_for_device(&connection, "device-a", &session).unwrap();
        connection
            .execute(
                "UPDATE register_stations SET is_active=0 WHERE store_id='store-a' AND station_key='main'",
                [],
            )
            .unwrap();
        assert!(station_key_for_device(&connection, "device-a", &session)
            .unwrap_err()
            .contains("inactive register station"));
    }
}
