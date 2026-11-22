// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::Utc;
use openssl::symm::{decrypt_aead, encrypt_aead, Cipher};
use rand_core::{OsRng, RngCore};
use rusqlite::{backup::Backup, params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{commands::active_session, db::AppState, models::SessionUser};

const BACKUP_DIRECTORY: &str = "backups";
const PORTABLE_MAGIC: &[u8; 8] = b"NTILBK01";
const PORTABLE_HEADER_LEN: usize = 8 + 1 + 16 + 12;
const PORTABLE_TAG_LEN: usize = 16;
const MAX_PORTABLE_BACKUP_BYTES: u64 = 256 * 1024 * 1024;
const PORTABLE_EXTENSIONS: &[&str] = &["ntbackup"];
const BACKUP_RETENTION_OPTIONS: [u32; 4] = [0, 10, 25, 50];
const REQUIRED_TABLES: &[&str] = &[
    "businesses",
    "stores",
    "users",
    "roles",
    "products",
    "sales",
    "sale_items",
    "cash_shifts",
    "refunds",
    "payment_integration_requests",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBackup {
    pub id: String,
    pub created_at: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_warning: Option<String>,
}

fn backup_dir(state: &AppState) -> PathBuf {
    state.data_dir.join(BACKUP_DIRECTORY)
}

fn backup_path(state: &AppState, id: &str) -> Result<PathBuf, String> {
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("Choose a valid backup from this device.".to_owned());
    }
    Ok(backup_dir(state).join(format!("{id}.sqlite3")))
}

fn insert_audit(db: &Connection, user: &SessionUser, action: &str, id: &str) -> Result<(), String> {
    let user_id: Option<String> = db
        .query_row(
            "SELECT id FROM users WHERE id=?1 AND business_id=?2",
            params![user.user_id, user.business_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let store_id: Option<String> = db
        .query_row(
            "SELECT id FROM stores WHERE id=?1 AND business_id=?2",
            params![user.store_id, user.business_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    db.execute(
        "INSERT INTO audit_events(id,business_id,store_id,user_id,action,entity_type,entity_id,details_json,created_at)
         VALUES(?1,?2,?3,?4,?5,'database_backup',?6,'{}',?7)",
        params![Uuid::new_v4().to_string(), user.business_id, store_id, user_id, action, id, Utc::now().to_rfc3339()],
    ).map_err(|error| format!("Could not record the backup action: {error}"))?;
    Ok(())
}

fn restore_from_snapshot(db: &mut Connection, path: &Path) -> Result<(), String> {
    let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("Could not open the safety snapshot: {error}"))?;
    restore_from_connection(db, &source)
}

fn restore_from_connection(db: &mut Connection, source: &Connection) -> Result<(), String> {
    let backup = Backup::new(source, db)
        .map_err(|error| format!("Could not start database recovery: {error}"))?;
    backup
        .run_to_completion(128, Duration::from_millis(10), None)
        .map_err(|error| format!("Could not finish database recovery: {error}"))
}

fn validate_portable_password(password: &str) -> Result<(), String> {
    if password.chars().count() < 12 || password.len() > 1024 {
        return Err(
            "Use a backup passphrase with at least 12 characters and no more than 1024 bytes."
                .to_owned(),
        );
    }
    Ok(())
}

fn derive_portable_key(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = argon2::Params::new(64 * 1024, 3, 1, Some(32))
        .map_err(|_| "Could not prepare backup encryption.".to_owned())?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| "Could not protect the portable backup.".to_owned())?;
    Ok(key)
}

fn encrypt_portable_backup(password: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    validate_portable_password(password)?;
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = derive_portable_key(password, &salt)?;
    let mut header = Vec::with_capacity(PORTABLE_HEADER_LEN);
    header.extend_from_slice(PORTABLE_MAGIC);
    header.push(1);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    let mut tag = [0u8; PORTABLE_TAG_LEN];
    let ciphertext = encrypt_aead(
        Cipher::aes_256_gcm(),
        key.as_ref(),
        Some(&nonce),
        &header,
        plaintext,
        &mut tag,
    )
    .map_err(|_| "Could not encrypt the portable backup.".to_owned())?;
    let mut output = header;
    output.extend_from_slice(&tag);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn decrypt_portable_backup(password: &str, encrypted: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    validate_portable_password(password)?;
    if encrypted.len() < PORTABLE_HEADER_LEN + PORTABLE_TAG_LEN
        || &encrypted[..8] != PORTABLE_MAGIC
        || encrypted[8] != 1
    {
        return Err("This is not a supported NimbleTill encrypted backup.".to_owned());
    }
    let salt: &[u8; 16] = encrypted[9..25]
        .try_into()
        .map_err(|_| "The encrypted backup header is invalid.".to_owned())?;
    let nonce: &[u8; 12] = encrypted[25..PORTABLE_HEADER_LEN]
        .try_into()
        .map_err(|_| "The encrypted backup header is invalid.".to_owned())?;
    let tag = &encrypted[PORTABLE_HEADER_LEN..PORTABLE_HEADER_LEN + PORTABLE_TAG_LEN];
    let ciphertext = &encrypted[PORTABLE_HEADER_LEN + PORTABLE_TAG_LEN..];
    let key = derive_portable_key(password, salt)?;
    let plaintext = decrypt_aead(
        Cipher::aes_256_gcm(),
        key.as_ref(),
        Some(nonce),
        &encrypted[..PORTABLE_HEADER_LEN],
        ciphertext,
        tag,
    )
    .map_err(|_| {
        "The backup passphrase is incorrect or the encrypted file is damaged.".to_owned()
    })?;
    Ok(Zeroizing::new(plaintext))
}

fn connection_from_portable_bytes(bytes: &[u8]) -> Result<Connection, String> {
    let length = bytes.len();
    let mut connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
    connection
        .deserialize_read_exact("main", Cursor::new(bytes), length, true)
        .map_err(|_| "The decrypted file is not a valid SQLite backup.".to_owned())?;
    Ok(connection)
}

fn serialize_snapshot(
    state: &AppState,
    expected_business_id: &str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut snapshot = Connection::open_in_memory().map_err(|error| error.to_string())?;
    Backup::new(&db, &mut snapshot)
        .map_err(|error| format!("Could not start the portable backup: {error}"))?
        .run_to_completion(128, Duration::from_millis(10), None)
        .map_err(|error| format!("Could not finish the portable backup: {error}"))?;
    let business_id = read_backup_business_id(&snapshot)?;
    if business_id != expected_business_id {
        return Err("The local database business identity changed. Sign in again.".to_owned());
    }
    let serialized = snapshot
        .serialize("main")
        .map_err(|error| format!("Could not serialize the portable backup: {error}"))?;
    Ok(Zeroizing::new(serialized.to_vec()))
}

fn create_snapshot(db: &Connection, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the backup folder: {error}"))?;
    }
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("Could not replace an incomplete backup: {error}"))?;
    }
    db.backup("main", path, None)
        .map_err(|error| format!("Could not write the database backup: {error}"))
}

fn read_backup_business_id(db: &Connection) -> Result<String, String> {
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|error| format!("Could not verify this backup: {error}"))?;
    if integrity != "ok" {
        return Err("This backup failed SQLite integrity verification.".to_owned());
    }
    let foreign_key_problem: Option<String> = db
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .optional()
        .map_err(|error| format!("Could not verify backup relationships: {error}"))?;
    if foreign_key_problem.is_some() {
        return Err("This backup contains broken record relationships.".to_owned());
    }
    for table in REQUIRED_TABLES {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !exists {
            return Err("This file is not a complete NimbleTill backup.".to_owned());
        }
    }
    db.query_row(
        "SELECT id FROM businesses ORDER BY rowid LIMIT 1",
        [],
        |row| row.get(0),
    )
    .map_err(|_| "This backup does not contain a configured business.".to_owned())
}

fn validate_backup(path: &Path, expected_business_id: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err("That backup is no longer available on this device.".to_owned());
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("Could not open this backup: {error}"))?;
    let business_id = read_backup_business_id(&db)?;
    if business_id != expected_business_id {
        return Err(
            "This backup belongs to a different business and cannot be restored here.".to_owned(),
        );
    }
    Ok(())
}

fn create_named_backup(
    state: &AppState,
    user: &SessionUser,
    id: &str,
) -> Result<LocalBackup, String> {
    let final_path = backup_path(state, id)?;
    let staging_path = backup_dir(state).join(format!("{id}.partial"));
    let db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    insert_audit(&db, user, "database.backup.created", id)?;
    create_snapshot(&db, &staging_path)?;
    validate_backup(&staging_path, &user.business_id)?;
    fs::rename(&staging_path, &final_path)
        .map_err(|error| format!("Could not finalize the backup: {error}"))?;
    let size_bytes = fs::metadata(&final_path)
        .map_err(|error| error.to_string())?
        .len();
    Ok(LocalBackup {
        id: id.to_owned(),
        created_at: Utc::now().to_rfc3339(),
        size_bytes,
        retention_warning: None,
    })
}

fn read_retention_limit(db: &Connection, business_id: &str) -> Result<u32, String> {
    let value: Option<String> = db
        .query_row(
            "SELECT setting_value FROM app_settings WHERE business_id=?1 AND setting_key='backup_retention_count'",
            [business_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let limit = value
        .as_deref()
        .unwrap_or("0")
        .parse::<u32>()
        .map_err(|_| "The saved backup retention limit is invalid.".to_owned())?;
    if !BACKUP_RETENTION_OPTIONS.contains(&limit) {
        return Err("The saved backup retention limit is not supported.".to_owned());
    }
    Ok(limit)
}

#[tauri::command]
pub fn get_backup_retention_limit(state: State<'_, AppState>) -> Result<u32, String> {
    let user = active_session(&state, "settings:write")?;
    let db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    read_retention_limit(&db, &user.business_id)
}

#[tauri::command]
pub fn save_backup_retention_limit(
    retention_limit: u32,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    if !BACKUP_RETENTION_OPTIONS.contains(&retention_limit) {
        return Err("Choose unlimited, 10, 25, or 50 local backups.".to_owned());
    }
    let user = active_session(&state, "settings:write")?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO app_settings(business_id,setting_key,setting_value,updated_at) VALUES(?1,'backup_retention_count',?2,?3)
             ON CONFLICT(business_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=excluded.updated_at",
            params![user.business_id, retention_limit.to_string(), Utc::now().to_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    insert_audit(
        &transaction,
        &user,
        "database.backup.retention_changed",
        &retention_limit.to_string(),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(retention_limit)
}

fn prune_created_backups(state: &AppState, user: &SessionUser, keep: u32) -> Result<(), String> {
    if keep == 0 {
        return Ok(());
    }
    let mut backups = list_backups_for_business(state, &user.business_id)?
        .into_iter()
        .filter(|backup| backup.id.starts_with("backup-"))
        .collect::<Vec<_>>();
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    for backup in backups.into_iter().skip(keep as usize) {
        let path = backup_path(state, &backup.id)?;
        let db = state
            .db
            .lock()
            .map_err(|_| "The local database is unavailable.".to_owned())?;
        insert_audit(
            &db,
            user,
            "database.backup.retention_prune_attempted",
            &backup.id,
        )?;
        fs::remove_file(path)
            .map_err(|error| format!("Could not remove an expired local backup: {error}"))?;
    }
    Ok(())
}

fn list_backups_for_business(
    state: &AppState,
    business_id: &str,
) -> Result<Vec<LocalBackup>, String> {
    let directory = backup_dir(state);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut backups = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("Could not read local backups: {error}"))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("sqlite3") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if validate_backup(&path, business_id).is_err() {
            continue;
        }
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        let modified = metadata.modified().map_err(|error| error.to_string())?;
        backups.push(LocalBackup {
            id: id.to_owned(),
            created_at: chrono::DateTime::<Utc>::from(modified).to_rfc3339(),
            size_bytes: metadata.len(),
            retention_warning: None,
        });
    }
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Result<Vec<LocalBackup>, String> {
    let user = active_session(&state, "settings:write")?;
    list_backups_for_business(&state, &user.business_id)
}

#[tauri::command]
pub fn create_backup(state: State<'_, AppState>) -> Result<LocalBackup, String> {
    let user = active_session(&state, "settings:write")?;
    let id = format!(
        "backup-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let mut backup = create_named_backup(&state, &user, &id)?;
    let retention_limit = {
        let db = state
            .db
            .lock()
            .map_err(|_| "The local database is unavailable.".to_owned())?;
        read_retention_limit(&db, &user.business_id)?
    };
    if let Err(error) = prune_created_backups(&state, &user, retention_limit) {
        backup.retention_warning = Some(format!(
            "Backup created, but automatic cleanup could not finish: {error}"
        ));
    }
    Ok(backup)
}

#[tauri::command]
pub fn restore_backup(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let user = active_session(&state, "settings:write")?;
    let source_path = backup_path(&state, &id)?;
    validate_backup(&source_path, &user.business_id)?;
    let source =
        Connection::open_with_flags(&source_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| format!("Could not open this backup: {error}"))?;
    restore_verified_source(
        &state,
        Some(&user),
        &source,
        &id,
        "database.backup.restored",
    )
}

fn restore_verified_source(
    state: &AppState,
    user: Option<&SessionUser>,
    source: &Connection,
    backup_id: &str,
    restore_action: &str,
) -> Result<(), String> {
    let business_id = read_backup_business_id(source)?;
    if user.is_some_and(|user| business_id != user.business_id) {
        return Err(
            "This backup belongs to a different business and cannot be restored here.".to_owned(),
        );
    }
    let rollback_id = format!(
        "rollback-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let rollback_path = backup_path(state, &rollback_id)?;
    if let Some(user) = user {
        create_named_backup(state, user, &rollback_id)?;
    } else {
        let db = state
            .db
            .lock()
            .map_err(|_| "The local database is unavailable.".to_owned())?;
        let has_business: bool = db
            .query_row("SELECT EXISTS(SELECT 1 FROM businesses)", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        if has_business {
            return Err(
                "Sign in with an account that can manage backups before restoring.".to_owned(),
            );
        }
        create_snapshot(&db, &rollback_path)?;
    }
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    if let Err(error) = restore_from_connection(&mut db, source) {
        let rollback_path = backup_path(&state, &rollback_id)?;
        return match restore_from_snapshot(&mut db, &rollback_path) {
            Ok(()) => Err(format!(
                "Restore could not complete and the previous database was recovered: {error}"
            )),
            Err(recovery_error) => Err(format!(
                "Restore failed ({error}) and automatic recovery failed: {recovery_error}. Keep the app open and contact support."
            )),
        };
    }
    if let Err(error) = read_backup_business_id(&db).and_then(|business| {
        if business == business_id {
            Ok(())
        } else {
            Err("Restored database business identity did not match.".to_owned())
        }
    }) {
        let rollback_path = backup_path(&state, &rollback_id)?;
        return match restore_from_snapshot(&mut db, &rollback_path) {
            Ok(()) => Err(format!("Restore was rejected and the previous database was recovered: {error}")),
            Err(recovery_error) => Err(format!("Restore verification failed ({error}); automatic recovery failed: {recovery_error}. Keep the app open and contact support.")),
        };
    }
    if let Some(user) = user {
        if let Err(error) = insert_audit(&db, user, restore_action, backup_id) {
            let rollback_path = backup_path(&state, &rollback_id)?;
            return match restore_from_snapshot(&mut db, &rollback_path) {
            Ok(()) => Err(format!(
                "Restore was rolled back because its audit entry could not be saved: {error}"
            )),
            Err(recovery_error) => Err(format!(
                "Restore audit failed ({error}) and automatic recovery failed: {recovery_error}. Keep the app open and contact support."
            )),
        };
        }
    }
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.".to_owned())? = None;
    Ok(())
}

#[tauri::command]
pub async fn export_portable_backup(
    password: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let session = active_session(&state, "settings:write")?;
    validate_portable_password(&password)?;
    let user = session.user.clone();
    drop(session);
    let Some(selected) = app
        .dialog()
        .file()
        .set_title("Export encrypted NimbleTill backup")
        .set_file_name("nimbletill-backup.ntbackup")
        .add_filter("NimbleTill encrypted backup", PORTABLE_EXTENSIONS)
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let mut path = selected
        .into_path()
        .map_err(|error| format!("Could not use the selected backup path: {error}"))?;
    if path.extension().is_none() {
        path.set_extension("ntbackup");
    }
    let plaintext = serialize_snapshot(&state, &user.business_id)?;
    let encrypted = encrypt_portable_backup(&password, &plaintext)?;
    fs::write(&path, encrypted)
        .map_err(|error| format!("Could not write the encrypted backup file: {error}"))?;
    let id = format!("portable-{}", Uuid::new_v4().simple());
    let db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    insert_audit(&db, &user, "database.backup.portable_exported", &id)?;
    Ok(true)
}

#[tauri::command]
pub async fn restore_portable_backup(
    password: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    validate_portable_password(&password)?;
    let user = match active_session(&state, "settings:write") {
        Ok(session) => Some(session.user.clone()),
        Err(session_error) => {
            let db = state
                .db
                .lock()
                .map_err(|_| "The local database is unavailable.".to_owned())?;
            let has_business: bool = db
                .query_row("SELECT EXISTS(SELECT 1 FROM businesses)", [], |row| {
                    row.get(0)
                })
                .map_err(|error| error.to_string())?;
            if has_business {
                return Err(session_error);
            }
            None
        }
    };
    let Some(selected) = app
        .dialog()
        .file()
        .set_title("Restore encrypted NimbleTill backup")
        .add_filter("NimbleTill encrypted backup", PORTABLE_EXTENSIONS)
        .blocking_pick_file()
    else {
        return Ok(false);
    };
    let path = selected
        .into_path()
        .map_err(|error| format!("Could not use the selected backup path: {error}"))?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Could not read the selected backup file: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_PORTABLE_BACKUP_BYTES {
        return Err("Choose an encrypted backup file no larger than 256 MB.".to_owned());
    }
    let file = fs::File::open(&path)
        .map_err(|error| format!("Could not read the selected backup file: {error}"))?;
    let mut encrypted = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PORTABLE_BACKUP_BYTES + 1)
        .read_to_end(&mut encrypted)
        .map_err(|error| format!("Could not read the selected backup file: {error}"))?;
    if encrypted.len() as u64 > MAX_PORTABLE_BACKUP_BYTES {
        return Err("Choose an encrypted backup file no larger than 256 MB.".to_owned());
    }
    let plaintext = decrypt_portable_backup(&password, &encrypted)?;
    let source = connection_from_portable_bytes(&plaintext)?;
    let source_business = read_backup_business_id(&source)?;
    if user
        .as_ref()
        .is_some_and(|user| source_business != user.business_id)
    {
        return Err(
            "This backup belongs to a different business and cannot be restored here.".to_owned(),
        );
    }
    let id = format!("portable-{}", Uuid::new_v4().simple());
    restore_verified_source(
        &state,
        user.as_ref(),
        &source,
        &id,
        "database.backup.portable_restored",
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn backup_test_state() -> AppState {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE businesses(id TEXT PRIMARY KEY); INSERT INTO businesses VALUES('business-a');
             CREATE TABLE stores(id TEXT); CREATE TABLE users(id TEXT); CREATE TABLE roles(id TEXT); CREATE TABLE products(id TEXT);
             CREATE TABLE sales(id TEXT); CREATE TABLE sale_items(id TEXT); CREATE TABLE cash_shifts(id TEXT); CREATE TABLE refunds(id TEXT);
             CREATE TABLE payment_integration_requests(id TEXT);",
        )
        .unwrap();
        AppState {
            data_dir: PathBuf::from("C:/nimbletill-test"),
            device_id: Uuid::new_v4().to_string(),
            db: Mutex::new(db),
            active_session: Mutex::new(None),
            session_gate: std::sync::RwLock::new(()),
            passkey_operation: Mutex::new(()),
        }
    }

    fn empty_backup_test_state(data_dir: PathBuf) -> AppState {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE businesses(id TEXT PRIMARY KEY);
             CREATE TABLE stores(id TEXT); CREATE TABLE users(id TEXT); CREATE TABLE roles(id TEXT); CREATE TABLE products(id TEXT);
             CREATE TABLE sales(id TEXT); CREATE TABLE sale_items(id TEXT); CREATE TABLE cash_shifts(id TEXT); CREATE TABLE refunds(id TEXT);
             CREATE TABLE payment_integration_requests(id TEXT);",
        )
        .unwrap();
        AppState {
            data_dir,
            device_id: Uuid::new_v4().to_string(),
            db: Mutex::new(db),
            active_session: Mutex::new(None),
            session_gate: std::sync::RwLock::new(()),
            passkey_operation: Mutex::new(()),
        }
    }

    #[test]
    fn backup_retention_defaults_to_unlimited_and_rejects_unknown_values() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE app_settings(business_id TEXT,setting_key TEXT,setting_value TEXT,updated_at TEXT);
             INSERT INTO app_settings VALUES('business-a','backup_retention_count','25','now');",
        )
        .unwrap();
        assert_eq!(read_retention_limit(&db, "business-a").unwrap(), 25);
        assert_eq!(read_retention_limit(&db, "business-b").unwrap(), 0);
        db.execute(
            "UPDATE app_settings SET setting_value='3' WHERE business_id='business-a'",
            [],
        )
        .unwrap();
        assert!(read_retention_limit(&db, "business-a").is_err());
    }

    #[test]
    fn retention_prunes_only_old_regular_backups_after_a_verified_snapshot_exists() {
        let directory =
            std::env::temp_dir().join(format!("nimbletill-retention-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE businesses(id TEXT PRIMARY KEY); INSERT INTO businesses VALUES('business-a');
             CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT); INSERT INTO stores VALUES('store-a','business-a');
             CREATE TABLE users(id TEXT PRIMARY KEY,business_id TEXT); INSERT INTO users VALUES('user-a','business-a');
             CREATE TABLE roles(id TEXT); CREATE TABLE products(id TEXT); CREATE TABLE sales(id TEXT);
             CREATE TABLE sale_items(id TEXT); CREATE TABLE cash_shifts(id TEXT); CREATE TABLE refunds(id TEXT);
             CREATE TABLE payment_integration_requests(id TEXT);
             CREATE TABLE audit_events(id TEXT,business_id TEXT,store_id TEXT,user_id TEXT,action TEXT,entity_type TEXT,entity_id TEXT,details_json TEXT,created_at TEXT);",
        )
        .unwrap();
        let state = AppState {
            data_dir: directory.clone(),
            device_id: Uuid::new_v4().to_string(),
            db: Mutex::new(db),
            active_session: Mutex::new(None),
            session_gate: std::sync::RwLock::new(()),
            passkey_operation: Mutex::new(()),
        };
        {
            let db = state.db.lock().unwrap();
            for id in ["backup-a", "backup-b", "backup-c", "rollback-a"] {
                create_snapshot(&db, &backup_path(&state, id).unwrap()).unwrap();
            }
        }
        let user = SessionUser {
            user_id: "user-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Owner".into(),
            username: "owner".into(),
            role_key: "admin".into(),
            role_name: "Administrator".into(),
            workspace_mode: "management".into(),
            is_owner: true,
            permissions: vec!["settings:write".into()],
            business_name: "Test".into(),
            store_name: "Main".into(),
            currency_code: "GHS".into(),
        };
        prune_created_backups(&state, &user, 1).unwrap();
        let remaining = list_backups_for_business(&state, "business-a").unwrap();
        assert_eq!(
            remaining
                .iter()
                .filter(|backup| backup.id.starts_with("backup-"))
                .count(),
            1
        );
        assert!(remaining.iter().any(|backup| backup.id == "rollback-a"));
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn backup_validation_checks_integrity_schema_and_business_identity() {
        let directory =
            std::env::temp_dir().join(format!("nimbletill-backup-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("snapshot.sqlite3");
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE businesses(id TEXT PRIMARY KEY); INSERT INTO businesses VALUES('business-a');
            CREATE TABLE stores(id TEXT); CREATE TABLE users(id TEXT); CREATE TABLE roles(id TEXT); CREATE TABLE products(id TEXT);
            CREATE TABLE sales(id TEXT); CREATE TABLE sale_items(id TEXT); CREATE TABLE cash_shifts(id TEXT); CREATE TABLE refunds(id TEXT);
            CREATE TABLE payment_integration_requests(id TEXT);").unwrap();
        create_snapshot(&db, &path).unwrap();
        validate_backup(&path, "business-a").unwrap();
        assert!(validate_backup(&path, "business-b")
            .unwrap_err()
            .contains("different business"));
        let source = Connection::open(&path).unwrap();
        let mut destination = Connection::open_in_memory().unwrap();
        destination
            .execute_batch("CREATE TABLE businesses(id TEXT PRIMARY KEY); INSERT INTO businesses VALUES('old-business');")
            .unwrap();
        Backup::new(&source, &mut destination)
            .unwrap()
            .run_to_completion(16, Duration::from_millis(1), None)
            .unwrap();
        assert_eq!(read_backup_business_id(&destination).unwrap(), "business-a");
        drop(source);
        fs::remove_file(&path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn backup_ids_cannot_escape_the_backup_folder() {
        let state = AppState {
            data_dir: PathBuf::from("C:/nimbletill"),
            device_id: Uuid::new_v4().to_string(),
            db: Mutex::new(Connection::open_in_memory().unwrap()),
            active_session: Mutex::new(None),
            session_gate: std::sync::RwLock::new(()),
            passkey_operation: Mutex::new(()),
        };
        assert!(backup_path(&state, "../outside").is_err());
        assert!(backup_path(&state, "backup-good-id")
            .unwrap()
            .starts_with("C:/nimbletill/backups"));
    }

    #[test]
    fn portable_backup_encrypts_round_trips_and_rejects_wrong_password_or_tampering() {
        let password = "a long backup passphrase";
        let plaintext = b"SQLite snapshot bytes";
        let encrypted = encrypt_portable_backup(password, plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        assert_eq!(
            &*decrypt_portable_backup(password, &encrypted).unwrap(),
            plaintext
        );
        assert!(decrypt_portable_backup("a different backup passphrase", &encrypted).is_err());
        let mut tampered = encrypted;
        *tampered.last_mut().unwrap() ^= 1;
        assert!(decrypt_portable_backup(password, &tampered).is_err());
        assert!(encrypt_portable_backup("short", plaintext).is_err());
    }

    #[test]
    fn portable_snapshot_stays_in_memory_and_validates_business_before_restore() {
        let state = backup_test_state();
        let password = "a long backup passphrase";
        let plaintext = serialize_snapshot(&state, "business-a").unwrap();
        let encrypted = encrypt_portable_backup(password, &plaintext).unwrap();
        let restored_bytes = decrypt_portable_backup(password, &encrypted).unwrap();
        let source = connection_from_portable_bytes(&restored_bytes).unwrap();
        assert_eq!(read_backup_business_id(&source).unwrap(), "business-a");
        assert!(serialize_snapshot(&state, "another-business").is_err());
    }

    #[test]
    fn portable_restore_can_initialize_an_empty_device_but_not_replace_a_signed_out_store() {
        let directory =
            std::env::temp_dir().join(format!("nimbletill-portable-test-{}", Uuid::new_v4()));
        let empty_state = empty_backup_test_state(directory.clone());
        let populated_state = backup_test_state();
        let mut source = Connection::open_in_memory().unwrap();
        {
            let populated = populated_state.db.lock().unwrap();
            Backup::new(&populated, &mut source)
                .unwrap()
                .run_to_completion(16, Duration::from_millis(1), None)
                .unwrap();
        }
        restore_verified_source(
            &empty_state,
            None,
            &source,
            "portable-test",
            "database.backup.portable_restored",
        )
        .unwrap();
        let restored_id = {
            let restored = empty_state.db.lock().unwrap();
            read_backup_business_id(&restored).unwrap()
        };
        assert_eq!(restored_id, "business-a");
        assert!(restore_verified_source(
            &populated_state,
            None,
            &source,
            "portable-test",
            "database.backup.portable_restored",
        )
        .unwrap_err()
        .contains("Sign in"));
        fs::remove_dir_all(directory).unwrap();
    }
}
