// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::PaymentMethod,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentOption {
    pub id: Option<String>,
    pub name: String,
    pub kind: PaymentMethod,
    pub provider: String,
    pub merchant_account: String,
    pub terminal_id: String,
    pub instructions: String,
    pub reference_required: bool,
    pub enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentDetail {
    pub label: String,
    pub provider: String,
    pub terminal_id: String,
    pub reference: String,
    pub confirmation_source: String,
}

#[tauri::command]
pub fn get_payment_options(state: State<'_, AppState>) -> Result<Vec<PaymentOption>, String> {
    let user = active_session(&state, "")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let mut q=db.prepare("SELECT id,name,kind,provider,merchant_account,terminal_id,instructions,reference_required,enabled FROM payment_options WHERE store_id=?1 ORDER BY name").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([&user.store_id], |r| {
            let kind: String = r.get(2)?;
            let kind = match kind.as_str() {
                "cash" => PaymentMethod::Cash,
                "card" => PaymentMethod::Card,
                "mobile_money" => PaymentMethod::MobileMoney,
                _ => PaymentMethod::Other,
            };
            Ok(PaymentOption {
                id: Some(r.get(0)?),
                name: r.get(1)?,
                kind,
                provider: r.get(3)?,
                merchant_account: r.get(4)?,
                terminal_id: r.get(5)?,
                instructions: r.get(6)?,
                reference_required: r.get(7)?,
                enabled: r.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_payment_option(
    mut input: PaymentOption,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let user = active_session(&state, "settings:write")?;
    input.name = input.name.trim().to_owned();
    if input.name.is_empty()
        || input.name.chars().count() > 80
        || input.provider.len() > 120
        || input.merchant_account.len() > 120
        || input.terminal_id.len() > 120
        || input.instructions.len() > 500
    {
        return Err("Use a name up to 80 characters, account/provider/terminal details up to 120, and instructions up to 500.".to_owned());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let id = if let Some(id) = &input.id {
        let own: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM payment_options WHERE id=?1 AND store_id=?2)",
                params![id, user.store_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !own {
            return Err("This payment method does not belong to this store.".to_owned());
        }
        id.clone()
    } else {
        Uuid::new_v4().to_string()
    };
    tx.execute("INSERT INTO payment_options(id,store_id,name,kind,provider,merchant_account,terminal_id,instructions,reference_required,enabled) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,provider=excluded.provider,merchant_account=excluded.merchant_account,terminal_id=excluded.terminal_id,instructions=excluded.instructions,reference_required=excluded.reference_required,enabled=excluded.enabled",
        params![id,user.store_id,input.name,input.kind.as_str(),input.provider,input.merchant_account,input.terminal_id,input.instructions,input.reference_required,input.enabled]).map_err(|_|"This method could not be saved. Check that its name is unique.".to_owned())?;
    add_audit_event(
        &tx,
        &user,
        "payment_method_saved",
        "payment_option",
        Some(&id),
        serde_json::json!({
            "kind": input.kind.as_str(),
            "enabled": input.enabled,
            "referenceRequired": input.reference_required
        }),
    )?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn validate(
    db: &Connection,
    store_id: &str,
    option_id: Option<&str>,
    kind: PaymentMethod,
    reference: &str,
) -> Result<PaymentDetail, String> {
    if reference.chars().count() > 120 {
        return Err("The payment reference must be 120 characters or fewer.".to_owned());
    }
    let source = if matches!(kind, PaymentMethod::Cash) {
        "cashier_cash"
    } else {
        "manual_external"
    };
    let Some(id) = option_id else {
        let configured: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM payment_options WHERE store_id=?1)",
                [store_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if configured {
            return Err(
                "Choose an enabled payment method configured by your administrator.".to_owned(),
            );
        }
        return Ok(PaymentDetail {
            label: kind.as_str().replace('_', " "),
            provider: String::new(),
            terminal_id: String::new(),
            reference: reference.trim().into(),
            confirmation_source: source.into(),
        });
    };
    let option=db.query_row("SELECT name,kind,provider,terminal_id,reference_required FROM payment_options WHERE id=?1 AND store_id=?2 AND enabled=1",params![id,store_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?))).optional().map_err(|e|e.to_string())?;
    let Some((label, saved_kind, provider, terminal_id, required)) = option else {
        return Err("This payment method is unavailable. Choose another method.".to_owned());
    };
    if saved_kind != kind.as_str() {
        return Err("The payment method changed. Select it again.".to_owned());
    }
    if required && reference.trim().is_empty() {
        return Err("Enter the approved terminal/mobile-money transaction reference.".to_owned());
    }
    Ok(PaymentDetail {
        label,
        provider,
        terminal_id,
        reference: reference.trim().into(),
        confirmation_source: source.into(),
    })
}
pub(crate) fn record(
    db: &Connection,
    sale_id: &str,
    option_id: Option<&str>,
    detail: &PaymentDetail,
) -> Result<(), String> {
    db.execute("INSERT INTO sale_payment_details(sale_id,option_id,label,provider,terminal_id,reference,confirmation_source) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![sale_id,option_id,detail.label,detail.provider,detail.terminal_id,detail.reference,detail.confirmation_source]).map_err(|e|e.to_string())?;
    Ok(())
}
pub(crate) fn receipt_detail(
    db: &Connection,
    sale_id: &str,
) -> Result<Option<PaymentDetail>, String> {
    db.query_row("SELECT label,provider,terminal_id,reference,confirmation_source FROM sale_payment_details WHERE sale_id=?1",[sale_id],|r|Ok(PaymentDetail{label:r.get(0)?,provider:r.get(1)?,terminal_id:r.get(2)?,reference:r.get(3)?,confirmation_source:r.get(4)?})).optional().map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE stores(id TEXT PRIMARY KEY); INSERT INTO stores VALUES('store');",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0008_payment_options.sql"))
            .unwrap();
        db.execute("INSERT INTO payment_options VALUES('terminal','store','Bank POS','card','Bank','','Till 1','Confirm approval',1,1)",[]).unwrap();
        db
    }
    #[test]
    fn validates_reference_kind_store_and_enabled_state() {
        let db = fixture();
        assert!(validate(&db, "store", Some("terminal"), PaymentMethod::Card, "").is_err());
        assert!(validate(
            &db,
            "other-store",
            Some("terminal"),
            PaymentMethod::Card,
            "ref"
        )
        .is_err());
        assert!(validate(&db, "store", Some("terminal"), PaymentMethod::Cash, "ref").is_err());
        let details = validate(
            &db,
            "store",
            Some("terminal"),
            PaymentMethod::Card,
            "approved-123",
        )
        .unwrap();
        assert_eq!(details.confirmation_source, "manual_external");
        assert_eq!(details.terminal_id, "Till 1");
        db.execute("UPDATE payment_options SET enabled=0", [])
            .unwrap();
        assert!(validate(&db, "store", Some("terminal"), PaymentMethod::Card, "ref").is_err());
    }
    #[test]
    fn configured_methods_cannot_be_bypassed_with_legacy_checkout() {
        let db = fixture();
        assert!(validate(&db, "store", None, PaymentMethod::Card, "ref").is_err());
        assert!(validate(&db, "unconfigured", None, PaymentMethod::Cash, "").is_ok());
    }
}
