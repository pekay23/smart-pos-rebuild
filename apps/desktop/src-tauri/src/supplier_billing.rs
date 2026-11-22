// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::{NaiveDate, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    accounting,
    commands::{active_session, add_audit_event},
    db::AppState,
};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BillAllocationInput {
    pub purchase_id: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSupplierBillInput {
    pub supplier_id: String,
    pub supplier_reference: String,
    pub invoice_date: String,
    pub due_date: Option<String>,
    pub total_minor: i64,
    pub idempotency_key: String,
    pub allocations: Vec<BillAllocationInput>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentAllocationInput {
    pub bill_id: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSupplierPaymentInput {
    pub supplier_id: String,
    pub payment_method: String,
    pub cash_origin: String,
    pub cash_source_note: Option<String>,
    pub cash_location_id: Option<String>,
    pub external_reference: Option<String>,
    pub amount_minor: i64,
    pub idempotency_key: String,
    pub allocations: Vec<PaymentAllocationInput>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupplierReturnLineInput {
    pub purchase_id: String,
    pub purchase_item_id: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSupplierCreditNoteInput {
    pub bill_id: String,
    pub credit_reference: String,
    pub effective_date: String,
    pub reason: String,
    pub idempotency_key: String,
    pub returns: Vec<SupplierReturnLineInput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnbilledReceipt {
    pub purchase_id: String,
    pub supplier_id: String,
    pub reference: Option<String>,
    pub received_at: String,
    pub total_minor: i64,
    pub unbilled_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSupplierBill {
    pub bill_id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub supplier_reference: String,
    pub invoice_date: String,
    pub due_date: Option<String>,
    pub currency_code: String,
    pub total_minor: i64,
    pub paid_minor: i64,
    pub credited_minor: i64,
    pub outstanding_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierReturnCandidate {
    pub bill_id: String,
    pub purchase_id: String,
    pub purchase_reference: Option<String>,
    pub purchase_item_id: String,
    pub product_id: String,
    pub product_name: String,
    pub unit_cost_minor: i64,
    pub max_quantity: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierBillingWorkspace {
    pub unbilled_receipts: Vec<UnbilledReceipt>,
    pub open_bills: Vec<OpenSupplierBill>,
    pub returnable_items: Vec<SupplierReturnCandidate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierBillingRecord {
    pub id: String,
    pub amount_minor: i64,
    pub currency_code: String,
    pub replayed: bool,
}

fn date(value: &str, label: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| format!("Enter {label} as a valid date (YYYY-MM-DD)."))
}

fn key(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 100 {
        return Err("A retry key between 1 and 100 characters is required.".to_owned());
    }
    Ok(value.to_owned())
}

fn checked_allocations<T>(
    allocations: &[T],
    get_id: impl Fn(&T) -> String,
    get_amount: impl Fn(&T) -> i64,
    label: &str,
) -> Result<i64, String> {
    if allocations.is_empty() || allocations.len() > 50 {
        return Err(format!("Select between 1 and 50 {label}."));
    }
    let mut ids = std::collections::HashSet::new();
    allocations.iter().try_fold(0_i64, |sum, allocation| {
        if !ids.insert(get_id(allocation)) {
            return Err(format!("Each {label} can be selected only once."));
        }
        let amount = get_amount(allocation);
        if amount <= 0 {
            return Err("Each allocation must be a positive amount.".to_owned());
        }
        sum.checked_add(amount)
            .ok_or_else(|| "The allocation total is too large.".to_owned())
    })
}

fn checked_return_credit_amount(
    quantity: i64,
    unreturned_quantity: i64,
    current_stock: i64,
    remaining_delivery_allocation: i64,
    receipt_unit_cost: i64,
) -> Result<i64, String> {
    if quantity <= 0 || receipt_unit_cost <= 0 {
        return Err("Return quantity and recorded receipt cost must be positive.".to_owned());
    }
    if quantity > unreturned_quantity || quantity > current_stock {
        return Err(
            "The returned quantity exceeds unreturned received stock or current stock.".to_owned(),
        );
    }
    let amount = receipt_unit_cost
        .checked_mul(quantity)
        .ok_or_else(|| "The supplier credit amount is too large.".to_owned())?;
    if remaining_delivery_allocation < amount {
        return Err("The credit exceeds this delivery's remaining invoice allocation.".to_owned());
    }
    Ok(amount)
}

fn replay_bill(
    connection: &rusqlite::Connection,
    session: &crate::models::SessionUser,
    input: &MatchSupplierBillInput,
    reference: &str,
    key: &str,
    total: i64,
) -> Result<Option<SupplierBillingRecord>, String> {
    let row: Option<(String, String, String, String, Option<String>, i64, String)> = connection
        .query_row(
            "SELECT id,supplier_id,supplier_reference,invoice_date,due_date,total_minor,currency_code FROM supplier_bills WHERE store_id=?1 AND idempotency_key=?2",
            params![session.store_id, key],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((
        id,
        supplier_id,
        supplier_reference,
        invoice_date,
        due_date,
        total_minor,
        currency_code,
    )) = row
    else {
        return Ok(None);
    };
    let mut actual = Vec::<(String, i64)>::new();
    let mut statement = connection.prepare("SELECT purchase_id,amount_minor FROM supplier_bill_allocations WHERE bill_id=?1 ORDER BY purchase_id")
        .map_err(|error| error.to_string())?;
    for item in statement
        .query_map([&id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| error.to_string())?
    {
        actual.push(item.map_err(|error| error.to_string())?);
    }
    let mut expected: Vec<_> = input
        .allocations
        .iter()
        .map(|allocation| (allocation.purchase_id.clone(), allocation.amount_minor))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    if supplier_id != input.supplier_id
        || supplier_reference != reference
        || invoice_date != input.invoice_date
        || due_date != input.due_date
        || total_minor != total
        || actual != expected
    {
        return Err("This retry key was already used for a different supplier invoice.".to_owned());
    }
    Ok(Some(SupplierBillingRecord {
        id,
        amount_minor: total_minor,
        currency_code,
        replayed: true,
    }))
}

#[tauri::command]
pub fn get_supplier_billing_workspace(
    state: State<'_, AppState>,
) -> Result<SupplierBillingWorkspace, String> {
    let session = active_session(&state, "")?;
    if !session.permissions.iter().any(|permission| {
        matches!(
            permission.as_str(),
            "purchases:read" | "purchases:invoice" | "suppliers:settle"
        )
    }) {
        return Err("Your account does not have permission to view supplier balances.".to_owned());
    }
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut unbilled_receipts = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT p.id,p.supplier_id,p.reference,p.received_at,p.total_minor,
                p.total_minor-COALESCE((SELECT SUM(a.amount_minor) FROM supplier_bill_allocations a WHERE a.store_id=p.store_id AND a.purchase_id=p.id),0)
             FROM purchases p JOIN suppliers s ON s.id=p.supplier_id AND s.store_id=p.store_id
             JOIN accounting_journals j ON j.source_type='purchase' AND j.source_id=p.id AND j.store_id=p.store_id
             WHERE p.store_id=?1 AND p.supplier_id IS NOT NULL AND s.is_active=1
             AND p.total_minor>COALESCE((SELECT SUM(a.amount_minor) FROM supplier_bill_allocations a WHERE a.store_id=p.store_id AND a.purchase_id=p.id),0)
             ORDER BY p.received_at DESC,p.id DESC")
            .map_err(|error| error.to_string())?;
        for row in statement
            .query_map([&session.store_id], |row| {
                Ok(UnbilledReceipt {
                    purchase_id: row.get(0)?,
                    supplier_id: row.get(1)?,
                    reference: row.get(2)?,
                    received_at: row.get(3)?,
                    total_minor: row.get(4)?,
                    unbilled_minor: row.get(5)?,
                })
            })
            .map_err(|error| error.to_string())?
        {
            unbilled_receipts.push(row.map_err(|error| error.to_string())?);
        }
    }
    let mut open_bills = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT b.id,b.supplier_id,s.name,b.supplier_reference,b.invoice_date,b.due_date,b.currency_code,b.total_minor,
                COALESCE((SELECT SUM(a.amount_minor) FROM supplier_payment_allocations a WHERE a.bill_id=b.id AND a.store_id=b.store_id),0),
                COALESCE((SELECT SUM(c.total_minor) FROM supplier_bill_credit_notes c WHERE c.bill_id=b.id AND c.store_id=b.store_id),0),
                b.total_minor
                 -COALESCE((SELECT SUM(a.amount_minor) FROM supplier_payment_allocations a WHERE a.bill_id=b.id AND a.store_id=b.store_id),0)
                 -COALESCE((SELECT SUM(c.total_minor) FROM supplier_bill_credit_notes c WHERE c.bill_id=b.id AND c.store_id=b.store_id),0)
             FROM supplier_bills b JOIN suppliers s ON s.id=b.supplier_id AND s.store_id=b.store_id
             WHERE b.store_id=?1
               AND b.total_minor
                 -COALESCE((SELECT SUM(a.amount_minor) FROM supplier_payment_allocations a WHERE a.bill_id=b.id AND a.store_id=b.store_id),0)
                 -COALESCE((SELECT SUM(c.total_minor) FROM supplier_bill_credit_notes c WHERE c.bill_id=b.id AND c.store_id=b.store_id),0)>0
             ORDER BY COALESCE(b.due_date,'9999-12-31'),b.invoice_date,b.id")
            .map_err(|error| error.to_string())?;
        for row in statement
            .query_map([&session.store_id], |row| {
                Ok(OpenSupplierBill {
                    bill_id: row.get(0)?,
                    supplier_id: row.get(1)?,
                    supplier_name: row.get(2)?,
                    supplier_reference: row.get(3)?,
                    invoice_date: row.get(4)?,
                    due_date: row.get(5)?,
                    currency_code: row.get(6)?,
                    total_minor: row.get(7)?,
                    paid_minor: row.get(8)?,
                    credited_minor: row.get(9)?,
                    outstanding_minor: row.get(10)?,
                })
            })
            .map_err(|error| error.to_string())?
        {
            open_bills.push(row.map_err(|error| error.to_string())?);
        }
    }
    let mut returnable_items = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT b.id,p.id,p.reference,pi.id,pr.id,pr.name,pi.unit_cost_minor,
                MIN(
                    pi.quantity-COALESCE((SELECT SUM(ci.quantity) FROM supplier_bill_credit_items ci WHERE ci.purchase_item_id=pi.id AND ci.store_id=b.store_id),0),
                    pr.stock_quantity,
                    CAST((a.amount_minor-COALESCE((SELECT SUM(ci.amount_minor)
                        FROM supplier_bill_credit_notes cn JOIN supplier_bill_credit_items ci
                          ON ci.credit_note_id=cn.id AND ci.store_id=cn.store_id
                        WHERE cn.bill_id=b.id AND ci.purchase_id=p.id AND cn.store_id=b.store_id),0))/pi.unit_cost_minor AS INTEGER)
                )
             FROM supplier_bills b
             JOIN supplier_bill_allocations a ON a.bill_id=b.id AND a.store_id=b.store_id
             JOIN purchases p ON p.id=a.purchase_id AND p.store_id=a.store_id
             JOIN purchase_items pi ON pi.purchase_id=p.id
             JOIN products pr ON pr.id=pi.product_id AND pr.store_id=p.store_id
             WHERE b.store_id=?1 AND pi.unit_cost_minor>0 AND pr.stock_quantity>0
               AND CAST((a.amount_minor-COALESCE((SELECT SUM(ci.amount_minor)
                    FROM supplier_bill_credit_notes cn JOIN supplier_bill_credit_items ci
                      ON ci.credit_note_id=cn.id AND ci.store_id=cn.store_id
                    WHERE cn.bill_id=b.id AND ci.purchase_id=p.id AND cn.store_id=b.store_id),0))/pi.unit_cost_minor AS INTEGER)>0
               AND pi.quantity>COALESCE((SELECT SUM(ci.quantity) FROM supplier_bill_credit_items ci WHERE ci.purchase_item_id=pi.id AND ci.store_id=b.store_id),0)
               AND a.amount_minor>COALESCE((SELECT SUM(ci.amount_minor)
                    FROM supplier_bill_credit_notes cn JOIN supplier_bill_credit_items ci
                      ON ci.credit_note_id=cn.id AND ci.store_id=cn.store_id
                    WHERE cn.bill_id=b.id AND ci.purchase_id=p.id AND cn.store_id=b.store_id),0)
               AND b.total_minor
                 -COALESCE((SELECT SUM(pa.amount_minor) FROM supplier_payment_allocations pa WHERE pa.bill_id=b.id AND pa.store_id=b.store_id),0)
                 -COALESCE((SELECT SUM(c.total_minor) FROM supplier_bill_credit_notes c WHERE c.bill_id=b.id AND c.store_id=b.store_id),0)>0
             ORDER BY b.invoice_date,b.supplier_reference,p.received_at,pr.name")
            .map_err(|error| error.to_string())?;
        for row in statement
            .query_map([&session.store_id], |row| {
                Ok(SupplierReturnCandidate {
                    bill_id: row.get(0)?,
                    purchase_id: row.get(1)?,
                    purchase_reference: row.get(2)?,
                    purchase_item_id: row.get(3)?,
                    product_id: row.get(4)?,
                    product_name: row.get(5)?,
                    unit_cost_minor: row.get(6)?,
                    max_quantity: row.get(7)?,
                })
            })
            .map_err(|error| error.to_string())?
        {
            returnable_items.push(row.map_err(|error| error.to_string())?);
        }
    }
    Ok(SupplierBillingWorkspace {
        unbilled_receipts,
        open_bills,
        returnable_items,
    })
}

#[tauri::command]
pub fn match_supplier_bill(
    state: State<'_, AppState>,
    input: MatchSupplierBillInput,
) -> Result<SupplierBillingRecord, String> {
    let session = active_session(&state, "purchases:invoice")?;
    let reference = input.supplier_reference.trim();
    if reference.is_empty() || reference.len() > 100 {
        return Err("Enter a supplier invoice reference up to 100 characters.".to_owned());
    }
    date(&input.invoice_date, "invoice date")?;
    if let Some(due_date) = &input.due_date {
        date(due_date, "due date")?;
        if due_date < &input.invoice_date {
            return Err("The due date cannot be earlier than the invoice date.".to_owned());
        }
    }
    let retry_key = key(&input.idempotency_key)?;
    let allocated_total = checked_allocations(
        &input.allocations,
        |a| a.purchase_id.clone(),
        |a| a.amount_minor,
        "received deliveries",
    )?;
    if input.total_minor <= 0 || allocated_total != input.total_minor {
        return Err(
            "The supplier invoice amount must exactly match its delivery allocations.".to_owned(),
        );
    }
    let total = input.total_minor;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(record) = replay_bill(&tx, &session.user, &input, reference, &retry_key, total)? {
        return Ok(record);
    }
    let supplier: Option<String> = tx
        .query_row(
            "SELECT id FROM suppliers WHERE id=?1 AND store_id=?2 AND is_active=1",
            params![input.supplier_id, session.store_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if supplier.is_none() {
        return Err("Choose an active supplier in this store.".to_owned());
    }
    let reference_key = reference.to_lowercase();
    let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM supplier_bills WHERE business_id=?1 AND supplier_id=?2 AND reference_key=?3)", params![session.business_id,input.supplier_id,reference_key], |row| row.get(0)).map_err(|error| error.to_string())?;
    if duplicate {
        return Err("That supplier invoice reference is already recorded.".to_owned());
    }
    for allocation in &input.allocations {
        let receipt: Option<(String,i64)> = tx.query_row(
            "SELECT p.supplier_id,p.total_minor-COALESCE((SELECT SUM(a.amount_minor) FROM supplier_bill_allocations a WHERE a.store_id=p.store_id AND a.purchase_id=p.id),0)
             FROM purchases p JOIN accounting_journals j ON j.source_type='purchase' AND j.source_id=p.id AND j.store_id=p.store_id
             WHERE p.id=?1 AND p.store_id=?2",
            params![allocation.purchase_id,session.store_id], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(|error| error.to_string())?;
        let Some((receipt_supplier, available)) = receipt else {
            return Err(
                "A selected delivery is missing or has not been posted to accounting.".to_owned(),
            );
        };
        if receipt_supplier != input.supplier_id {
            return Err(
                "Invoice allocations must use deliveries from the selected supplier.".to_owned(),
            );
        }
        if allocation.amount_minor > available {
            return Err(
                "An invoice allocation exceeds the unbilled value of its delivery.".to_owned(),
            );
        }
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute("INSERT INTO supplier_bills(id,business_id,store_id,supplier_id,supplier_reference,reference_key,invoice_date,due_date,currency_code,total_minor,idempotency_key,created_by,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)", params![id,session.business_id,session.store_id,input.supplier_id,reference,reference_key,input.invoice_date,input.due_date,session.currency_code,total,retry_key,session.user_id,now]).map_err(|error| format!("Could not save supplier invoice: {error}"))?;
    for allocation in &input.allocations {
        tx.execute("INSERT INTO supplier_bill_allocations(id,business_id,store_id,bill_id,purchase_id,amount_minor,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![Uuid::new_v4().to_string(),session.business_id,session.store_id,id,allocation.purchase_id,allocation.amount_minor,now]).map_err(|error| error.to_string())?;
    }
    accounting::post_supplier_bill_for_session(&tx, &session.user, &id)?;
    add_audit_event(
        &tx,
        &session.user,
        "supplier_bill_matched",
        "supplier_bill",
        Some(&id),
        serde_json::json!({"supplierId":input.supplier_id,"reference":reference,"totalMinor":total,"allocationCount":input.allocations.len()}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(SupplierBillingRecord {
        id,
        amount_minor: total,
        currency_code: session.currency_code.clone(),
        replayed: false,
    })
}

fn replay_payment(
    connection: &rusqlite::Connection,
    session: &crate::models::SessionUser,
    input: &RecordSupplierPaymentInput,
    method: &str,
    reference: Option<&str>,
    retry_key: &str,
    amount: i64,
    cash_location_id: Option<&str>,
) -> Result<Option<SupplierBillingRecord>, String> {
    let row: Option<(String,String,String,Option<String>,i64,String,String,Option<String>,Option<String>)> = connection.query_row(
        "SELECT id,supplier_id,payment_method,external_reference,amount_minor,currency_code,cash_origin,cash_source_note,cash_location_id FROM supplier_payments WHERE store_id=?1 AND idempotency_key=?2",
        params![session.store_id,retry_key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?)),
    ).optional().map_err(|error| error.to_string())?;
    let Some((
        id,
        supplier_id,
        payment_method,
        external_reference,
        amount_minor,
        currency_code,
        cash_origin,
        cash_source_note,
        saved_location_id,
    )) = row
    else {
        return Ok(None);
    };
    let mut actual = Vec::<(String, i64)>::new();
    let mut statement = connection.prepare("SELECT bill_id,amount_minor FROM supplier_payment_allocations WHERE payment_id=?1 ORDER BY bill_id").map_err(|error| error.to_string())?;
    for row in statement
        .query_map([&id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| error.to_string())?
    {
        actual.push(row.map_err(|error| error.to_string())?);
    }
    let mut expected: Vec<_> = input
        .allocations
        .iter()
        .map(|a| (a.bill_id.clone(), a.amount_minor))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    if supplier_id != input.supplier_id
        || payment_method != method
        || external_reference.as_deref() != reference
        || amount_minor != amount
        || cash_origin != input.cash_origin
        || cash_source_note.as_deref()
            != input
                .cash_source_note
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        || saved_location_id.as_deref() != cash_location_id
        || actual != expected
    {
        return Err("This retry key was already used for a different supplier payment.".to_owned());
    }
    Ok(Some(SupplierBillingRecord {
        id,
        amount_minor,
        currency_code,
        replayed: true,
    }))
}

#[tauri::command]
pub fn record_supplier_payment(
    state: State<'_, AppState>,
    input: RecordSupplierPaymentInput,
) -> Result<SupplierBillingRecord, String> {
    let session = active_session(&state, "suppliers:settle")?;
    let method = input.payment_method.trim();
    if !["cash", "card", "mobile_money", "bank_transfer", "other"].contains(&method) {
        return Err("Choose a supported payment method.".to_owned());
    }
    let reference = input
        .external_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if reference.is_some_and(|value| value.len() > 100) {
        return Err("Payment reference must be 100 characters or fewer.".to_owned());
    }
    let cash_source_note = input
        .cash_source_note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if cash_source_note.is_some_and(|value| value.chars().count() > 100) {
        return Err("Keep the cash source name under 100 characters.".to_owned());
    }
    match (method, input.cash_origin.as_str()) {
        ("cash", "drawer") if cash_source_note.is_none() => {}
        ("cash", "other_cash") => {}
        ("cash", _) => {
            return Err("Choose the register drawer or a tracked cash location.".to_owned())
        }
        (_, "not_applicable") if cash_source_note.is_none() => {}
        _ => return Err("Cash origin must match the selected payment method.".to_owned()),
    }
    let retry_key = key(&input.idempotency_key)?;
    let allocated_amount = checked_allocations(
        &input.allocations,
        |a| a.bill_id.clone(),
        |a| a.amount_minor,
        "open supplier bills",
    )?;
    if input.amount_minor <= 0 || allocated_amount != input.amount_minor {
        return Err("The payment amount must exactly match its invoice allocations.".to_owned());
    }
    let amount = input.amount_minor;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let cash_location_id = if method == "cash" {
        crate::cash_locations::resolve_source_location(
            &tx,
            &state.device_id,
            &session.user,
            &input.cash_origin,
            input.cash_location_id.as_deref(),
        )?
    } else {
        None
    };
    if let Some(record) = replay_payment(
        &tx,
        &session.user,
        &input,
        method,
        reference,
        &retry_key,
        amount,
        cash_location_id.as_deref(),
    )? {
        return Ok(record);
    }
    let supplier: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM suppliers WHERE id=?1 AND store_id=?2 AND is_active=1)",
            params![input.supplier_id, session.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !supplier {
        return Err("Choose an active supplier in this store.".to_owned());
    }
    for allocation in &input.allocations {
        let bill: Option<(String,String,i64,i64)> = tx.query_row(
            "SELECT b.supplier_id,b.currency_code,
                b.total_minor
                  -COALESCE((SELECT SUM(a.amount_minor) FROM supplier_payment_allocations a WHERE a.store_id=b.store_id AND a.bill_id=b.id),0)
                  -COALESCE((SELECT SUM(c.total_minor) FROM supplier_bill_credit_notes c WHERE c.store_id=b.store_id AND c.bill_id=b.id),0),
                b.total_minor
             FROM supplier_bills b WHERE b.id=?1 AND b.store_id=?2",
            params![allocation.bill_id,session.store_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(|error|error.to_string())?;
        let Some((bill_supplier, currency, available, _total)) = bill else {
            return Err(
                "A selected supplier invoice is missing or belongs to another store.".to_owned(),
            );
        };
        if bill_supplier != input.supplier_id || currency != session.currency_code {
            return Err("Payments must match the selected supplier and store currency.".to_owned());
        }
        if allocation.amount_minor > available {
            return Err(
                "A payment allocation exceeds the invoice's outstanding balance.".to_owned(),
            );
        }
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let reference_key = reference.map(str::to_lowercase);
    tx.execute("INSERT INTO supplier_payments(id,business_id,store_id,supplier_id,currency_code,payment_method,external_reference,reference_key,amount_minor,idempotency_key,recorded_by,paid_at,created_at,cash_origin,cash_source_note,cash_location_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",params![id,session.business_id,session.store_id,input.supplier_id,session.currency_code,method,reference,reference_key,amount,retry_key,session.user_id,now,now,input.cash_origin,cash_source_note,cash_location_id]).map_err(|error| format!("Could not record supplier payment: {error}"))?;
    for allocation in &input.allocations {
        tx.execute("INSERT INTO supplier_payment_allocations(id,business_id,store_id,payment_id,bill_id,amount_minor,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![Uuid::new_v4().to_string(),session.business_id,session.store_id,id,allocation.bill_id,allocation.amount_minor,now]).map_err(|error|error.to_string())?;
    }
    if input.cash_origin == "drawer" {
        crate::cash_drawer::record_source_paid_out(
            &tx,
            &state.device_id,
            &session.user,
            "supplier_payment",
            &id,
            amount,
            &format!("Supplier payment · {}", input.supplier_id),
            &now,
        )?;
    }
    if method == "cash" {
        let shift_id = if input.cash_origin == "drawer" {
            let station_key =
                crate::stations::station_key_for_device(&tx, &state.device_id, &session.user)?;
            tx.query_row(
                "SELECT id FROM cash_shifts WHERE store_id=?1 AND station_key=?2 AND status='open'",
                params![session.store_id, station_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
        } else {
            None
        };
        crate::cash_locations::record_source_cash_outflow(
            &tx,
            &session.user,
            "supplier_payment",
            &id,
            shift_id.as_deref(),
            amount,
            &format!("Supplier payment · {}", input.supplier_id),
            &now,
        )?;
    }
    accounting::post_supplier_payment_for_session(&tx, &session.user, &id)?;
    add_audit_event(
        &tx,
        &session.user,
        "supplier_payment_recorded",
        "supplier_payment",
        Some(&id),
        serde_json::json!({"supplierId":input.supplier_id,"method":method,"amountMinor":amount,"allocationCount":input.allocations.len(),"cashOrigin":input.cash_origin,"cashSourceNote":cash_source_note,"idempotencyKey":retry_key}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(SupplierBillingRecord {
        id,
        amount_minor: amount,
        currency_code: session.currency_code.clone(),
        replayed: false,
    })
}

fn replay_supplier_credit_note(
    connection: &rusqlite::Connection,
    session: &crate::models::SessionUser,
    input: &CreateSupplierCreditNoteInput,
    reference: &str,
    reason: &str,
    retry_key: &str,
) -> Result<Option<SupplierBillingRecord>, String> {
    let row: Option<(String, String, String, String, String, i64, String)> = connection
        .query_row(
            "SELECT id,bill_id,credit_reference,effective_date,reason,total_minor,currency_code
             FROM supplier_bill_credit_notes WHERE store_id=?1 AND idempotency_key=?2",
            params![session.store_id, retry_key],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((
        id,
        bill_id,
        saved_reference,
        effective_date,
        saved_reason,
        amount_minor,
        currency_code,
    )) = row
    else {
        return Ok(None);
    };
    let mut actual = Vec::<(String, String, i64)>::new();
    let mut statement = connection
        .prepare("SELECT purchase_id,purchase_item_id,quantity FROM supplier_bill_credit_items WHERE credit_note_id=?1 ORDER BY purchase_item_id")
        .map_err(|error| error.to_string())?;
    for row in statement
        .query_map([&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|error| error.to_string())?
    {
        actual.push(row.map_err(|error| error.to_string())?);
    }
    let mut expected: Vec<_> = input
        .returns
        .iter()
        .map(|line| {
            (
                line.purchase_id.clone(),
                line.purchase_item_id.clone(),
                line.quantity,
            )
        })
        .collect();
    expected.sort_by(|left, right| left.1.cmp(&right.1));
    if bill_id != input.bill_id
        || saved_reference != reference
        || effective_date != input.effective_date
        || saved_reason != reason
        || actual != expected
    {
        return Err(
            "This retry key was already used for a different supplier credit note.".to_owned(),
        );
    }
    Ok(Some(SupplierBillingRecord {
        id,
        amount_minor,
        currency_code,
        replayed: true,
    }))
}

#[tauri::command]
pub fn create_supplier_credit_note(
    state: State<'_, AppState>,
    input: CreateSupplierCreditNoteInput,
) -> Result<SupplierBillingRecord, String> {
    let session = active_session(&state, "purchases:invoice")?;
    let reference = input.credit_reference.trim();
    let reason = input.reason.trim();
    if reference.is_empty() || reference.len() > 100 {
        return Err("Enter the supplier's credit note reference up to 100 characters.".to_owned());
    }
    date(&input.effective_date, "credit note date")?;
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err("Describe the goods returned in 1 to 500 characters.".to_owned());
    }
    if input.returns.is_empty() || input.returns.len() > 100 {
        return Err("Select between 1 and 100 received product lines to return.".to_owned());
    }
    let retry_key = key(&input.idempotency_key)?;
    let mut seen_items = std::collections::HashSet::new();
    for line in &input.returns {
        if line.quantity <= 0
            || line.quantity > 1_000_000
            || !seen_items.insert(line.purchase_item_id.as_str())
        {
            return Err(
                "Each returned item needs a positive whole quantity and can appear only once."
                    .to_owned(),
            );
        }
    }

    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    create_supplier_credit_note_in_transaction(
        &mut connection,
        &session.user,
        &input,
        reference,
        reason,
        &retry_key,
    )
}

fn create_supplier_credit_note_in_transaction(
    connection: &mut rusqlite::Connection,
    session: &crate::models::SessionUser,
    input: &CreateSupplierCreditNoteInput,
    reference: &str,
    reason: &str,
    retry_key: &str,
) -> Result<SupplierBillingRecord, String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(record) =
        replay_supplier_credit_note(&tx, session, input, reference, reason, retry_key)?
    {
        return Ok(record);
    }

    let bill: Option<(String, String, i64)> = tx
        .query_row(
            "SELECT b.supplier_id,b.currency_code,
                b.total_minor
                  -COALESCE((SELECT SUM(a.amount_minor) FROM supplier_payment_allocations a WHERE a.store_id=b.store_id AND a.bill_id=b.id),0)
                  -COALESCE((SELECT SUM(c.total_minor) FROM supplier_bill_credit_notes c WHERE c.store_id=b.store_id AND c.bill_id=b.id),0)
             FROM supplier_bills b WHERE b.id=?1 AND b.store_id=?2 AND b.business_id=?3",
            params![input.bill_id, session.store_id, session.business_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((supplier_id, currency_code, outstanding_minor)) = bill else {
        return Err("Choose an open supplier invoice in this store.".to_owned());
    };
    if currency_code != session.currency_code || outstanding_minor <= 0 {
        return Err(
            "This invoice has no payable balance available for a supplier credit.".to_owned(),
        );
    }
    let reference_key = reference.to_lowercase();
    let duplicate_reference: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM supplier_bill_credit_notes WHERE business_id=?1 AND supplier_id=?2 AND reference_key=?3)",
            params![session.business_id, supplier_id, reference_key],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if duplicate_reference {
        return Err("That supplier credit note reference is already recorded.".to_owned());
    }

    let mut total_minor = 0_i64;
    let mut remaining_credit_by_purchase = std::collections::HashMap::<String, i64>::new();
    let mut reserved_stock_by_product = std::collections::HashMap::<String, i64>::new();
    let mut validated = Vec::with_capacity(input.returns.len());
    for line in &input.returns {
        let source: Option<(String, i64, i64, i64, i64, i64)> = tx
            .query_row(
                "SELECT pi.product_id,pi.quantity,pi.unit_cost_minor,pr.stock_quantity,
                    COALESCE((SELECT SUM(ci.quantity) FROM supplier_bill_credit_items ci WHERE ci.purchase_item_id=pi.id AND ci.store_id=p.store_id),0),
                    a.amount_minor-COALESCE((SELECT SUM(ci.amount_minor)
                        FROM supplier_bill_credit_notes cn JOIN supplier_bill_credit_items ci
                          ON ci.credit_note_id=cn.id AND ci.store_id=cn.store_id
                        WHERE cn.bill_id=?1 AND ci.purchase_id=p.id AND cn.store_id=p.store_id),0)
                 FROM purchases p
                 JOIN purchase_items pi ON pi.purchase_id=p.id AND pi.id=?2
                 JOIN products pr ON pr.id=pi.product_id AND pr.store_id=p.store_id
                 JOIN supplier_bill_allocations a ON a.purchase_id=p.id AND a.bill_id=?1 AND a.store_id=p.store_id
                 WHERE p.id=?3 AND p.store_id=?4 AND p.supplier_id=?5",
                params![input.bill_id, line.purchase_item_id, line.purchase_id, session.store_id, supplier_id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((
            product_id,
            received_quantity,
            unit_cost_minor,
            stock_quantity,
            returned_quantity,
            receipt_credit_remaining,
        )) = source
        else {
            return Err(
                "A selected return line does not belong to this supplier invoice's received goods."
                    .to_owned(),
            );
        };
        let quantity_remaining = received_quantity
            .checked_sub(returned_quantity)
            .ok_or_else(|| "Saved supplier return quantities are inconsistent.".to_owned())?;
        let already_reserved = reserved_stock_by_product
            .get(&product_id)
            .copied()
            .unwrap_or(0);
        let stock_remaining = stock_quantity
            .checked_sub(already_reserved)
            .ok_or_else(|| "Saved stock quantities are inconsistent.".to_owned())?;
        let receipt_credit_remaining = *remaining_credit_by_purchase
            .entry(line.purchase_id.clone())
            .or_insert(receipt_credit_remaining);
        let line_total = checked_return_credit_amount(
            line.quantity,
            quantity_remaining,
            stock_remaining,
            receipt_credit_remaining,
            unit_cost_minor,
        )?;
        total_minor = total_minor
            .checked_add(line_total)
            .ok_or_else(|| "The supplier credit amount is too large.".to_owned())?;
        let remaining_credit = remaining_credit_by_purchase
            .get_mut(&line.purchase_id)
            .ok_or_else(|| "The supplier credit allocation could not be updated.".to_owned())?;
        *remaining_credit = remaining_credit
            .checked_sub(line_total)
            .ok_or_else(|| "The supplier credit allocation is inconsistent.".to_owned())?;
        let reserved_stock = reserved_stock_by_product
            .entry(product_id.clone())
            .or_insert(0);
        *reserved_stock = reserved_stock
            .checked_add(line.quantity)
            .ok_or_else(|| "The requested return quantity is too large.".to_owned())?;
        validated.push((line.clone(), product_id, unit_cost_minor, line_total));
    }
    if total_minor <= 0 || total_minor > outstanding_minor {
        return Err("The returned goods value must be positive and cannot exceed the invoice's unpaid balance.".to_owned());
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO supplier_bill_credit_notes(id,business_id,store_id,bill_id,supplier_id,credit_reference,reference_key,effective_date,reason,currency_code,total_minor,idempotency_key,created_by,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![id,session.business_id,session.store_id,input.bill_id,supplier_id,reference,reference_key,input.effective_date,reason,currency_code,total_minor,retry_key,session.user_id,now],
    )
    .map_err(|error| format!("Could not save supplier credit note: {error}"))?;
    for (line, product_id, unit_cost_minor, line_total) in &validated {
        tx.execute(
            "INSERT INTO supplier_bill_credit_items(id,business_id,store_id,credit_note_id,purchase_id,purchase_item_id,product_id,quantity,unit_cost_minor,amount_minor,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![Uuid::new_v4().to_string(),session.business_id,session.store_id,id,line.purchase_id,line.purchase_item_id,product_id,line.quantity,unit_cost_minor,line_total,now],
        )
        .map_err(|error| error.to_string())?;
        let changed = tx.execute(
            "UPDATE products SET stock_quantity=stock_quantity-?1,updated_at=?2 WHERE id=?3 AND store_id=?4 AND stock_quantity>=?1",
            params![line.quantity,now,product_id,session.store_id],
        )
        .map_err(|error| error.to_string())?;
        if changed != 1 {
            return Err(
                "Stock changed while recording the return. Refresh the delivery and try again."
                    .to_owned(),
            );
        }
        tx.execute(
            "INSERT INTO stock_movements(id,store_id,product_id,user_id,quantity_delta,reason,reference_id,note,created_at)
             VALUES(?1,?2,?3,?4,?5,'adjustment',?6,?7,?8)",
            params![Uuid::new_v4().to_string(),session.store_id,product_id,session.user_id,-line.quantity,id,format!("Supplier return credit {}",reference),now],
        )
        .map_err(|error| error.to_string())?;
    }
    accounting::post_supplier_credit_for_session(&tx, session, &id)?;
    add_audit_event(
        &tx,
        session,
        "supplier_credit_note_recorded",
        "supplier_credit_note",
        Some(&id),
        serde_json::json!({"billId":input.bill_id,"supplierId":supplier_id,"creditReference":reference,"amountMinor":total_minor,"returnLineCount":validated.len(),"idempotencyKey":retry_key}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(SupplierBillingRecord {
        id,
        amount_minor: total_minor,
        currency_code,
        replayed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credit_fixture() -> (rusqlite::Connection, crate::models::SessionUser) {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE suppliers(id TEXT, store_id TEXT, name TEXT);
             CREATE TABLE products(id TEXT, store_id TEXT, stock_quantity INTEGER, updated_at TEXT);
             CREATE TABLE purchases(id TEXT, store_id TEXT, supplier_id TEXT, reference TEXT, total_minor INTEGER, received_at TEXT, created_at TEXT);
             CREATE TABLE purchase_items(id TEXT, purchase_id TEXT, product_id TEXT, quantity INTEGER, unit_cost_minor INTEGER, line_total_minor INTEGER);
             CREATE TABLE supplier_bills(id TEXT, business_id TEXT, store_id TEXT, supplier_id TEXT, currency_code TEXT, total_minor INTEGER);
             CREATE TABLE supplier_bill_allocations(store_id TEXT, bill_id TEXT, purchase_id TEXT, amount_minor INTEGER);
             CREATE TABLE supplier_payment_allocations(store_id TEXT, bill_id TEXT, amount_minor INTEGER);
             CREATE TABLE supplier_bill_credit_notes(id TEXT PRIMARY KEY, business_id TEXT, store_id TEXT, bill_id TEXT, supplier_id TEXT, credit_reference TEXT, reference_key TEXT, effective_date TEXT, reason TEXT, currency_code TEXT, total_minor INTEGER, idempotency_key TEXT, created_by TEXT, created_at TEXT, UNIQUE(store_id,idempotency_key), UNIQUE(business_id,supplier_id,reference_key));
             CREATE TABLE supplier_bill_credit_items(id TEXT, business_id TEXT, store_id TEXT, credit_note_id TEXT, purchase_id TEXT, purchase_item_id TEXT, product_id TEXT, quantity INTEGER, unit_cost_minor INTEGER, amount_minor INTEGER, created_at TEXT);
             CREATE TABLE stock_movements(id TEXT, store_id TEXT, product_id TEXT, user_id TEXT, quantity_delta INTEGER, reason TEXT, reference_id TEXT, note TEXT, created_at TEXT);
             CREATE TABLE audit_events(id TEXT, business_id TEXT, store_id TEXT, user_id TEXT, action TEXT, entity_type TEXT, entity_id TEXT, details_json TEXT, created_at TEXT);
             CREATE TABLE accounting_accounts(id TEXT PRIMARY KEY, business_id TEXT, code TEXT, name TEXT, account_type TEXT, is_system INTEGER, is_active INTEGER, created_at TEXT, UNIQUE(business_id,code));
             CREATE TABLE accounting_journals(id TEXT PRIMARY KEY, business_id TEXT, store_id TEXT, source_type TEXT, source_id TEXT, currency_code TEXT, effective_at TEXT, recorded_at TEXT, created_by TEXT, description TEXT, coverage_status TEXT, reversal_of TEXT, UNIQUE(business_id,source_type,source_id));
             CREATE TABLE accounting_journal_lines(id TEXT PRIMARY KEY, business_id TEXT, journal_id TEXT, account_id TEXT, line_number INTEGER, memo TEXT, debit_minor INTEGER, credit_minor INTEGER);
             INSERT INTO suppliers VALUES('supplier-a','store-a','Supplier A');
             INSERT INTO products VALUES('product-a','store-a',10,'2026-11-07T00:00:00Z');
             INSERT INTO purchases VALUES('purchase-a','store-a','supplier-a','PO-1',5000,'2026-11-06','2026-11-06');
             INSERT INTO purchase_items VALUES('item-a','purchase-a','product-a',10,500,5000);
             INSERT INTO supplier_bills VALUES('bill-a','business-a','store-a','supplier-a','GHS',5000);
             INSERT INTO supplier_bill_allocations VALUES('store-a','bill-a','purchase-a',5000);",
        )
        .unwrap();
        let session = crate::models::SessionUser {
            user_id: "owner-a".to_owned(),
            business_id: "business-a".to_owned(),
            store_id: "store-a".to_owned(),
            full_name: "Owner A".to_owned(),
            username: "owner".to_owned(),
            role_key: "owner".to_owned(),
            role_name: "Owner".to_owned(),
            workspace_mode: "management".to_owned(),
            is_owner: true,
            permissions: vec!["suppliers:settle".to_owned()],
            business_name: "Business A".to_owned(),
            store_name: "Store A".to_owned(),
            currency_code: "GHS".to_owned(),
        };
        (db, session)
    }

    fn return_input(key: &str, quantity: i64) -> CreateSupplierCreditNoteInput {
        CreateSupplierCreditNoteInput {
            bill_id: "bill-a".to_owned(),
            credit_reference: "CN-1".to_owned(),
            effective_date: "2026-11-07".to_owned(),
            reason: "Damaged goods returned".to_owned(),
            idempotency_key: key.to_owned(),
            returns: vec![SupplierReturnLineInput {
                purchase_id: "purchase-a".to_owned(),
                purchase_item_id: "item-a".to_owned(),
                quantity,
            }],
        }
    }

    fn record_return(
        db: &mut rusqlite::Connection,
        session: &crate::models::SessionUser,
        input: &CreateSupplierCreditNoteInput,
    ) -> Result<SupplierBillingRecord, String> {
        create_supplier_credit_note_in_transaction(
            db,
            session,
            input,
            input.credit_reference.trim(),
            input.reason.trim(),
            input.idempotency_key.trim(),
        )
    }

    fn scalar(db: &rusqlite::Connection, query: &str) -> i64 {
        db.query_row(query, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn supplier_return_command_replays_exactly_and_rejects_changed_payload() {
        let (mut db, session) = credit_fixture();
        let input = return_input("retry-1", 2);
        let first = record_return(&mut db, &session, &input).unwrap();
        assert_eq!(first.amount_minor, 1000);
        assert!(!first.replayed);
        assert_eq!(
            scalar(
                &db,
                "SELECT stock_quantity FROM products WHERE id='product-a'"
            ),
            8
        );
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM stock_movements"), 1);
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) FROM accounting_journals WHERE source_type='supplier_credit'"
            ),
            1
        );
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM audit_events"), 1);
        let retry = record_return(&mut db, &session, &input).unwrap();
        assert!(retry.replayed);
        assert_eq!(retry.id, first.id);
        assert_eq!(
            scalar(
                &db,
                "SELECT stock_quantity FROM products WHERE id='product-a'"
            ),
            8
        );
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM stock_movements"), 1);
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM accounting_journals"), 1);
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM audit_events"), 1);

        let changed = return_input("retry-1", 1);
        assert!(record_return(&mut db, &session, &changed).is_err());
        assert_eq!(
            scalar(
                &db,
                "SELECT stock_quantity FROM products WHERE id='product-a'"
            ),
            8
        );
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM supplier_bill_credit_notes"),
            1
        );
    }

    #[test]
    fn supplier_return_command_rolls_back_all_writes_when_audit_fails() {
        let (mut db, session) = credit_fixture();
        db.execute_batch(
            "CREATE TRIGGER fail_credit_audit BEFORE INSERT ON audit_events
             BEGIN SELECT RAISE(ABORT, 'fixture audit failure'); END;",
        )
        .unwrap();
        let input = return_input("retry-rollback", 2);
        assert!(record_return(&mut db, &session, &input).is_err());
        assert_eq!(
            scalar(
                &db,
                "SELECT stock_quantity FROM products WHERE id='product-a'"
            ),
            10
        );
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM supplier_bill_credit_notes"),
            0
        );
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM supplier_bill_credit_items"),
            0
        );
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM stock_movements"), 0);
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM accounting_journals"), 0);
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM accounting_journal_lines"),
            0
        );
    }

    #[test]
    fn supplier_return_credit_is_bounded_by_receipt_stock_and_allocated_value() {
        assert_eq!(
            checked_return_credit_amount(2, 3, 2, 200, 100).unwrap(),
            200
        );
        assert!(checked_return_credit_amount(4, 3, 9, 900, 100).is_err());
        assert!(checked_return_credit_amount(3, 9, 2, 900, 100).is_err());
        assert!(checked_return_credit_amount(3, 9, 9, 299, 100).is_err());
        assert!(checked_return_credit_amount(2, 9, 9, i64::MAX, i64::MAX).is_err());
        assert!(checked_return_credit_amount(0, 9, 9, 900, 100).is_err());
        assert!(checked_return_credit_amount(1, 9, 9, 900, 0).is_err());
    }

    #[test]
    fn allocation_totals_reject_duplicates_nonpositive_and_overflow() {
        let one = BillAllocationInput {
            purchase_id: "receipt-a".to_owned(),
            amount_minor: 125,
        };
        let two = BillAllocationInput {
            purchase_id: "receipt-b".to_owned(),
            amount_minor: 375,
        };
        assert_eq!(
            checked_allocations(
                &[one.clone(), two],
                |a| a.purchase_id.clone(),
                |a| a.amount_minor,
                "deliveries"
            )
            .unwrap(),
            500
        );
        assert!(checked_allocations(
            &[one.clone(), one],
            |a| a.purchase_id.clone(),
            |a| a.amount_minor,
            "deliveries"
        )
        .is_err());
        let zero = BillAllocationInput {
            purchase_id: "receipt-zero".to_owned(),
            amount_minor: 0,
        };
        assert!(checked_allocations(
            &[zero],
            |a| a.purchase_id.clone(),
            |a| a.amount_minor,
            "deliveries"
        )
        .is_err());
        let large = BillAllocationInput {
            purchase_id: "receipt-large-a".to_owned(),
            amount_minor: i64::MAX,
        };
        let larger = BillAllocationInput {
            purchase_id: "receipt-large-b".to_owned(),
            amount_minor: 1,
        };
        assert!(checked_allocations(
            &[large, larger],
            |a| a.purchase_id.clone(),
            |a| a.amount_minor,
            "deliveries"
        )
        .is_err());
    }
}
