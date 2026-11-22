// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::PaymentMethod,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundItemInput {
    pub sale_item_id: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundInput {
    pub sale_id: String,
    pub idempotency_key: String,
    pub kind: String,
    pub reason: String,
    pub items: Vec<RefundItemInput>,
    pub payment_method: PaymentMethod,
    pub payment_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundReceipt {
    pub id: String,
    pub sale_id: String,
    pub receipt_number: String,
    pub kind: String,
    pub total_minor: i64,
    pub reason: String,
    pub payment_method: String,
    pub payment_reference: Option<String>,
    pub created_at: String,
    pub recorded_by: String,
    pub station_key: Option<String>,
    pub station_name_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundableSaleItem {
    pub sale_item_id: String,
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub sold_quantity: i64,
    pub refunded_quantity: i64,
    pub remaining_quantity: i64,
    pub line_subtotal_minor: i64,
    pub line_discount_minor: i64,
    pub line_tax_minor: i64,
    pub refundable_total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleRefundDetail {
    pub sale_id: String,
    pub receipt_number: String,
    pub created_at: String,
    pub cashier_name: String,
    pub status: String,
    pub total_minor: i64,
    pub items: Vec<RefundableSaleItem>,
    pub refunds: Vec<RefundReceipt>,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn payment_method(value: &str) -> Result<&'static str, String> {
    match value {
        "cash" => Ok("cash"),
        "card" => Ok("card"),
        "mobile_money" => Ok("mobile_money"),
        "other" => Ok("other"),
        _ => Err("Choose a valid refund method.".to_owned()),
    }
}

fn refundable_line_total(line_total: i64, discount: i64, tax: i64) -> Result<i64, String> {
    line_total
        .checked_sub(discount)
        .and_then(|value| value.checked_add(tax))
        .filter(|value| *value >= 0)
        .ok_or_else(|| "The saved sale item totals are inconsistent.".to_owned())
}

fn cumulative_part(total: i64, sold: i64, quantity: i64) -> Result<i64, String> {
    if total < 0 || sold <= 0 || quantity < 0 || quantity > sold {
        return Err("The saved sale item totals are inconsistent.".to_owned());
    }
    i64::try_from(i128::from(total) * i128::from(quantity) / i128::from(sold))
        .map_err(|_| "The refund amount is too large.".to_owned())
}

fn components_for_returned_quantity(
    line_total: i64,
    discount: i64,
    tax: i64,
    sold: i64,
    already_refunded: i64,
    returning: i64,
) -> Result<(i64, i64, i64, i64), String> {
    let cumulative = already_refunded
        .checked_add(returning)
        .ok_or_else(|| "The return quantity is too large.".to_owned())?;
    if sold <= 0 || already_refunded < 0 || returning <= 0 || cumulative > sold {
        return Err("The return quantity exceeds the quantity still available.".to_owned());
    }
    let _ = refundable_line_total(line_total, discount, tax)?;
    let subtotal = cumulative_part(line_total, sold, cumulative)?
        - cumulative_part(line_total, sold, already_refunded)?;
    let discount = cumulative_part(discount, sold, cumulative)?
        - cumulative_part(discount, sold, already_refunded)?;
    let tax =
        cumulative_part(tax, sold, cumulative)? - cumulative_part(tax, sold, already_refunded)?;
    let total = subtotal
        .checked_sub(discount)
        .and_then(|value| value.checked_add(tax))
        .ok_or_else(|| "The refund amount is too large.".to_owned())?;
    Ok((subtotal, discount, tax, total))
}

fn refund_by_key(
    connection: &Connection,
    store_id: &str,
    key: &str,
) -> Result<Option<RefundReceipt>, String> {
    connection
        .query_row(
            "SELECT r.id,r.sale_id,s.receipt_number,r.refund_type,r.total_minor,r.reason,
                    t.method,t.reference,r.created_at,u.full_name,r.station_key,r.station_name_snapshot
             FROM refunds r
             JOIN sales s ON s.id=r.sale_id
             JOIN users u ON u.id=r.cashier_id
             JOIN refund_tenders t ON t.refund_id=r.id
             WHERE r.store_id=?1 AND r.idempotency_key=?2",
            params![store_id, key],
            |row| {
                Ok(RefundReceipt {
                    id: row.get(0)?,
                    sale_id: row.get(1)?,
                    receipt_number: row.get(2)?,
                    kind: row.get(3)?,
                    total_minor: row.get(4)?,
                    reason: row.get(5)?,
                    payment_method: row.get(6)?,
                    payment_reference: row.get(7)?,
                    created_at: row.get(8)?,
                    recorded_by: row.get(9)?,
                    station_key: row.get(10)?,
                    station_name_snapshot: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn receipt_for_id(connection: &Connection, id: &str) -> Result<RefundReceipt, String> {
    connection
        .query_row(
            "SELECT r.id,r.sale_id,s.receipt_number,r.refund_type,r.total_minor,r.reason,
                t.method,t.reference,r.created_at,u.full_name,r.station_key,r.station_name_snapshot
         FROM refunds r
         JOIN sales s ON s.id=r.sale_id
         JOIN users u ON u.id=r.cashier_id
         JOIN refund_tenders t ON t.refund_id=r.id
         WHERE r.id=?1",
            [id],
            |row| {
                Ok(RefundReceipt {
                    id: row.get(0)?,
                    sale_id: row.get(1)?,
                    receipt_number: row.get(2)?,
                    kind: row.get(3)?,
                    total_minor: row.get(4)?,
                    reason: row.get(5)?,
                    payment_method: row.get(6)?,
                    payment_reference: row.get(7)?,
                    created_at: row.get(8)?,
                    recorded_by: row.get(9)?,
                    station_key: row.get(10)?,
                    station_name_snapshot: row.get(11)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_sale_refund_detail(
    sale_id: String,
    state: State<'_, AppState>,
) -> Result<SaleRefundDetail, String> {
    let session = active_session(&state, "sales:read")?;
    let connection = state.db.lock().map_err(|_| "Database unavailable.")?;
    let (receipt_number, created_at, cashier_name, status, total_minor) = connection
        .query_row(
            "SELECT s.receipt_number,s.created_at,u.full_name,s.status,s.total_minor
             FROM sales s JOIN users u ON u.id=s.cashier_id
             WHERE s.id=?1 AND s.store_id=?2",
            params![sale_id, session.store_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT si.id,si.product_id,si.sku_snapshot,si.name_snapshot,si.quantity,
                    si.line_total_minor,si.line_discount_minor,si.line_tax_minor,
                    COALESCE((SELECT SUM(ri.quantity) FROM refund_items ri
                              JOIN refunds r ON r.id=ri.refund_id
                              WHERE ri.sale_item_id=si.id AND r.sale_id=?1),0)
             FROM sale_items si WHERE si.sale_id=?1 ORDER BY si.rowid",
        )
        .map_err(|error| error.to_string())?;
    let items = statement
        .query_map([&sale_id], |row| {
            let sold: i64 = row.get(4)?;
            let refunded: i64 = row.get(8)?;
            let value = row.get::<_, i64>(5)? - row.get::<_, i64>(6)? + row.get::<_, i64>(7)?;
            Ok(RefundableSaleItem {
                sale_item_id: row.get(0)?,
                product_id: row.get(1)?,
                sku: row.get(2)?,
                name: row.get(3)?,
                sold_quantity: sold,
                refunded_quantity: refunded,
                remaining_quantity: (sold - refunded).max(0),
                line_subtotal_minor: row.get(5)?,
                line_discount_minor: row.get(6)?,
                line_tax_minor: row.get(7)?,
                refundable_total_minor: value,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut refund_statement = connection
        .prepare(
            "SELECT r.id,r.sale_id,s.receipt_number,r.refund_type,r.total_minor,r.reason,
                    t.method,t.reference,r.created_at,u.full_name,r.station_key,r.station_name_snapshot
             FROM refunds r JOIN sales s ON s.id=r.sale_id JOIN users u ON u.id=r.cashier_id
             JOIN refund_tenders t ON t.refund_id=r.id
             WHERE r.store_id=?1 AND r.sale_id=?2 ORDER BY r.created_at DESC,r.id DESC",
        )
        .map_err(|error| error.to_string())?;
    let refunds = refund_statement
        .query_map(params![session.store_id, sale_id], |row| {
            Ok(RefundReceipt {
                id: row.get(0)?,
                sale_id: row.get(1)?,
                receipt_number: row.get(2)?,
                kind: row.get(3)?,
                total_minor: row.get(4)?,
                reason: row.get(5)?,
                payment_method: row.get(6)?,
                payment_reference: row.get(7)?,
                created_at: row.get(8)?,
                recorded_by: row.get(9)?,
                station_key: row.get(10)?,
                station_name_snapshot: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(SaleRefundDetail {
        sale_id,
        receipt_number,
        created_at,
        cashier_name,
        status,
        total_minor,
        items,
        refunds,
    })
}

#[tauri::command]
pub fn refund_sale(
    input: RefundInput,
    state: State<'_, AppState>,
) -> Result<RefundReceipt, String> {
    let session = active_session(&state, "sales:refund")?;
    let key = input.idempotency_key.trim();
    if key.len() < 16
        || key.len() > 80
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("The refund request ID is invalid. Reload the sale and try again.".to_owned());
    }
    let reason = input.reason.trim();
    if reason.is_empty() || reason.chars().count() > 250 {
        return Err("Enter a refund reason of up to 250 characters.".to_owned());
    }
    if input.items.is_empty() || input.items.len() > 500 {
        return Err("Choose at least one item to return.".to_owned());
    }
    if !matches!(input.kind.as_str(), "refund" | "void") {
        return Err("Choose a refund or void action.".to_owned());
    }
    let method = payment_method(input.payment_method.as_str())?;
    let reference = input.payment_reference.trim();
    if reference.chars().count() > 120 {
        return Err("The refund reference is too long.".to_owned());
    }
    if method != "cash" && reference.is_empty() {
        return Err("Enter the confirmed external refund reference before saving.".to_owned());
    }
    let mut seen = HashSet::new();
    for item in &input.items {
        if item.sale_item_id.trim().is_empty()
            || item.quantity <= 0
            || !seen.insert(item.sale_item_id.as_str())
        {
            return Err("Each returned item needs a unique item and positive quantity.".to_owned());
        }
    }

    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(existing) = refund_by_key(&tx, &session.store_id, key)? {
        if existing.sale_id != input.sale_id {
            return Err("That refund request ID has already been used.".to_owned());
        }
        tx.commit().map_err(|error| error.to_string())?;
        return Ok(existing);
    }
    let sale: Option<(String, String, i64)> = tx
        .query_row(
            "SELECT receipt_number,status,total_minor FROM sales WHERE id=?1 AND store_id=?2",
            params![input.sale_id, session.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (receipt_number, status, sale_total) =
        sale.ok_or_else(|| "That sale is not available in this store.".to_owned())?;
    if !matches!(status.as_str(), "completed" | "partially_refunded") {
        return Err("Only completed sales can be refunded or voided.".to_owned());
    }
    if input.kind == "void" && status != "completed" {
        return Err(
            "A partially refunded sale cannot be voided. Refund the remaining items instead."
                .to_owned(),
        );
    }

    let mut refund_lines = Vec::with_capacity(input.items.len());
    let mut refund_total = 0_i64;
    for item in &input.items {
        let line: Option<(String, i64, i64, i64, i64)> = tx
            .query_row(
                "SELECT si.product_id,si.quantity,si.line_total_minor,si.line_discount_minor,
                        si.line_tax_minor
                 FROM sale_items si JOIN sales s ON s.id=si.sale_id
                 WHERE si.id=?1 AND si.sale_id=?2 AND s.store_id=?3",
                params![item.sale_item_id, input.sale_id, session.store_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let (product_id, sold, line_total, discount, tax) =
            line.ok_or_else(|| "A selected item does not belong to this sale.".to_owned())?;
        let already_refunded: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(ri.quantity),0) FROM refund_items ri
                 JOIN refunds r ON r.id=ri.refund_id WHERE ri.sale_item_id=?1 AND r.sale_id=?2",
                params![item.sale_item_id, input.sale_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let components = components_for_returned_quantity(
            line_total,
            discount,
            tax,
            sold,
            already_refunded,
            item.quantity,
        )?;
        refund_total = refund_total
            .checked_add(components.3)
            .ok_or_else(|| "The refund amount is too large.".to_owned())?;
        refund_lines.push((item, product_id, sold, already_refunded, components));
    }
    if refund_total <= 0 || refund_total > sale_total {
        return Err("The selected items do not produce a valid positive refund amount.".to_owned());
    }
    if input.kind == "void" {
        let item_count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sale_items WHERE sale_id=?1",
                [&input.sale_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let returns_every_line = refund_lines.len() as i64 == item_count
            && refund_lines
                .iter()
                .all(|(line, _, sold, already, _)| *already == 0 && line.quantity == *sold);
        if !returns_every_line || refund_total != sale_total {
            return Err(
                "A sale can only be voided when every item is returned in full.".to_owned(),
            );
        }
    }

    let station_key = crate::stations::station_key_for_device(&tx, &state.device_id, &session)?;
    let station_name = crate::stations::station_name_for_key(&tx, &session.store_id, &station_key)?;
    let cash_shift_id: Option<String> = if method == "cash" {
        tx.query_row(
            "SELECT id FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open'",
            params![session.store_id, station_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
    } else {
        None
    };
    if method == "cash" && cash_shift_id.is_none() {
        return Err("Open the cash drawer before recording a cash refund.".to_owned());
    }

    let refund_id = Uuid::new_v4().to_string();
    let timestamp = now();
    tx.execute(
        "INSERT INTO refunds(id,store_id,sale_id,cashier_id,idempotency_key,refund_type,total_minor,reason,created_at,station_key,station_name_snapshot,cash_shift_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![refund_id, session.store_id, input.sale_id, session.user_id, key, input.kind, refund_total, reason, timestamp, station_key, station_name, cash_shift_id],
    )
    .map_err(|error| error.to_string())?;

    for (item, product_id, _sold, _already, components) in &refund_lines {
        tx.execute(
            "INSERT INTO refund_items(id,refund_id,sale_item_id,quantity,subtotal_minor,discount_minor,tax_minor,amount_minor) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Uuid::new_v4().to_string(), refund_id, item.sale_item_id, item.quantity, components.0, components.1, components.2, components.3],
        )
        .map_err(|error| error.to_string())?;
        let current_stock: i64 = tx
            .query_row(
                "SELECT stock_quantity FROM products WHERE id=?1 AND store_id=?2",
                params![product_id, session.store_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let restored = current_stock
            .checked_add(item.quantity)
            .ok_or_else(|| "Returned stock exceeded the supported quantity.".to_owned())?;
        tx.execute(
            "UPDATE products SET stock_quantity=?1,updated_at=?2 WHERE id=?3 AND store_id=?4",
            params![restored, timestamp, product_id, session.store_id],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO stock_movements(id,store_id,product_id,user_id,quantity_delta,reason,reference_id,note,created_at)
             VALUES(?1,?2,?3,?4,?5,'refund',?6,?7,?8)",
            params![Uuid::new_v4().to_string(), session.store_id, product_id, session.user_id, item.quantity, refund_id, reason, timestamp],
        )
        .map_err(|error| error.to_string())?;
    }

    tx.execute(
        "INSERT INTO refund_tenders(id,refund_id,method,amount_minor,reference,created_at)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            Uuid::new_v4().to_string(),
            refund_id,
            method,
            refund_total,
            if reference.is_empty() {
                None
            } else {
                Some(reference)
            },
            timestamp
        ],
    )
    .map_err(|error| error.to_string())?;
    if let Some(shift_id) = cash_shift_id.as_deref() {
        crate::cash_locations::record_cash_refund(
            &tx,
            &session,
            &refund_id,
            Some(shift_id),
            refund_total,
            &timestamp,
        )?;
    }
    if let Some(shift_id) = cash_shift_id {
        tx.execute(
            "INSERT INTO cash_movements(id,shift_id,store_id,user_id,movement_type,amount_minor,note,created_at)
             VALUES(?1,?2,?3,?4,'paid_out',?5,?6,?7)",
            params![Uuid::new_v4().to_string(), shift_id, session.store_id, session.user_id, refund_total, format!("Customer refund {receipt_number}"), timestamp],
        )
        .map_err(|error| error.to_string())?;
    }

    let total_refunded: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(total_minor),0) FROM refunds WHERE sale_id=?1 AND store_id=?2",
            params![input.sale_id, session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if total_refunded > sale_total {
        return Err("Refunds cannot exceed the original sale total.".to_owned());
    }
    let new_status = if input.kind == "void" {
        "voided"
    } else if total_refunded == sale_total {
        "refunded"
    } else {
        "partially_refunded"
    };
    tx.execute(
        "UPDATE sales SET status=?1 WHERE id=?2 AND store_id=?3",
        params![new_status, input.sale_id, session.store_id],
    )
    .map_err(|error| error.to_string())?;
    add_audit_event(
        &tx,
        &session,
        if input.kind == "void" {
            "sale_voided"
        } else {
            "sale_refunded"
        },
        "refund",
        Some(&refund_id),
        serde_json::json!({
            "saleId":input.sale_id,
            "receiptNumber":receipt_number,
            "kind":input.kind,
            "totalMinor":refund_total,
            "paymentMethod":method,
            "reason":reason
        }),
    )?;
    crate::accounting::post_refund_for_session(&tx, &session, &refund_id)?;
    tx.commit().map_err(|error| error.to_string())?;
    receipt_for_id(&db, &refund_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_item_returns_reconcile_exactly_to_saved_line_components() {
        let mut refunded = [0_i64; 4];
        for index in 0..3 {
            let components = components_for_returned_quantity(300, 1, 15, 3, index, 1).unwrap();
            refunded[0] += components.0;
            refunded[1] += components.1;
            refunded[2] += components.2;
            refunded[3] += components.3;
        }
        assert_eq!(refunded, [300, 1, 15, 314]);
    }

    #[test]
    fn return_quantity_cannot_exceed_unreturned_units() {
        assert!(components_for_returned_quantity(100, 0, 0, 2, 2, 1).is_err());
        assert!(components_for_returned_quantity(100, 0, 0, 2, 0, 0).is_err());
    }
}
