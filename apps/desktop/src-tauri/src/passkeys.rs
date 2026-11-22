// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    attendance,
    commands::{active_session, add_audit_event, verify_password},
    db::AppState,
    models::SessionUser,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use uuid::Uuid;
use webauthn_rs::prelude::*;

// This reserved local RP deliberately does not impersonate the future cloud domain.
const RP_ID: &str = "nimbletill.localhost";
const ORIGIN: &str = "https://nimbletill.localhost";
const CEREMONY_SECONDS: u64 = 120;

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn relying_party() -> Result<Webauthn, String> {
    let origin = Url::parse(ORIGIN).map_err(|e| e.to_string())?;
    WebauthnBuilder::new(RP_ID, &origin)
        .map_err(|e| e.to_string())?
        .rp_name("NimbleTill local accounts")
        .timeout(Duration::from_secs(CEREMONY_SECONDS))
        .build()
        .map_err(|e| e.to_string())
}
fn main_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Open account security in the main application.".to_owned());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeySummary {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

fn stored_keys(db: &Connection, user_id: &str) -> Result<Vec<(String, Passkey)>, String> {
    let mut query = db
        .prepare("SELECT id,credential_json FROM personal_passkeys WHERE user_id=?1")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([user_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        let (id, json) = row.map_err(|e| e.to_string())?;
        Ok((
            id,
            serde_json::from_str(&json)
                .map_err(|_| "A saved passkey could not be read.".to_owned())?,
        ))
    })
    .collect()
}

pub(crate) fn verify_account_password(
    db: &Connection,
    user_id: &str,
    password: &str,
) -> Result<(), String> {
    let locked: Option<String> = db
        .query_row(
            "SELECT locked_until FROM account_password_attempts WHERE user_id=?1",
            [user_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    if let Some(remaining_seconds) = locked
        .as_deref()
        .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
        .map(|until| {
            let milliseconds = until
                .with_timezone(&Utc)
                .signed_duration_since(Utc::now())
                .num_milliseconds()
                .max(0);
            (milliseconds + 999) / 1000
        })
        .filter(|seconds| *seconds > 0)
    {
        return Err(format!(
            "Too many attempts. Wait {remaining_seconds} seconds and try again."
        ));
    }
    let hash: Option<String> = db
        .query_row(
            "SELECT password_hash FROM users WHERE id=?1 AND is_active=1",
            [user_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if password.len() > 1024 || !hash.is_some_and(|hash| verify_password(password, &hash)) {
        let until = (Utc::now() + chrono::Duration::seconds(30))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        db.execute("INSERT INTO account_password_attempts(user_id,failures) VALUES(?1,1) ON CONFLICT(user_id) DO UPDATE SET failures=failures+1,locked_until=CASE WHEN failures+1>=5 THEN ?2 ELSE NULL END",params![user_id,until]).map_err(|e|e.to_string())?;
        return Err("Your password was not recognized.".to_owned());
    }
    db.execute(
        "DELETE FROM account_password_attempts WHERE user_id=?1",
        [user_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn load_user(db: &Connection, username: &str) -> Result<SessionUser, String> {
    db.query_row("SELECT u.id,u.business_id,s.id,u.full_name,u.username,r.role_key,r.display_name,r.permissions_json,b.name,s.name,b.currency_code,r.workspace_mode,COALESCE(b.owner_user_id=u.id,0)
        FROM users u JOIN roles r ON r.id=u.role_id AND r.business_id=u.business_id JOIN businesses b ON b.id=u.business_id JOIN stores s ON s.business_id=b.id AND s.is_active=1
        WHERE u.username=?1 AND u.is_active=1 ORDER BY s.created_at LIMIT 1",[username.trim()],|r|Ok(SessionUser{
        user_id:r.get(0)?,business_id:r.get(1)?,store_id:r.get(2)?,full_name:r.get(3)?,username:r.get(4)?,role_key:r.get(5)?,role_name:r.get(6)?,
        permissions:serde_json::from_str(&r.get::<_,String>(7)?).unwrap_or_default(),business_name:r.get(8)?,store_name:r.get(9)?,currency_code:r.get(10)?,workspace_mode:r.get(11)?,is_owner:r.get::<_,i64>(12)?!=0,
    })).optional().map_err(|e|e.to_string())?.ok_or_else(||"No usable passkey was found for this account on this installation. Use your password.".to_owned())
}

#[tauri::command]
pub fn passkeys_available() -> bool {
    cfg!(target_os = "windows")
}

#[tauri::command]
pub fn list_passkeys(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<PasskeySummary>, String> {
    main_window(&window)?;
    let user = active_session(&state, "")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let mut query=db.prepare("SELECT id,label,created_at,last_used_at FROM personal_passkeys WHERE user_id=?1 ORDER BY created_at DESC").map_err(|e|e.to_string())?;
    let rows = query
        .query_map([&user.user_id], |r| {
            Ok(PasskeySummary {
                id: r.get(0)?,
                label: r.get(1)?,
                created_at: r.get(2)?,
                last_used_at: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn register_passkey(
    app: AppHandle,
    window: WebviewWindow,
    label: String,
    password: String,
    personal_device: bool,
) -> Result<(), String> {
    main_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        register(&app.state::<AppState>(), label, password, personal_device)
    })
    .await
    .map_err(|_| "Passkey setup could not finish.".to_owned())?
}

fn register(
    state: &AppState,
    label: String,
    password: String,
    personal_device: bool,
) -> Result<(), String> {
    let _operation = state
        .passkey_operation
        .try_lock()
        .map_err(|_| "Another passkey request is in progress.")?;
    let user = active_session(state, "")?;
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 80 {
        return Err("Name this passkey using 1 to 80 characters.".to_owned());
    }
    let keys = {
        let db = state.db.lock().map_err(|_| "Database unavailable.")?;
        verify_account_password(&db, &user.user_id, &password)?;
        stored_keys(&db, &user.user_id)?
    };
    if keys.len() >= 10 {
        return Err("You can register up to 10 passkeys. Remove an unused one first.".to_owned());
    }
    let rp = relying_party()?;
    let (mut options, challenge) = rp
        .start_passkey_registration(
            Uuid::parse_str(&user.user_id).map_err(|e| e.to_string())?,
            &user.username,
            &user.full_name,
            Some(keys.iter().map(|(_, key)| key.cred_id().clone()).collect()),
        )
        .map_err(|e| e.to_string())?;
    if !personal_device {
        if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
            selection.authenticator_attachment = Some(AuthenticatorAttachment::CrossPlatform);
        }
    }
    let started = Instant::now();
    let credential = native_register(options)?;
    if started.elapsed() > Duration::from_secs(CEREMONY_SECONDS) {
        return Err("Passkey setup expired. Try again.".to_owned());
    }
    let key = rp
        .finish_passkey_registration(&credential, &challenge)
        .map_err(|_| "The passkey could not be verified. Try again.".to_owned())?;
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO personal_passkeys(id,user_id,credential_id,label,credential_json,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id,user.user_id,serde_json::to_string(key.cred_id()).map_err(|e|e.to_string())?,label,serde_json::to_string(&key).map_err(|e|e.to_string())?,now()]).map_err(|_|"This passkey is already registered or could not be saved.".to_owned())?;
    add_audit_event(
        &tx,
        &user,
        "passkey_registered",
        "passkey",
        Some(&id),
        serde_json::json!({"label":label,"personalDevice":personal_device}),
    )?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn login_passkey(
    app: AppHandle,
    window: WebviewWindow,
    username: String,
) -> Result<SessionUser, String> {
    main_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || authenticate(&app.state::<AppState>(), username))
        .await
        .map_err(|_| "Passkey sign-in could not finish.".to_owned())?
}

fn authenticate(state: &AppState, username: String) -> Result<SessionUser, String> {
    if username.trim().is_empty() || username.chars().count() > 50 {
        return Err("Enter your username first.".to_owned());
    }
    let _operation = state
        .passkey_operation
        .try_lock()
        .map_err(|_| "Another passkey request is in progress.")?;
    let _gate = state
        .session_gate
        .write()
        .map_err(|_| "Session unavailable.")?;
    if state
        .active_session
        .lock()
        .map_err(|_| "Session unavailable.")?
        .is_some()
    {
        return Err("Sign out before using the login screen.".to_owned());
    }
    let (user, mut keys) = {
        let db = state.db.lock().map_err(|_| "Database unavailable.")?;
        let user = load_user(&db, &username)?;
        let keys = stored_keys(&db, &user.user_id)?;
        (user, keys)
    };
    if keys.is_empty() {
        return Err("No passkey is registered for this account on this installation. Sign in with your password and open My account.".to_owned());
    }
    let rp = relying_party()?;
    let (options, challenge) = rp
        .start_passkey_authentication(&keys.iter().map(|(_, key)| key.clone()).collect::<Vec<_>>())
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let response = native_authenticate(options)?;
    if started.elapsed() > Duration::from_secs(CEREMONY_SECONDS) {
        return Err("Passkey sign-in expired. Try again.".to_owned());
    }
    let result = rp
        .finish_passkey_authentication(&response, &challenge)
        .map_err(|_| "The passkey could not be verified.".to_owned())?;
    let (id, key) = keys
        .iter_mut()
        .find(|(_, key)| key.cred_id() == result.cred_id())
        .ok_or("The passkey does not belong to this account.")?;
    let id = id.clone();
    key.update_credential(&result);
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let current = load_user(&tx, &username)?;
    if current.user_id != user.user_id {
        return Err("The account changed. Sign in again.".to_owned());
    }
    let changed=tx.execute("UPDATE personal_passkeys SET credential_json=?1,last_used_at=?2 WHERE id=?3 AND user_id=?4",
        params![serde_json::to_string(key).map_err(|e|e.to_string())?,now(),id,current.user_id]).map_err(|e|e.to_string())?;
    if changed != 1 {
        return Err("This passkey has been removed.".to_owned());
    }
    attendance::start_for_login(&tx, &current, Utc::now())?;
    add_audit_event(
        &tx,
        &current,
        "passkey_login",
        "passkey",
        Some(&id),
        serde_json::json!({}),
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    *state
        .active_session
        .lock()
        .map_err(|_| "Session unavailable.")? = Some(current.clone());
    Ok(current)
}

#[tauri::command]
pub fn remove_passkey(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    password: String,
) -> Result<(), String> {
    main_window(&window)?;
    let user = active_session(&state, "")?;
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    verify_account_password(&db, &user.user_id, &password)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let count = tx
        .execute(
            "DELETE FROM personal_passkeys WHERE id=?1 AND user_id=?2",
            params![id, user.user_id],
        )
        .map_err(|e| e.to_string())?;
    if count != 1 {
        return Err(
            "That passkey does not belong to your account or has already been removed.".to_owned(),
        );
    }
    add_audit_event(
        &tx,
        &user,
        "passkey_removed",
        "passkey",
        Some(&id),
        serde_json::json!({}),
    )?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn native_register(
    options: CreationChallengeResponse,
) -> Result<RegisterPublicKeyCredential, String> {
    webauthn_authenticator_rs::WebauthnAuthenticator::new(webauthn_authenticator_rs::win10::Win10::default())
        .do_registration(Url::parse(ORIGIN).map_err(|e|e.to_string())?,options)
        .map_err(|_|"Passkey setup was cancelled or the authenticator is unavailable. Use a personal security key or a supported Windows authenticator.".to_owned())
}
#[cfg(target_os = "windows")]
fn native_authenticate(options: RequestChallengeResponse) -> Result<PublicKeyCredential, String> {
    webauthn_authenticator_rs::WebauthnAuthenticator::new(webauthn_authenticator_rs::win10::Win10::default())
        .do_authentication(Url::parse(ORIGIN).map_err(|e|e.to_string())?,options)
        .map_err(|_|"Passkey sign-in was cancelled or the authenticator is unavailable. You can still use your password.".to_owned())
}
#[cfg(not(target_os = "windows"))]
fn native_register(_: CreationChallengeResponse) -> Result<RegisterPublicKeyCredential, String> {
    Err("Native passkeys are currently available in the Windows build.".to_owned())
}
#[cfg(not(target_os = "windows"))]
fn native_authenticate(_: RequestChallengeResponse) -> Result<PublicKeyCredential, String> {
    Err("Native passkeys are currently available in the Windows build.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    // This authenticator is for cryptographic protocol tests only; no test software keys ship.
    #[test]
    fn verifies_passkey_and_rejects_replayed_challenge() {
        let rp = relying_party().unwrap();
        let origin = Url::parse(ORIGIN).unwrap();
        let mut device = webauthn_authenticator_rs::WebauthnAuthenticator::new(
            webauthn_authenticator_rs::softpasskey::SoftPasskey::new(true),
        );
        let (options, registration) = rp
            .start_passkey_registration(Uuid::new_v4(), "cashier", "Cashier", None)
            .unwrap();
        let response = device.do_registration(origin.clone(), options).unwrap();
        let key = rp
            .finish_passkey_registration(&response, &registration)
            .unwrap();
        let saved = serde_json::to_string(&key).unwrap();
        let key: Passkey = serde_json::from_str(&saved).unwrap();
        let (options, challenge) = rp.start_passkey_authentication(&[key.clone()]).unwrap();
        let assertion = device.do_authentication(origin, options).unwrap();
        assert!(rp
            .finish_passkey_authentication(&assertion, &challenge)
            .is_ok());
        let (_, fresh_challenge) = rp.start_passkey_authentication(&[key]).unwrap();
        assert!(rp
            .finish_passkey_authentication(&assertion, &fresh_challenge)
            .is_err());
    }
    #[test]
    fn registration_rejects_wrong_origin() {
        let rp = relying_party().unwrap();
        let mut device = webauthn_authenticator_rs::WebauthnAuthenticator::new(
            webauthn_authenticator_rs::softpasskey::SoftPasskey::new(true),
        );
        let (options, challenge) = rp
            .start_passkey_registration(Uuid::new_v4(), "cashier", "Cashier", None)
            .unwrap();
        let mut response = device
            .do_registration(Url::parse(ORIGIN).unwrap(), options)
            .unwrap();
        response.response.client_data_json=b"{\"type\":\"webauthn.create\",\"origin\":\"https://wrong.example\",\"challenge\":\"wrong\"}".to_vec().into();
        assert!(rp
            .finish_passkey_registration(&response, &challenge)
            .is_err());
    }
    #[test]
    fn password_attempts_lock_and_persist() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE users(id TEXT PRIMARY KEY,password_hash TEXT,is_active INTEGER); CREATE TABLE account_password_attempts(user_id TEXT PRIMARY KEY,failures INTEGER,locked_until TEXT); INSERT INTO users VALUES('u','invalid',1);").unwrap();
        for _ in 0..5 {
            assert!(verify_account_password(&db, "u", "wrong").is_err());
        }
        assert!(verify_account_password(&db, "u", "wrong")
            .unwrap_err()
            .contains("30 seconds"));
    }
}
