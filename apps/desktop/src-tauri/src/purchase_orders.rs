// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::{
        PurchaseOrderInput, PurchaseOrderLine, PurchaseOrderSummary, PurchaseOrderTransitionInput,
    },
};

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn clean(value: Option<String>, max: usize) -> Result<Option<String>, String> {
    let value = value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    if value
        .as_ref()
        .is_some_and(|text| text.chars().count() > max)
    {
        return Err(format!("Keep this value under {max} characters."));
    }
    Ok(value)
}

fn load_order(
    connection: &Connection,
    store_id: &str,
    id: &str,
) -> Result<PurchaseOrderSummary, String> {
    let header = connection.query_row(
        "SELECT po.id,po.order_number,po.supplier_id,s.name,po.reference,po.status,po.revision,po.placed_at,po.updated_at
         FROM purchase_orders po LEFT JOIN suppliers s ON s.id=po.supplier_id
         WHERE po.store_id=?1 AND po.id=?2",
        params![store_id, id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?, row.get::<_, i64>(6)?, row.get::<_, Option<String>>(7)?, row.get::<_, String>(8)?)),
    ).optional().map_err(|error| error.to_string())?.ok_or_else(|| "Purchase order not found for this store.".to_owned())?;
    let mut statement = connection.prepare(
        "SELECT pol.id,pol.product_id,pol.product_name,pol.sku,pol.ordered_quantity,pol.received_quantity,pol.expected_unit_cost_minor
         FROM purchase_order_lines pol JOIN purchase_orders po ON po.id=pol.order_id
         WHERE po.store_id=?1 AND po.id=?2 ORDER BY pol.rowid",
    ).map_err(|error| error.to_string())?;
    let lines = statement
        .query_map(params![store_id, id], |row| {
            Ok(PurchaseOrderLine {
                id: row.get(0)?,
                product_id: row.get(1)?,
                product_name: row.get(2)?,
                sku: row.get(3)?,
                ordered_quantity: row.get(4)?,
                received_quantity: row.get(5)?,
                expected_unit_cost_minor: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let total_estimated_minor = lines
        .iter()
        .try_fold(0_i64, |total, line| {
            line.expected_unit_cost_minor
                .checked_mul(line.ordered_quantity)
                .and_then(|value| total.checked_add(value))
        })
        .ok_or_else(|| "The purchase order total is too large.".to_owned())?;
    Ok(PurchaseOrderSummary {
        id: header.0,
        order_number: header.1,
        supplier_id: header.2,
        supplier_name: header.3,
        reference: header.4,
        status: header.5,
        revision: header.6,
        placed_at: header.7,
        updated_at: header.8,
        total_estimated_minor,
        lines,
    })
}

#[tauri::command]
pub fn list_purchase_orders(
    state: State<'_, AppState>,
) -> Result<Vec<PurchaseOrderSummary>, String> {
    list_purchase_orders_for_state(&state)
}

pub(crate) fn list_purchase_orders_for_state(
    state: &AppState,
) -> Result<Vec<PurchaseOrderSummary>, String> {
    let session = active_session(state, "purchases:read")?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let ids = {
        let mut statement = connection
            .prepare(
                "SELECT id FROM purchase_orders WHERE store_id=?1 ORDER BY updated_at DESC,id DESC",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([&session.store_id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    ids.iter()
        .map(|id| load_order(&connection, &session.store_id, id))
        .collect()
}

#[tauri::command]
pub fn save_purchase_order(
    input: PurchaseOrderInput,
    state: State<'_, AppState>,
) -> Result<PurchaseOrderSummary, String> {
    save_purchase_order_for_state(input, &state)
}

pub(crate) fn save_purchase_order_for_state(
    input: PurchaseOrderInput,
    state: &AppState,
) -> Result<PurchaseOrderSummary, String> {
    let session = active_session(state, "purchases:write")?;
    if input.lines.is_empty() || input.lines.len() > 100 {
        return Err("Add between 1 and 100 products to this order.".to_owned());
    }
    let supplier_id = clean(input.supplier_id, 80)?;
    let reference = clean(input.reference, 100)?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(supplier_id) = supplier_id.as_deref() {
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM suppliers WHERE id=?1 AND store_id=?2 AND is_active=1)", params![supplier_id, session.store_id], |row| row.get(0)).map_err(|error| error.to_string())?;
        if !valid {
            return Err("Choose an active supplier from this store.".to_owned());
        }
    }
    let mut lines = Vec::with_capacity(input.lines.len());
    let mut seen = std::collections::HashSet::new();
    let line_count = input.lines.len();
    for line in input.lines {
        let product_id = line.product_id.trim().to_owned();
        if product_id.is_empty() || !seen.insert(product_id.clone()) {
            return Err("Each product can appear only once in an order.".to_owned());
        }
        if line.quantity <= 0 || line.quantity > 1_000_000 || line.expected_unit_cost_minor < 0 {
            return Err("Enter valid quantities and estimated unit costs.".to_owned());
        }
        let (sku, name, active): (String, String, bool) = tx
            .query_row(
                "SELECT sku,name,is_active FROM products WHERE id=?1 AND store_id=?2",
                params![product_id, session.store_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0)),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "An order product does not belong to this store.".to_owned())?;
        if !active {
            return Err(format!("{name} is archived and cannot be ordered."));
        }
        line.expected_unit_cost_minor
            .checked_mul(line.quantity)
            .ok_or_else(|| "The purchase order total is too large.".to_owned())?;
        lines.push((
            Uuid::new_v4().to_string(),
            product_id,
            name,
            sku,
            line.quantity,
            line.expected_unit_cost_minor,
        ));
    }
    let timestamp = now();
    let id = if let Some(id) = input.id.as_deref() {
        let revision = input
            .revision
            .ok_or_else(|| "Reload the order before saving changes.".to_owned())?;
        let changed = tx.execute("UPDATE purchase_orders SET supplier_id=?1,reference=?2,revision=revision+1,updated_at=?3 WHERE id=?4 AND store_id=?5 AND status='draft' AND revision=?6", params![supplier_id, reference, timestamp, id, session.store_id, revision]).map_err(|error| error.to_string())?;
        if changed != 1 {
            return Err(
                "This draft changed or is no longer editable. Reload it and try again.".to_owned(),
            );
        }
        tx.execute("DELETE FROM purchase_order_lines WHERE order_id=?1", [id])
            .map_err(|error| error.to_string())?;
        id.to_owned()
    } else {
        let sequence: i64 = tx.query_row("SELECT COALESCE(MAX(CAST(substr(order_number,4) AS INTEGER)),0)+1 FROM purchase_orders WHERE store_id=?1", [&session.store_id], |row| row.get(0)).map_err(|error| error.to_string())?;
        let order_number = format!("PO-{sequence:06}");
        let id = Uuid::new_v4().to_string();
        tx.execute("INSERT INTO purchase_orders(id,store_id,supplier_id,created_by,order_number,reference,status,revision,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'draft',1,?7,?7)", params![id, session.store_id, supplier_id, session.user_id, order_number, reference, timestamp]).map_err(|error| error.to_string())?;
        id
    };
    for (line_id, product_id, name, sku, quantity, unit_cost) in lines {
        tx.execute("INSERT INTO purchase_order_lines(id,order_id,product_id,product_name,sku,ordered_quantity,expected_unit_cost_minor) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![line_id,id,product_id,name,sku,quantity,unit_cost]).map_err(|error| error.to_string())?;
    }
    add_audit_event(
        &tx,
        &session,
        "purchase_order_saved",
        "purchase_order",
        Some(&id),
        serde_json::json!({"supplierId":supplier_id,"lineCount":line_count}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    load_order(&connection, &session.store_id, &id)
}

#[tauri::command]
pub fn place_purchase_order(
    input: PurchaseOrderTransitionInput,
    state: State<'_, AppState>,
) -> Result<PurchaseOrderSummary, String> {
    transition_order_for_state(input, &state, "placed")
}

#[tauri::command]
pub fn cancel_purchase_order(
    input: PurchaseOrderTransitionInput,
    state: State<'_, AppState>,
) -> Result<PurchaseOrderSummary, String> {
    transition_order_for_state(input, &state, "cancelled")
}

fn transition_order_for_state(
    input: PurchaseOrderTransitionInput,
    state: &AppState,
    target: &str,
) -> Result<PurchaseOrderSummary, String> {
    let session = active_session(state, "purchases:write")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let current: Option<String> = tx
        .query_row(
            "SELECT status FROM purchase_orders WHERE id=?1 AND store_id=?2 AND revision=?3",
            params![input.id, session.store_id, input.revision],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let current = current.ok_or_else(|| {
        "This order changed or does not belong to this store. Reload it and try again.".to_owned()
    })?;
    let valid = match (target, current.as_str()) {
        ("placed", "draft") => true,
        ("cancelled", "draft" | "placed" | "partially_received") => true,
        _ => false,
    };
    if !valid {
        return Err("This purchase order cannot make that status change.".to_owned());
    }
    if target == "placed" {
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM purchase_order_lines WHERE order_id=?1",
                [&input.id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Add at least one product before placing this order.".to_owned());
        }
    }
    let timestamp = now();
    let changed = if target == "placed" {
        tx.execute("UPDATE purchase_orders SET status='placed',placed_at=?1,revision=revision+1,updated_at=?1 WHERE id=?2 AND store_id=?3 AND revision=?4",params![timestamp,input.id,session.store_id,input.revision])
    } else {
        tx.execute("UPDATE purchase_orders SET status='cancelled',cancelled_at=?1,revision=revision+1,updated_at=?1 WHERE id=?2 AND store_id=?3 AND revision=?4",params![timestamp,input.id,session.store_id,input.revision])
    }.map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err("This order changed while being updated. Reload and try again.".to_owned());
    }
    add_audit_event(
        &tx,
        &session,
        if target == "placed" {
            "purchase_order_placed"
        } else {
            "purchase_order_cancelled"
        },
        "purchase_order",
        Some(&input.id),
        serde_json::json!({"from":current,"to":target}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    load_order(&connection, &session.store_id, &input.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::purchase_receipt_tests::receipt_test_state;

    #[test]
    fn draft_place_and_cancel_are_non_economic_and_use_revision_checks() {
        let state = receipt_test_state();
        let draft = save_purchase_order_for_state(
            PurchaseOrderInput {
                id: None,
                revision: None,
                supplier_id: Some("supplier-a".into()),
                reference: Some("QUOTE-1".into()),
                lines: vec![crate::models::PurchaseOrderLineInput {
                    product_id: "product-a".into(),
                    quantity: 4,
                    expected_unit_cost_minor: 130,
                }],
            },
            &state,
        )
        .unwrap();
        assert_eq!(draft.status, "draft");
        assert_eq!(draft.total_estimated_minor, 520);

        let placed = transition_order_for_state(
            PurchaseOrderTransitionInput {
                id: draft.id.clone(),
                revision: draft.revision,
            },
            &state,
            "placed",
        )
        .unwrap();
        assert_eq!(placed.status, "placed");
        assert!(transition_order_for_state(
            PurchaseOrderTransitionInput {
                id: draft.id.clone(),
                revision: draft.revision,
            },
            &state,
            "cancelled",
        )
        .is_err());

        let cancelled = transition_order_for_state(
            PurchaseOrderTransitionInput {
                id: placed.id.clone(),
                revision: placed.revision,
            },
            &state,
            "cancelled",
        )
        .unwrap();
        assert_eq!(cancelled.status, "cancelled");

        let connection = state.db.lock().unwrap();
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT stock_quantity FROM products WHERE id='product-a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            10
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM purchases", [], |row| row.get(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM stock_movements", [], |row| row
                    .get(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM accounting_journals", [], |row| row
                    .get(0))
                .unwrap(),
            0
        );
    }
}
