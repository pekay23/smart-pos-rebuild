// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{commands::active_session, db::AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentIntegrationRequest {
    pub id: String,
    pub country: String,
    pub provider_name: String,
    pub terminal_model: String,
    pub capability: String,
    pub details: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewPaymentIntegrationRequest {
    pub country: String,
    pub provider_name: String,
    pub terminal_model: String,
    pub capability: String,
    pub details: String,
}

fn validate_text(value: &str, label: &str, max: usize, required: bool) -> Result<String, String> {
    let value = value.trim();
    if (required && value.is_empty()) || value.chars().count() > max {
        return Err(format!(
            "{label} is required and must be no longer than {max} characters."
        ));
    }
    Ok(value.to_owned())
}

#[tauri::command]
pub fn list_payment_integration_requests(
    state: State<'_, AppState>,
) -> Result<Vec<PaymentIntegrationRequest>, String> {
    let user = active_session(&state, "settings:write")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let mut statement = db
        .prepare("SELECT id,country,provider_name,terminal_model,capability,details,status,created_at FROM payment_integration_requests WHERE store_id=?1 ORDER BY created_at DESC LIMIT 100")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([&user.store_id], |row| {
            Ok(PaymentIntegrationRequest {
                id: row.get(0)?,
                country: row.get(1)?,
                provider_name: row.get(2)?,
                terminal_model: row.get(3)?,
                capability: row.get(4)?,
                details: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_payment_integration_request(
    input: NewPaymentIntegrationRequest,
    state: State<'_, AppState>,
) -> Result<PaymentIntegrationRequest, String> {
    let user = active_session(&state, "settings:write")?;
    let country = validate_text(&input.country, "Country", 80, true)?;
    let provider_name = validate_text(&input.provider_name, "Bank or payment provider", 120, true)?;
    let terminal_model = validate_text(&input.terminal_model, "Terminal model", 120, false)?;
    let capability = validate_text(&input.capability, "Requested capability", 80, true)?;
    let details = validate_text(&input.details, "Request details", 1000, false)?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    db.execute(
        "INSERT INTO payment_integration_requests(id,store_id,requested_by,country,provider_name,terminal_model,capability,details,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'local_only',?9,?9)",
        params![id, user.store_id, user.user_id, country, provider_name, terminal_model, capability, details, now],
    ).map_err(|error| error.to_string())?;
    Ok(PaymentIntegrationRequest {
        id,
        country,
        provider_name,
        terminal_model,
        capability,
        details,
        status: "local_only".into(),
        created_at: now,
    })
}

#[tauri::command]
pub fn get_payment_integration_request_export(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<PaymentIntegrationRequest, String> {
    let user = active_session(&state, "settings:write")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    db.query_row(
        "SELECT id,country,provider_name,terminal_model,capability,details,status,created_at FROM payment_integration_requests WHERE id=?1 AND store_id=?2",
        params![request_id, user.store_id],
        |row| Ok(PaymentIntegrationRequest {
            id: row.get(0)?, country: row.get(1)?, provider_name: row.get(2)?, terminal_model: row.get(3)?,
            capability: row.get(4)?, details: row.get(5)?, status: row.get(6)?, created_at: row.get(7)?,
        }),
    ).optional().map_err(|error| error.to_string())?
        .ok_or_else(|| "This request is not available in the current store.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_request_fields_are_trimmed_bounded_and_required_as_expected() {
        assert_eq!(
            validate_text(" Ghana ", "Country", 80, true).unwrap(),
            "Ghana"
        );
        assert!(validate_text(" ", "Country", 80, true).is_err());
        assert_eq!(
            validate_text(" ", "Terminal model", 120, false).unwrap(),
            ""
        );
        assert!(validate_text(&"x".repeat(81), "Country", 80, true).is_err());
    }
}
