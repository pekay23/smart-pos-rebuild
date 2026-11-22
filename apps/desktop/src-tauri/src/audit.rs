// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use rusqlite::{params, Connection, TransactionBehavior};
use tauri::State;

use crate::{
    commands::active_session,
    db::AppState,
    models::{AuditEventPage, AuditEventQuery, AuditEventSummary},
};

fn query_audit_events(
    db: &mut Connection,
    business_id: &str,
    store_id: &str,
    input: AuditEventQuery,
) -> Result<AuditEventPage, String> {
    if input.offset < 0 || !(1..=100).contains(&input.page_size) {
        return Err("Audit history page is outside the supported range.".to_owned());
    }
    if input.snapshot_row_id.is_some_and(|row_id| row_id < 0) {
        return Err("Audit history snapshot is invalid.".to_owned());
    }
    let search = input
        .search
        .unwrap_or_default()
        .trim()
        .chars()
        .take(100)
        .collect::<String>();
    let pattern = search.to_lowercase();
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| error.to_string())?;
    let snapshot_row_id = input.snapshot_row_id.unwrap_or(
        tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM audit_events WHERE business_id=?1 AND store_id=?2",
            params![business_id, store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?,
    );
    let filter = "e.business_id=?1 AND e.store_id=?2 AND e.rowid<=?3 AND (?4='' OR instr(lower(e.action),?5)>0 OR instr(lower(e.entity_type),?5)>0 OR instr(lower(COALESCE(e.entity_id,'')),?5)>0 OR instr(lower(COALESCE(u.full_name,'')),?5)>0 OR instr(lower(COALESCE(u.username,'')),?5)>0)";
    let count_sql = format!(
        "SELECT COUNT(*) FROM audit_events e LEFT JOIN users u ON u.id=e.user_id WHERE {filter}"
    );
    let matched_count = tx
        .query_row(
            &count_sql,
            params![business_id, store_id, snapshot_row_id, search, pattern],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let page_sql = format!("SELECT e.id,e.action,e.entity_type,e.entity_id,COALESCE(NULLIF(trim(u.full_name),''),u.username),e.created_at FROM audit_events e LEFT JOIN users u ON u.id=e.user_id WHERE {filter} ORDER BY e.created_at DESC,e.id DESC LIMIT ?6 OFFSET ?7");
    let mut statement = tx.prepare(&page_sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                business_id,
                store_id,
                snapshot_row_id,
                search,
                pattern,
                input.page_size,
                input.offset
            ],
            |row| {
                Ok(AuditEventSummary {
                    id: row.get(0)?,
                    action: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    actor_name: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(AuditEventPage {
        rows,
        matched_count,
        offset: input.offset,
        page_size: input.page_size,
        snapshot_row_id,
    })
}

#[tauri::command]
pub fn get_audit_events(
    input: AuditEventQuery,
    state: State<'_, AppState>,
) -> Result<AuditEventPage, String> {
    let session = active_session(&state, "reports:read")?;
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    query_audit_events(&mut db, &session.business_id, &session.store_id, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        db.execute_batch("INSERT INTO businesses(id,name,created_at) VALUES('b1','Business','now'); INSERT INTO stores(id,business_id,name,created_at) VALUES('s1','b1','Store','now'),('s2','b1','Other','now'); INSERT INTO roles(id,business_id,role_key,display_name,permissions_json,is_system) VALUES('r1','b1','admin','Admin','[]',1); INSERT INTO users(id,business_id,role_id,username,password_hash,full_name,created_at) VALUES('u1','b1','r1','ana','hash','Ana Mensah','now');").unwrap();
        db
    }

    fn add(db: &Connection, id: &str, store: &str, action: &str, created: &str) {
        db.execute("INSERT INTO audit_events(id,business_id,store_id,user_id,action,entity_type,entity_id,details_json,created_at) VALUES(?1,'b1',?2,'u1',?3,'product','p1','{}',?4)", params![id,store,action,created]).unwrap();
    }

    #[test]
    fn audit_history_is_store_scoped_searchable_and_excludes_later_rows() {
        let mut db = fixture();
        add(&db, "a1", "s1", "product.created", "2026-01-01T00:00:00Z");
        add(&db, "a2", "s2", "product.deleted", "2026-01-02T00:00:00Z");
        let first = query_audit_events(
            &mut db,
            "b1",
            "s1",
            AuditEventQuery {
                search: Some("ANA".into()),
                offset: 0,
                page_size: 20,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        assert_eq!(first.matched_count, 1);
        assert_eq!(first.rows[0].actor_name.as_deref(), Some("Ana Mensah"));
        add(&db, "a3", "s1", "product.updated", "2026-01-03T00:00:00Z");
        let next = query_audit_events(
            &mut db,
            "b1",
            "s1",
            AuditEventQuery {
                search: None,
                offset: 0,
                page_size: 20,
                snapshot_row_id: Some(first.snapshot_row_id),
            },
        )
        .unwrap();
        assert_eq!(next.matched_count, 1);
        assert_eq!(next.rows[0].id, "a1");
    }

    #[test]
    fn audit_history_rejects_invalid_page_size() {
        let mut db = fixture();
        let result = query_audit_events(
            &mut db,
            "b1",
            "s1",
            AuditEventQuery {
                search: None,
                offset: 0,
                page_size: 101,
                snapshot_row_id: None,
            },
        );
        assert!(result.is_err());
    }
}
