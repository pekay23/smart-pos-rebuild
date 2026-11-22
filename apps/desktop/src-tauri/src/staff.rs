// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use tauri::State;

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::{SessionUser, StaffAccount, StaffAccountStatusInput},
};

fn staff_account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StaffAccount> {
    Ok(StaffAccount {
        user_id: row.get(0)?,
        full_name: row.get(1)?,
        username: row.get(2)?,
        role_key: row.get(3)?,
        role_name: row.get(4)?,
        has_pin: row.get::<_, i64>(5)? != 0,
        is_active: row.get::<_, i64>(6)? != 0,
        is_owner: row.get::<_, i64>(7)? != 0,
    })
}

fn load_staff_accounts(db: &Connection, business_id: &str) -> Result<Vec<StaffAccount>, String> {
    let mut statement = db
        .prepare(
            "SELECT u.id,u.full_name,u.username,r.role_key,r.display_name,u.pin_hash IS NOT NULL,u.is_active,COALESCE(b.owner_user_id=u.id,0)
             FROM users u JOIN roles r ON r.id=u.role_id
             JOIN businesses b ON b.id=u.business_id
             WHERE u.business_id=?1
             ORDER BY u.is_active DESC, CASE r.role_key WHEN 'admin' THEN 0 WHEN 'manager' THEN 1 ELSE 2 END, u.full_name COLLATE NOCASE",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([business_id], staff_account_from_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn set_staff_active(
    db: &mut Connection,
    session: &SessionUser,
    user_id: &str,
    is_active: bool,
) -> Result<StaffAccount, String> {
    let user_id = user_id.trim();
    if user_id.is_empty() || user_id.chars().count() > 64 {
        return Err("Choose a valid staff account.".to_owned());
    }
    if !is_active && user_id == session.user_id {
        return Err("You cannot deactivate the account you are using.".to_owned());
    }
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let target = tx
        .query_row(
            "SELECT u.username,r.role_key,u.is_active,COALESCE(b.owner_user_id=u.id,0) FROM users u JOIN roles r ON r.id=u.role_id
             JOIN businesses b ON b.id=u.business_id WHERE u.id=?1 AND u.business_id=?2",
            params![user_id, session.business_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                    row.get::<_, i64>(3)? != 0,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That staff account could not be found in this business.".to_owned())?;
    if target.2 == is_active {
        return load_account_in_transaction(&tx, &session.business_id, user_id);
    }
    if !is_active && target.3 {
        return Err("The business owner account cannot be deactivated.".to_owned());
    }
    if !is_active && target.1 == "admin" {
        let active_admins: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM users u JOIN roles r ON r.id=u.role_id JOIN businesses b ON b.id=u.business_id
                 WHERE u.business_id=?1 AND u.is_active=1 AND (r.role_key='admin' OR b.owner_user_id=u.id)",
                [&session.business_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if active_admins <= 1 {
            return Err("Keep at least one active administrator in the business.".to_owned());
        }
    }
    tx.execute(
        "UPDATE users SET is_active=?1 WHERE id=?2 AND business_id=?3",
        params![is_active as i64, user_id, session.business_id],
    )
    .map_err(|error| error.to_string())?;
    add_audit_event(
        &tx,
        session,
        if is_active {
            "staff_account_reactivated"
        } else {
            "staff_account_deactivated"
        },
        "user",
        Some(user_id),
        serde_json::json!({ "username": target.0, "active": is_active }),
    )?;
    let account = load_account_in_transaction(&tx, &session.business_id, user_id)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(account)
}

fn load_account_in_transaction(
    db: &Connection,
    business_id: &str,
    user_id: &str,
) -> Result<StaffAccount, String> {
    db.query_row(
        "SELECT u.id,u.full_name,u.username,r.role_key,r.display_name,u.pin_hash IS NOT NULL,u.is_active,COALESCE(b.owner_user_id=u.id,0)
         FROM users u JOIN roles r ON r.id=u.role_id JOIN businesses b ON b.id=u.business_id WHERE u.business_id=?1 AND u.id=?2",
        params![business_id, user_id],
        staff_account_from_row,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_staff_accounts(state: State<'_, AppState>) -> Result<Vec<StaffAccount>, String> {
    let session = active_session(&state, "users:write")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    load_staff_accounts(&db, &session.business_id)
}

#[tauri::command]
pub fn set_staff_account_active(
    input: StaffAccountStatusInput,
    state: State<'_, AppState>,
) -> Result<StaffAccount, String> {
    let session = active_session(&state, "users:write")?;
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    set_staff_active(&mut db, &session, &input.user_id, input.is_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Connection, SessionUser) {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        db.execute_batch(
            "INSERT INTO businesses(id,name,created_at) VALUES('b','Business','now');
             INSERT INTO businesses(id,name,created_at) VALUES('other','Other business','now');
             INSERT INTO stores(id,business_id,name,created_at) VALUES('s','b','Store','now');
             INSERT INTO roles(id,business_id,role_key,display_name,permissions_json,is_system) VALUES
               ('ra','b','admin','Administrator','[\"users:write\"]',1),
               ('rc','b','cashier','Cashier','[]',1),
               ('ro','other','admin','Administrator','[\"users:write\"]',1);
             INSERT INTO users(id,business_id,role_id,username,password_hash,full_name,created_at) VALUES
               ('admin1','b','ra','owner1','hash','Owner One','now'),
               ('admin2','b','ra','owner2','hash','Owner Two','now'),
               ('cashier','b','rc','cashier','hash','Cashier One','now'),
               ('other-admin','other','ro','other-owner','hash','Other Owner','now');",
        )
        .unwrap();
        db.execute_batch("ALTER TABLE businesses ADD COLUMN owner_user_id TEXT;")
            .unwrap();
        db.execute_batch("ALTER TABLE users ADD COLUMN pin_hash TEXT;")
            .unwrap();
        let session = SessionUser {
            user_id: "admin1".into(),
            business_id: "b".into(),
            store_id: "s".into(),
            full_name: "Owner One".into(),
            username: "owner1".into(),
            role_key: "admin".into(),
            role_name: "Administrator".into(),
            workspace_mode: "management".into(),
            is_owner: false,
            permissions: vec!["users:write".into()],
            business_name: "Business".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        };
        (db, session)
    }

    #[test]
    fn account_lifecycle_blocks_self_and_last_admin_but_allows_restore() {
        let (mut db, session) = fixture();
        assert!(set_staff_active(&mut db, &session, "admin1", false).is_err());
        set_staff_active(&mut db, &session, "admin2", false).unwrap();
        let accounts = load_staff_accounts(&db, "b").unwrap();
        assert_eq!(accounts.len(), 3);
        assert!(
            !accounts
                .iter()
                .find(|account| account.user_id == "admin2")
                .unwrap()
                .is_active
        );
        assert_eq!(load_staff_accounts(&db, "other").unwrap().len(), 1);
        let mut different_actor = session.clone();
        different_actor.user_id = "cashier".into();
        assert!(set_staff_active(&mut db, &different_actor, "admin1", false).is_err());
        let restored = set_staff_active(&mut db, &session, "admin2", true).unwrap();
        assert!(restored.is_active);
        let events: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE entity_id='admin2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(events, 2);
    }
}
