// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BillingError {
    InvalidInput,
    NotFound,
    Conflict,
    IdempotencyConflict,
    DrawerProof,
    DrawerLease,
    CashLocationUnavailable,
    CashLocationConflict,
    CashLocationInsufficientFunds,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MatchBillInput {
    supplier_id: String,
    supplier_reference: String,
    invoice_date: String,
    due_date: Option<String>,
    total_minor: i64,
    allocations: Vec<BillAllocationInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecordSupplierPaymentInput {
    supplier_id: String,
    payment_method: String,
    external_reference: Option<String>,
    amount_minor: i64,
    #[serde(default)]
    drawer_shift_id: Option<String>,
    #[serde(default)]
    cash_location_id: Option<String>,
    allocations: Vec<PaymentAllocationInput>,
}

pub(crate) struct DrawerPaymentProof<'a> {
    pub station_id: &'a str,
    pub lease_id: &'a str,
    pub lease_token: &'a str,
    pub challenge_id: &'a str,
    pub proof_hex: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PaymentAllocationInput {
    bill_id: String,
    amount_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BillAllocationInput {
    receipt_id: String,
    amount_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SupplierReturnLineInput {
    receipt_id: String,
    receipt_line_id: String,
    quantity: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateSupplierCreditNoteInput {
    bill_id: String,
    credit_reference: String,
    effective_date: String,
    reason: String,
    lines: Vec<SupplierReturnLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnbilledReceipt {
    pub receipt_id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub supplier_reference: Option<String>,
    pub purchase_order_id: String,
    pub received_at: String,
    pub total_minor: i64,
    pub billed_minor: i64,
    pub available_minor: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupplierBill {
    pub id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub supplier_reference: String,
    pub invoice_date: String,
    pub due_date: Option<String>,
    pub currency_code: String,
    pub total_minor: i64,
    pub paid_minor: i64,
    pub credited_minor: i64,
    pub balance_minor: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupplierCreditNote {
    pub id: String,
    pub bill_id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub credit_reference: String,
    pub effective_date: String,
    pub reason: String,
    pub currency_code: String,
    pub total_minor: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupplierReturnCandidate {
    pub bill_id: String,
    pub receipt_id: String,
    pub receipt_reference: Option<String>,
    pub receipt_line_id: String,
    pub product_id: String,
    pub sku: String,
    pub product_name: String,
    pub unit_cost_minor: i64,
    pub max_quantity: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupplierCreditNoteResult {
    #[serde(flatten)]
    pub credit_note: SupplierCreditNote,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupplierPayment {
    pub id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub payment_method: String,
    pub cash_location_id: Option<String>,
    pub external_reference: Option<String>,
    pub amount_minor: i64,
    pub currency_code: String,
    pub paid_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupplierBillingWorkspace {
    pub currency_code: String,
    pub unbilled_receipts: Vec<UnbilledReceipt>,
    pub bills: Vec<SupplierBill>,
    pub payments: Vec<SupplierPayment>,
    pub credit_notes: Vec<SupplierCreditNote>,
    pub returnable_items: Vec<SupplierReturnCandidate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MatchBillResult {
    #[serde(flatten)]
    pub bill: SupplierBill,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecordSupplierPaymentResult {
    #[serde(flatten)]
    pub payment: SupplierPayment,
    pub replayed: bool,
}

pub(crate) async fn workspace(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<SupplierBillingWorkspace, BillingError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let currency_code = sqlx::query_scalar::<_, String>(
        "SELECT currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(BillingError::NotFound)?;
    let unbilled_receipts = sqlx::query_as::<_, UnbilledReceipt>(
        "SELECT r.id::text AS receipt_id, r.supplier_id::text AS supplier_id,
                s.name AS supplier_name, r.reference AS supplier_reference,
                r.order_id::text AS purchase_order_id, r.received_at::text AS received_at,
                r.total_minor,
                COALESCE(SUM(a.amount_minor),0)::bigint AS billed_minor,
                (r.total_minor-COALESCE(SUM(a.amount_minor),0))::bigint AS available_minor
         FROM hosted_purchase_receipts r
         JOIN hosted_suppliers s ON s.tenant_id=r.tenant_id AND s.id=r.supplier_id
         LEFT JOIN hosted_supplier_bill_allocations a ON a.tenant_id=r.tenant_id AND a.branch_id=r.branch_id AND a.receipt_id=r.id
         WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.supplier_id IS NOT NULL
         GROUP BY r.id,s.id HAVING r.total_minor>COALESCE(SUM(a.amount_minor),0)
         ORDER BY r.received_at DESC,r.id LIMIT 500",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let bills = sqlx::query_as::<_, SupplierBill>(
        "SELECT b.id::text AS id,b.supplier_id::text AS supplier_id,s.name AS supplier_name,
                b.supplier_reference,b.invoice_date::text AS invoice_date,b.due_date::text AS due_date,
                b.currency_code,b.total_minor,
                COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)::bigint AS paid_minor,
                COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0)::bigint AS credited_minor,
                (b.total_minor
                  -COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)
                  -COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0))::bigint AS balance_minor,
                b.created_at::text AS created_at
         FROM hosted_supplier_bills b JOIN hosted_suppliers s ON s.tenant_id=b.tenant_id AND s.id=b.supplier_id
         WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid
         ORDER BY b.invoice_date DESC,b.created_at DESC,b.id DESC LIMIT 500",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let payments = sqlx::query_as::<_, SupplierPayment>(
        "SELECT p.id::text AS id,p.supplier_id::text AS supplier_id,s.name AS supplier_name,
                p.payment_method,p.cash_location_id::text AS cash_location_id,p.external_reference,p.amount_minor,p.currency_code,p.paid_at::text AS paid_at
         FROM hosted_supplier_payments p JOIN hosted_suppliers s ON s.tenant_id=p.tenant_id AND s.id=p.supplier_id
         WHERE p.tenant_id=$1::uuid AND p.branch_id=$2::uuid
         ORDER BY p.paid_at DESC,p.id DESC LIMIT 500",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let credit_notes = sqlx::query_as::<_, SupplierCreditNote>(
        "SELECT c.id::text AS id,c.bill_id::text AS bill_id,c.supplier_id::text AS supplier_id,
                s.name AS supplier_name,c.credit_reference,c.effective_date::text AS effective_date,
                c.reason,c.currency_code,c.total_minor,c.created_at::text AS created_at
         FROM hosted_supplier_credit_notes c
         JOIN hosted_suppliers s ON s.tenant_id=c.tenant_id AND s.id=c.supplier_id
         WHERE c.tenant_id=$1::uuid AND c.branch_id=$2::uuid
         ORDER BY c.effective_date DESC,c.created_at DESC,c.id DESC LIMIT 500",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let returnable_items = sqlx::query_as::<_, SupplierReturnCandidate>(
        "SELECT b.id::text AS bill_id,r.id::text AS receipt_id,r.reference AS receipt_reference,
                l.id::text AS receipt_line_id,l.product_id::text AS product_id,p.sku,p.name AS product_name,
                l.actual_unit_cost_minor AS unit_cost_minor,
                LEAST(
                  l.quantity-COALESCE((SELECT SUM(old_line.quantity) FROM hosted_supplier_credit_lines old_line
                    WHERE old_line.tenant_id=l.tenant_id AND old_line.branch_id=l.branch_id AND old_line.receipt_line_id=l.id),0),
                  item.stock_quantity-COALESCE((SELECT SUM(commitment.quantity) FROM inventory_allocations commitment
                    WHERE commitment.tenant_id=item.tenant_id AND commitment.branch_id=item.branch_id
                      AND commitment.product_id=item.product_id AND commitment.released_at IS NULL
                      AND (commitment.expires_at IS NULL OR commitment.expires_at>clock_timestamp())),0),
                  (a.amount_minor-COALESCE((SELECT SUM(old_line.amount_minor)
                    FROM hosted_supplier_credit_notes old_note JOIN hosted_supplier_credit_lines old_line
                      ON old_line.tenant_id=old_note.tenant_id AND old_line.branch_id=old_note.branch_id AND old_line.credit_note_id=old_note.id
                    WHERE old_note.tenant_id=b.tenant_id AND old_note.branch_id=b.branch_id AND old_note.bill_id=b.id
                      AND old_line.receipt_id=r.id),0))/l.actual_unit_cost_minor
                )::bigint AS max_quantity
         FROM hosted_supplier_bills b
         JOIN hosted_supplier_bill_allocations a ON a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id
         JOIN hosted_purchase_receipts r ON r.tenant_id=a.tenant_id AND r.branch_id=a.branch_id AND r.id=a.receipt_id
         JOIN hosted_purchase_receipt_lines l ON l.tenant_id=r.tenant_id AND l.branch_id=r.branch_id AND l.receipt_id=r.id
         JOIN catalog_products p ON p.tenant_id=l.tenant_id AND p.id=l.product_id
         JOIN branch_catalog_items item ON item.tenant_id=l.tenant_id AND item.branch_id=l.branch_id AND item.product_id=l.product_id
         WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND l.actual_unit_cost_minor>0
           AND b.total_minor
             -COALESCE((SELECT SUM(paid.amount_minor) FROM hosted_supplier_payment_allocations paid WHERE paid.tenant_id=b.tenant_id AND paid.branch_id=b.branch_id AND paid.bill_id=b.id),0)
             -COALESCE((SELECT SUM(old_note.total_minor) FROM hosted_supplier_credit_notes old_note WHERE old_note.tenant_id=b.tenant_id AND old_note.branch_id=b.branch_id AND old_note.bill_id=b.id),0)>0
           AND LEAST(
             l.quantity-COALESCE((SELECT SUM(old_line.quantity) FROM hosted_supplier_credit_lines old_line
               WHERE old_line.tenant_id=l.tenant_id AND old_line.branch_id=l.branch_id AND old_line.receipt_line_id=l.id),0),
             item.stock_quantity-COALESCE((SELECT SUM(commitment.quantity) FROM inventory_allocations commitment
               WHERE commitment.tenant_id=item.tenant_id AND commitment.branch_id=item.branch_id
                 AND commitment.product_id=item.product_id AND commitment.released_at IS NULL
                 AND (commitment.expires_at IS NULL OR commitment.expires_at>clock_timestamp())),0),
             (a.amount_minor-COALESCE((SELECT SUM(old_line.amount_minor)
               FROM hosted_supplier_credit_notes old_note JOIN hosted_supplier_credit_lines old_line
                 ON old_line.tenant_id=old_note.tenant_id AND old_line.branch_id=old_note.branch_id AND old_line.credit_note_id=old_note.id
               WHERE old_note.tenant_id=b.tenant_id AND old_note.branch_id=b.branch_id AND old_note.bill_id=b.id
                 AND old_line.receipt_id=r.id),0))/l.actual_unit_cost_minor
           )>0
         ORDER BY b.invoice_date,b.supplier_reference,r.received_at,p.name LIMIT 2000",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(SupplierBillingWorkspace {
        currency_code,
        unbilled_receipts,
        bills,
        payments,
        credit_notes,
        returnable_items,
    })
}

pub(crate) async fn match_bill(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: MatchBillInput,
) -> Result<MatchBillResult, BillingError> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(BillingError::InvalidInput);
    }
    let supplier_id =
        Uuid::parse_str(&input.supplier_id).map_err(|_| BillingError::InvalidInput)?;
    let supplier_reference = input.supplier_reference.trim().to_owned();
    if supplier_reference.is_empty()
        || supplier_reference.chars().count() > 120
        || !(1..=MAX_SAFE_INTEGER).contains(&input.total_minor)
        || input.allocations.is_empty()
        || input.allocations.len() > 100
        || !valid_date(&input.invoice_date)
        || input
            .due_date
            .as_ref()
            .is_some_and(|date| !valid_date(date) || date < &input.invoice_date)
    {
        return Err(BillingError::InvalidInput);
    }
    let mut seen = std::collections::HashSet::new();
    let mut allocations = Vec::with_capacity(input.allocations.len());
    let mut sum = 0_i64;
    for allocation in input.allocations {
        let receipt =
            Uuid::parse_str(&allocation.receipt_id).map_err(|_| BillingError::InvalidInput)?;
        if !(1..=MAX_SAFE_INTEGER).contains(&allocation.amount_minor) || !seen.insert(receipt) {
            return Err(BillingError::InvalidInput);
        }
        sum = sum
            .checked_add(allocation.amount_minor)
            .ok_or(BillingError::InvalidInput)?;
        if sum > MAX_SAFE_INTEGER {
            return Err(BillingError::InvalidInput);
        }
        allocations.push((receipt, allocation.amount_minor));
    }
    if sum != input.total_minor {
        return Err(BillingError::InvalidInput);
    }
    allocations.sort_by_key(|(id, _)| *id);

    let mut request_hash = Sha256::new();
    request_hash.update(raw);
    request_hash.update(b"\nbranch:");
    request_hash.update(branch.as_bytes());
    let request_hash = request_hash.finalize().to_vec();
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let lock_key = format!("supplier-bill:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(old_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM hosted_supplier_bills WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    ).bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(database_error)? {
        if old_hash != request_hash { return Err(BillingError::IdempotencyConflict); }
        let bill = load_bill(&mut tx, tenant, branch, key, membership).await?;
        tx.commit().await.map_err(database_error)?;
        return Ok(MatchBillResult { bill, replayed: true });
    }
    let currency = sqlx::query_scalar::<_, String>("SELECT currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'")
        .bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)?;
    let supplier_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_suppliers WHERE tenant_id=$1::uuid AND id=$2)",
    )
    .bind(tenant)
    .bind(supplier_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    if !supplier_exists {
        return Err(BillingError::NotFound);
    }

    let receipt_ids: Vec<Uuid> = allocations.iter().map(|(id, _)| *id).collect();
    // Receipt rows are immutable, so the runtime role intentionally has no UPDATE privilege.
    // Serialize invoice allocators with transaction-scoped locks rather than SELECT FOR UPDATE.
    for receipt_id in &receipt_ids {
        let lock_key = format!("supplier-bill-receipt:{tenant}:{branch}:{receipt_id}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
    }
    for (receipt_id, amount) in &allocations {
        let (receipt_supplier, receipt_total, already_billed) = sqlx::query_as::<_, (Option<Uuid>, i64, i64)>(
            "SELECT r.supplier_id,r.total_minor,COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_bill_allocations a WHERE a.tenant_id=r.tenant_id AND a.branch_id=r.branch_id AND a.receipt_id=r.id),0)::bigint FROM hosted_purchase_receipts r WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.id=$3",
        ).bind(tenant).bind(branch).bind(receipt_id).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)?;
        if receipt_supplier != Some(supplier_id) || *amount > receipt_total - already_billed {
            return Err(BillingError::Conflict);
        }
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO hosted_supplier_bills(id,tenant_id,branch_id,supplier_id,supplier_reference,invoice_date,due_date,currency_code,total_minor,actor_membership_id,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6::date,$7::date,$8,$9,$10::uuid,$11,$12)",
    ).bind(id).bind(tenant).bind(branch).bind(supplier_id).bind(&supplier_reference).bind(&input.invoice_date).bind(&input.due_date).bind(&currency).bind(input.total_minor).bind(membership).bind(key).bind(&request_hash)
    .execute(&mut *tx).await.map_err(map_insert_error)?;
    for (receipt_id, amount) in allocations {
        sqlx::query("INSERT INTO hosted_supplier_bill_allocations(tenant_id,branch_id,bill_id,receipt_id,amount_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5)")
            .bind(tenant).bind(branch).bind(id).bind(receipt_id).bind(amount).execute(&mut *tx).await.map_err(database_error)?;
    }
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'supplier_bill_matched','supplier_bill',$3,$4)")
        .bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"supplierId":supplier_id.to_string(),"supplierReference":supplier_reference,"totalMinor":input.total_minor,"invoiceDate":input.invoice_date,"receiptCount":seen.len()}))
        .execute(&mut *tx).await.map_err(database_error)?;
    let bill_effective_at = format!("{}T00:00:00Z", input.invoice_date);
    insert_journal(
        &mut tx,
        tenant,
        branch,
        membership,
        "supplier_bill",
        id,
        &currency,
        &bill_effective_at,
        &format!("Matched supplier invoice {supplier_reference}"),
        2110,
        2000,
        input.total_minor,
    )
    .await?;
    let bill = load_bill(&mut tx, tenant, branch, &id.to_string(), "").await?;
    tx.commit().await.map_err(database_error)?;
    Ok(MatchBillResult {
        bill,
        replayed: false,
    })
}

pub(crate) async fn create_credit_note(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: CreateSupplierCreditNoteInput,
) -> Result<SupplierCreditNoteResult, BillingError> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(BillingError::InvalidInput);
    }
    let bill_id = Uuid::parse_str(&input.bill_id).map_err(|_| BillingError::InvalidInput)?;
    let reference = input.credit_reference.trim().to_owned();
    let reference_key = reference.to_lowercase();
    let reason = input.reason.trim().to_owned();
    if reference.is_empty()
        || reference.chars().count() > 120
        || reason.is_empty()
        || reason.chars().count() > 500
        || !valid_date(&input.effective_date)
        || input.lines.is_empty()
        || input.lines.len() > 100
    {
        return Err(BillingError::InvalidInput);
    }
    let mut seen_lines = std::collections::HashSet::new();
    let mut lines = Vec::with_capacity(input.lines.len());
    for line in input.lines {
        let receipt_id =
            Uuid::parse_str(&line.receipt_id).map_err(|_| BillingError::InvalidInput)?;
        let receipt_line_id =
            Uuid::parse_str(&line.receipt_line_id).map_err(|_| BillingError::InvalidInput)?;
        if !(1..=1_000_000).contains(&line.quantity) || !seen_lines.insert(receipt_line_id) {
            return Err(BillingError::InvalidInput);
        }
        lines.push((receipt_id, receipt_line_id, line.quantity));
    }
    lines.sort_by_key(|(_, receipt_line_id, _)| *receipt_line_id);

    let mut request_hash = Sha256::new();
    request_hash.update(raw);
    request_hash.update(b"\nbranch:");
    request_hash.update(branch.as_bytes());
    let request_hash = request_hash.finalize().to_vec();
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let retry_lock = format!("supplier-credit:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(retry_lock)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(previous_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM hosted_supplier_credit_notes WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    )
    .bind(tenant)
    .bind(branch)
    .bind(membership)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    {
        if previous_hash != request_hash {
            return Err(BillingError::IdempotencyConflict);
        }
        let credit_note = load_credit_note_by_key(&mut tx, tenant, branch, membership, key).await?;
        tx.commit().await.map_err(database_error)?;
        return Ok(SupplierCreditNoteResult { credit_note, replayed: true });
    }

    // Supplier payments and returns share the bill lock so their remaining balance cannot race.
    let bill_lock = format!("supplier-payment-bill:{tenant}:{branch}:{bill_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(bill_lock)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    for (_, receipt_line_id, _) in &lines {
        let lock = format!("supplier-credit-receipt-line:{tenant}:{branch}:{receipt_line_id}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(lock)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
    }

    let (supplier_id, currency, total, paid, credited) = sqlx::query_as::<_, (Uuid, String, i64, i64, i64)>(
        "SELECT b.supplier_id,b.currency_code,b.total_minor,
          COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)::bigint,
          COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0)::bigint
         FROM hosted_supplier_bills b WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND b.id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(bill_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(BillingError::NotFound)?;
    let outstanding = total
        .checked_sub(paid)
        .and_then(|value| value.checked_sub(credited))
        .ok_or(BillingError::Conflict)?;
    if outstanding <= 0 {
        return Err(BillingError::Conflict);
    }
    let duplicate_reference = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_supplier_credit_notes WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND supplier_id=$3 AND reference_key=$4)",
    )
    .bind(tenant)
    .bind(branch)
    .bind(supplier_id)
    .bind(&reference_key)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    if duplicate_reference {
        return Err(BillingError::Conflict);
    }

    struct ValidatedLine {
        receipt_id: Uuid,
        receipt_line_id: Uuid,
        product_id: Uuid,
        quantity: i64,
        unit_cost: i64,
        amount: i64,
    }
    let mut validated = Vec::with_capacity(lines.len());
    let mut total_credit = 0_i64;
    let mut credit_by_receipt = std::collections::HashMap::<Uuid, i64>::new();
    let mut product_ids = Vec::with_capacity(lines.len());
    for (receipt_id, receipt_line_id, quantity) in lines {
        let source = sqlx::query_as::<_, (Uuid, i64, i64, i64, i64, i64, i64)>(
            "SELECT l.product_id,l.quantity,l.actual_unit_cost_minor,item.stock_quantity,
              COALESCE((SELECT SUM(old_line.quantity) FROM hosted_supplier_credit_lines old_line WHERE old_line.tenant_id=l.tenant_id AND old_line.branch_id=l.branch_id AND old_line.receipt_line_id=l.id),0)::bigint,
              a.amount_minor,
              COALESCE((SELECT SUM(old_line.amount_minor) FROM hosted_supplier_credit_notes old_note JOIN hosted_supplier_credit_lines old_line ON old_line.tenant_id=old_note.tenant_id AND old_line.branch_id=old_note.branch_id AND old_line.credit_note_id=old_note.id WHERE old_note.tenant_id=b.tenant_id AND old_note.branch_id=b.branch_id AND old_note.bill_id=b.id AND old_line.receipt_id=r.id),0)::bigint
             FROM hosted_supplier_bills b
             JOIN hosted_supplier_bill_allocations a ON a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id AND a.receipt_id=$4
             JOIN hosted_purchase_receipts r ON r.tenant_id=a.tenant_id AND r.branch_id=a.branch_id AND r.id=a.receipt_id AND r.supplier_id=b.supplier_id
             JOIN hosted_purchase_receipt_lines l ON l.tenant_id=r.tenant_id AND l.branch_id=r.branch_id AND l.receipt_id=r.id AND l.id=$5
             JOIN branch_catalog_items item ON item.tenant_id=l.tenant_id AND item.branch_id=l.branch_id AND item.product_id=l.product_id
             WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND b.id=$3",
        )
        .bind(tenant).bind(branch).bind(bill_id).bind(receipt_id).bind(receipt_line_id)
        .fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)?;
        let (
            product_id,
            received,
            unit_cost,
            stock,
            previously_returned,
            receipt_allocation,
            prior_receipt_credit,
        ) = source;
        if unit_cost <= 0
            || quantity > received.saturating_sub(previously_returned)
            || quantity > stock
        {
            return Err(BillingError::Conflict);
        }
        product_ids.push(product_id.to_string());
        let amount = quantity
            .checked_mul(unit_cost)
            .filter(|value| (1..=MAX_SAFE_INTEGER).contains(value))
            .ok_or(BillingError::InvalidInput)?;
        let by_receipt = credit_by_receipt.entry(receipt_id).or_default();
        *by_receipt = by_receipt
            .checked_add(amount)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(BillingError::InvalidInput)?;
        let remaining_receipt_allocation = receipt_allocation
            .checked_sub(prior_receipt_credit)
            .ok_or(BillingError::Conflict)?;
        if *by_receipt > remaining_receipt_allocation {
            return Err(BillingError::Conflict);
        }
        total_credit = total_credit
            .checked_add(amount)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(BillingError::InvalidInput)?;
        validated.push(ValidatedLine {
            receipt_id,
            receipt_line_id,
            product_id,
            quantity,
            unit_cost,
            amount,
        });
    }
    if total_credit > outstanding {
        return Err(BillingError::Conflict);
    }
    crate::inventory_allocations::lock_products(&mut tx, tenant, branch, product_ids.iter())
        .await
        .map_err(database_error)?;
    let mut quantities_by_product = std::collections::HashMap::<Uuid, i64>::new();
    for line in &validated {
        let total_quantity = quantities_by_product.entry(line.product_id).or_default();
        *total_quantity = total_quantity
            .checked_add(line.quantity)
            .ok_or(BillingError::InvalidInput)?;
    }
    for (product_id, quantity) in quantities_by_product {
        let stock = sqlx::query_scalar::<_, i64>(
            "SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3 FOR UPDATE",
        ).bind(tenant).bind(branch).bind(product_id).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)?;
        let committed = crate::inventory_allocations::allocated_quantity(
            &mut tx,
            tenant,
            branch,
            &product_id.to_string(),
            None,
        )
        .await
        .map_err(database_error)?;
        if !crate::inventory_allocations::can_consume(stock, committed, quantity) {
            return Err(BillingError::Conflict);
        }
    }

    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO hosted_supplier_credit_notes(id,tenant_id,branch_id,bill_id,supplier_id,credit_reference,reference_key,effective_date,reason,currency_code,total_minor,actor_membership_id,idempotency_key,request_hash)
         VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7,$8::date,$9,$10,$11,$12::uuid,$13,$14)",
    ).bind(id).bind(tenant).bind(branch).bind(bill_id).bind(supplier_id).bind(&reference).bind(&reference_key).bind(&input.effective_date).bind(&reason).bind(&currency).bind(total_credit).bind(membership).bind(key).bind(&request_hash).execute(&mut *tx).await.map_err(map_insert_error)?;
    for line in &validated {
        sqlx::query("INSERT INTO hosted_supplier_credit_lines(tenant_id,branch_id,credit_note_id,receipt_id,receipt_line_id,product_id,quantity,unit_cost_minor,amount_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8,$9)")
            .bind(tenant).bind(branch).bind(id).bind(line.receipt_id).bind(line.receipt_line_id).bind(line.product_id).bind(line.quantity).bind(line.unit_cost).bind(line.amount).execute(&mut *tx).await.map_err(database_error)?;
        let resulting_stock = sqlx::query_scalar::<_, i64>(
            "UPDATE branch_catalog_items SET stock_quantity=stock_quantity-$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3 RETURNING stock_quantity",
        ).bind(tenant).bind(branch).bind(line.product_id).bind(line.quantity).fetch_one(&mut *tx).await.map_err(database_error)?;
        let movement_key = format!("supplier-credit:{id}:{}", line.receipt_line_id);
        sqlx::query("INSERT INTO stock_movements(tenant_id,branch_id,product_id,actor_membership_id,movement_kind,quantity_delta,note,source_id,idempotency_key,resulting_stock_quantity) VALUES($1::uuid,$2::uuid,$3,$4::uuid,'adjustment',$5,$6,$7,$8,$9)")
            .bind(tenant).bind(branch).bind(line.product_id).bind(membership).bind(-line.quantity).bind(format!("Supplier return credit {reference}")).bind(id).bind(movement_key).bind(resulting_stock).execute(&mut *tx).await.map_err(database_error)?;
        sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'supplier_return_stock_removed','catalog_product',$3,$4)")
            .bind(tenant).bind(membership).bind(line.product_id).bind(serde_json::json!({"branchId":branch,"creditNoteId":id.to_string(),"receiptLineId":line.receipt_line_id.to_string(),"quantity":line.quantity,"resultingStockQuantity":resulting_stock})).execute(&mut *tx).await.map_err(database_error)?;
    }
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'supplier_credit_note_recorded','supplier_credit_note',$3,$4)")
        .bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"billId":bill_id.to_string(),"supplierId":supplier_id.to_string(),"reference":reference,"amountMinor":total_credit,"lineCount":validated.len()})).execute(&mut *tx).await.map_err(database_error)?;
    let effective_at = format!("{}T00:00:00Z", input.effective_date);
    insert_journal(
        &mut tx,
        tenant,
        branch,
        membership,
        "supplier_credit",
        id,
        &currency,
        &effective_at,
        &format!("Supplier return credit {reference}"),
        2000,
        1300,
        total_credit,
    )
    .await?;
    let credit_note = load_credit_note(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(SupplierCreditNoteResult {
        credit_note,
        replayed: false,
    })
}

pub(crate) async fn record_payment(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: RecordSupplierPaymentInput,
    drawer_proof: Option<DrawerPaymentProof<'_>>,
) -> Result<RecordSupplierPaymentResult, BillingError> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(BillingError::InvalidInput);
    }
    let supplier_id =
        Uuid::parse_str(&input.supplier_id).map_err(|_| BillingError::InvalidInput)?;
    let method = input.payment_method.trim();
    let account_code = match method {
        "cash" => 1000,
        "card" => 1010,
        "mobile_money" => 1020,
        "bank_transfer" => 1030,
        "other" => 1090,
        _ => return Err(BillingError::InvalidInput),
    };
    let reference = input
        .external_reference
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if reference.is_some_and(|v| v.chars().count() > 100)
        || !(1..=MAX_SAFE_INTEGER).contains(&input.amount_minor)
        || input.allocations.is_empty()
        || input.allocations.len() > 100
    {
        return Err(BillingError::InvalidInput);
    }
    let drawer_shift_id = input.drawer_shift_id.as_deref();
    let cash_location_id = input
        .cash_location_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| BillingError::InvalidInput)?;
    match (
        method,
        drawer_shift_id,
        cash_location_id,
        drawer_proof.as_ref(),
    ) {
        ("cash", Some(shift), None, Some(proof))
            if Uuid::parse_str(shift).is_ok() && Uuid::parse_str(proof.station_id).is_ok() => {}
        ("cash", None, Some(_), None) => {}
        ("cash", _, _, _) => return Err(BillingError::InvalidInput),
        (_, None, None, None) => {}
        _ => return Err(BillingError::InvalidInput),
    }
    let mut seen = std::collections::HashSet::new();
    let mut allocations = Vec::with_capacity(input.allocations.len());
    let mut sum = 0_i64;
    for allocation in input.allocations {
        let bill_id =
            Uuid::parse_str(&allocation.bill_id).map_err(|_| BillingError::InvalidInput)?;
        if !(1..=MAX_SAFE_INTEGER).contains(&allocation.amount_minor) || !seen.insert(bill_id) {
            return Err(BillingError::InvalidInput);
        }
        sum = sum
            .checked_add(allocation.amount_minor)
            .ok_or(BillingError::InvalidInput)?;
        if sum > MAX_SAFE_INTEGER {
            return Err(BillingError::InvalidInput);
        }
        allocations.push((bill_id, allocation.amount_minor));
    }
    if sum != input.amount_minor {
        return Err(BillingError::InvalidInput);
    }
    allocations.sort_by_key(|(id, _)| *id);

    let mut hasher = Sha256::new();
    hasher.update(raw);
    hasher.update(b"\nbranch:");
    hasher.update(branch.as_bytes());
    let request_hash = hasher.finalize().to_vec();
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let lock_key = format!("supplier-payment:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(old_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM hosted_supplier_payments WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    ).bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(database_error)? {
        if old_hash != request_hash { return Err(BillingError::IdempotencyConflict); }
        let payment = load_payment_by_key(&mut tx, tenant, branch, membership, key).await?;
        if let Some(proof) = drawer_proof {
            link_payment_to_drawer(
                &mut tx, identity, tenant, membership, branch, proof, drawer_shift_id.unwrap(),
                key, raw, &payment.id, input.amount_minor,
            ).await?;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(RecordSupplierPaymentResult { payment, replayed: true });
    }
    let currency = sqlx::query_scalar::<_, String>("SELECT currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'")
        .bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)?;
    let supplier_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_suppliers WHERE tenant_id=$1::uuid AND id=$2 AND status='active')")
        .bind(tenant).bind(supplier_id).fetch_one(&mut *tx).await.map_err(database_error)?;
    if !supplier_exists {
        return Err(BillingError::NotFound);
    }
    if let Some(location_id) = cash_location_id {
        crate::cash_locations::validate_source_outflow(
            &mut tx,
            tenant,
            branch,
            location_id,
            input.amount_minor,
        )
        .await
        .map_err(map_cash_location_error)?;
    }
    for (bill_id, _) in &allocations {
        let lock_key = format!("supplier-payment-bill:{tenant}:{branch}:{bill_id}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
    }
    for (bill_id, amount) in &allocations {
        let bill = sqlx::query_as::<_, (Uuid,String,i64,i64,i64)>(
            "SELECT b.supplier_id,b.currency_code,b.total_minor,
                COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)::bigint,
                COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0)::bigint
             FROM hosted_supplier_bills b WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND b.id=$3",
        ).bind(tenant).bind(branch).bind(bill_id).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)?;
        let (bill_supplier, bill_currency, total, paid, credited) = bill;
        if bill_supplier != supplier_id
            || bill_currency != currency
            || *amount > total.saturating_sub(paid).saturating_sub(credited)
        {
            return Err(BillingError::Conflict);
        }
    }
    let id = Uuid::new_v4();
    let (paid_at,) = sqlx::query_as::<_, (String,)>(
        "INSERT INTO hosted_supplier_payments(id,tenant_id,branch_id,supplier_id,currency_code,payment_method,cash_location_id,external_reference,amount_minor,actor_membership_id,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7,$8,$9,$10::uuid,$11,$12) RETURNING paid_at::text",
    ).bind(id).bind(tenant).bind(branch).bind(supplier_id).bind(&currency).bind(method).bind(cash_location_id).bind(reference).bind(input.amount_minor).bind(membership).bind(key).bind(&request_hash).fetch_one(&mut *tx).await.map_err(map_insert_error)?;
    if let Some(proof) = drawer_proof {
        link_payment_to_drawer(
            &mut tx,
            identity,
            tenant,
            membership,
            branch,
            proof,
            drawer_shift_id.unwrap(),
            key,
            raw,
            &id.to_string(),
            input.amount_minor,
        )
        .await?;
    }
    for (bill_id, amount) in &allocations {
        sqlx::query("INSERT INTO hosted_supplier_payment_allocations(tenant_id,branch_id,payment_id,bill_id,amount_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5)")
            .bind(tenant).bind(branch).bind(id).bind(bill_id).bind(amount).execute(&mut *tx).await.map_err(database_error)?;
    }
    if let Some(location_id) = cash_location_id {
        crate::cash_locations::record_supplier_payment_outflow(
            &mut tx,
            tenant,
            branch,
            membership,
            id,
            location_id,
            input.amount_minor,
        )
        .await
        .map_err(map_cash_location_error)?;
    }
    insert_journal(
        &mut tx,
        tenant,
        branch,
        membership,
        "supplier_payment",
        id,
        &currency,
        &paid_at,
        "Recorded manual supplier payment",
        2000,
        account_code,
        input.amount_minor,
    )
    .await?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'supplier_payment_recorded','supplier_payment',$3,$4)")
        .bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"supplierId":supplier_id.to_string(),"paymentMethod":method,"cashLocationId":cash_location_id.map(|value|value.to_string()),"externalReference":reference,"amountMinor":input.amount_minor,"allocationCount":seen.len()}))
        .execute(&mut *tx).await.map_err(database_error)?;
    let payment = load_payment(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(RecordSupplierPaymentResult {
        payment,
        replayed: false,
    })
}

async fn link_payment_to_drawer(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    proof: DrawerPaymentProof<'_>,
    shift_id: &str,
    key: &str,
    raw: &[u8],
    payment_id: &str,
    amount_minor: i64,
) -> Result<(), BillingError> {
    let payment_id = Uuid::parse_str(payment_id).map_err(|_| BillingError::Database)?;
    crate::drawer::record_supplier_payment_outflow(
        tx,
        identity,
        membership,
        tenant,
        branch,
        proof.station_id,
        shift_id,
        proof.lease_id,
        proof.lease_token,
        key,
        proof.challenge_id,
        proof.proof_hex,
        raw,
        payment_id,
        amount_minor,
    )
    .await
    .map_err(|error| match error {
        crate::drawer::DrawerError::LeaseUnavailable => BillingError::DrawerLease,
        crate::drawer::DrawerError::InvalidProof => BillingError::DrawerProof,
        crate::drawer::DrawerError::NoOpenShift
        | crate::drawer::DrawerError::ShiftUnavailable
        | crate::drawer::DrawerError::RegisterCutoverRequired => BillingError::Conflict,
        crate::drawer::DrawerError::InvalidInput
        | crate::drawer::DrawerError::IdempotencyConflict => BillingError::IdempotencyConflict,
        crate::drawer::DrawerError::ShiftAlreadyOpen
        | crate::drawer::DrawerError::AmountOverflow
        | crate::drawer::DrawerError::Database => BillingError::Database,
    })
}

async fn insert_journal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    source_type: &str,
    source_id: Uuid,
    currency: &str,
    effective_at: &str,
    description: &str,
    debit_account: i32,
    credit_account: i32,
    amount: i64,
) -> Result<(), BillingError> {
    let journal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7::timestamptz,$8::uuid,$9)")
        .bind(journal_id).bind(tenant).bind(branch).bind(source_type).bind(source_id).bind(currency).bind(effective_at).bind(membership).bind(description)
        .execute(&mut **tx).await.map_err(database_error)?;
    for (line_number, account_code, line_description, debit, credit) in [
        (1_i16, debit_account, "Debit source account", amount, 0_i64),
        (
            2_i16,
            credit_account,
            "Credit offset account",
            0_i64,
            amount,
        ),
    ] {
        sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(branch).bind(journal_id).bind(line_number).bind(format!("{account_code:04}" )).bind(line_description).bind(debit).bind(credit)
            .execute(&mut **tx).await.map_err(database_error)?;
    }
    Ok(())
}

async fn load_payment_by_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<SupplierPayment, BillingError> {
    sqlx::query_as::<_, SupplierPayment>("SELECT p.id::text AS id,p.supplier_id::text AS supplier_id,s.name AS supplier_name,p.payment_method,p.cash_location_id::text AS cash_location_id,p.external_reference,p.amount_minor,p.currency_code,p.paid_at::text AS paid_at FROM hosted_supplier_payments p JOIN hosted_suppliers s ON s.tenant_id=p.tenant_id AND s.id=p.supplier_id WHERE p.tenant_id=$1::uuid AND p.branch_id=$2::uuid AND p.actor_membership_id=$3::uuid AND p.idempotency_key=$4")
        .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut **tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)
}

async fn load_payment(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: Uuid,
) -> Result<SupplierPayment, BillingError> {
    sqlx::query_as::<_, SupplierPayment>("SELECT p.id::text AS id,p.supplier_id::text AS supplier_id,s.name AS supplier_name,p.payment_method,p.cash_location_id::text AS cash_location_id,p.external_reference,p.amount_minor,p.currency_code,p.paid_at::text AS paid_at FROM hosted_supplier_payments p JOIN hosted_suppliers s ON s.tenant_id=p.tenant_id AND s.id=p.supplier_id WHERE p.tenant_id=$1::uuid AND p.branch_id=$2::uuid AND p.id=$3")
        .bind(tenant).bind(branch).bind(id).fetch_optional(&mut **tx).await.map_err(database_error)?.ok_or(BillingError::NotFound)
}

async fn load_credit_note(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: Uuid,
) -> Result<SupplierCreditNote, BillingError> {
    sqlx::query_as::<_, SupplierCreditNote>(
        "SELECT c.id::text AS id,c.bill_id::text AS bill_id,c.supplier_id::text AS supplier_id,s.name AS supplier_name,c.credit_reference,c.effective_date::text AS effective_date,c.reason,c.currency_code,c.total_minor,c.created_at::text AS created_at FROM hosted_supplier_credit_notes c JOIN hosted_suppliers s ON s.tenant_id=c.tenant_id AND s.id=c.supplier_id WHERE c.tenant_id=$1::uuid AND c.branch_id=$2::uuid AND c.id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .ok_or(BillingError::NotFound)
}

async fn load_credit_note_by_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<SupplierCreditNote, BillingError> {
    sqlx::query_as::<_, SupplierCreditNote>(
        "SELECT c.id::text AS id,c.bill_id::text AS bill_id,c.supplier_id::text AS supplier_id,s.name AS supplier_name,c.credit_reference,c.effective_date::text AS effective_date,c.reason,c.currency_code,c.total_minor,c.created_at::text AS created_at FROM hosted_supplier_credit_notes c JOIN hosted_suppliers s ON s.tenant_id=c.tenant_id AND s.id=c.supplier_id WHERE c.tenant_id=$1::uuid AND c.branch_id=$2::uuid AND c.actor_membership_id=$3::uuid AND c.idempotency_key=$4",
    )
    .bind(tenant)
    .bind(branch)
    .bind(membership)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .ok_or(BillingError::NotFound)
}

async fn load_bill(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    selector: &str,
    membership: &str,
) -> Result<SupplierBill, BillingError> {
    let by_key = !membership.is_empty();
    let row = if by_key {
        sqlx::query_as::<_, SupplierBill>("SELECT b.id::text AS id,b.supplier_id::text AS supplier_id,s.name AS supplier_name,b.supplier_reference,b.invoice_date::text AS invoice_date,b.due_date::text AS due_date,b.currency_code,b.total_minor,COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)::bigint AS paid_minor,COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0)::bigint AS credited_minor,(b.total_minor-COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)-COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0))::bigint AS balance_minor,b.created_at::text AS created_at FROM hosted_supplier_bills b JOIN hosted_suppliers s ON s.tenant_id=b.tenant_id AND s.id=b.supplier_id WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND b.idempotency_key=$3 AND b.actor_membership_id=$4::uuid")
            .bind(tenant).bind(branch).bind(selector).bind(membership).fetch_optional(&mut **tx).await.map_err(database_error)?
    } else {
        sqlx::query_as::<_, SupplierBill>("SELECT b.id::text AS id,b.supplier_id::text AS supplier_id,s.name AS supplier_name,b.supplier_reference,b.invoice_date::text AS invoice_date,b.due_date::text AS due_date,b.currency_code,b.total_minor,COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)::bigint AS paid_minor,COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0)::bigint AS credited_minor,(b.total_minor-COALESCE((SELECT SUM(a.amount_minor) FROM hosted_supplier_payment_allocations a WHERE a.tenant_id=b.tenant_id AND a.branch_id=b.branch_id AND a.bill_id=b.id),0)-COALESCE((SELECT SUM(c.total_minor) FROM hosted_supplier_credit_notes c WHERE c.tenant_id=b.tenant_id AND c.branch_id=b.branch_id AND c.bill_id=b.id),0))::bigint AS balance_minor,b.created_at::text AS created_at FROM hosted_supplier_bills b JOIN hosted_suppliers s ON s.tenant_id=b.tenant_id AND s.id=b.supplier_id WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND b.id=$3::uuid")
            .bind(tenant).bind(branch).bind(selector).fetch_optional(&mut **tx).await.map_err(database_error)?
    };
    row.ok_or(BillingError::NotFound)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<(), BillingError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", branch),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(database_error)?;
    }
    Ok(())
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<i32>().unwrap_or(0);
    let month = value[5..7].parse::<u32>().unwrap_or(0);
    let day = value[8..10].parse::<u32>().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    (1..=9999).contains(&year) && (1..=max).contains(&day)
}

fn database_error(error: sqlx::Error) -> BillingError {
    #[cfg(test)]
    eprintln!("supplier billing SQL failed: {error}");
    #[cfg(not(test))]
    let _ = error;
    BillingError::Database
}

fn map_insert_error(error: sqlx::Error) -> BillingError {
    if error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation())
    {
        BillingError::Conflict
    } else {
        database_error(error)
    }
}

fn map_cash_location_error(error: crate::cash_locations::CashLocationError) -> BillingError {
    match error {
        crate::cash_locations::CashLocationError::InvalidInput => BillingError::InvalidInput,
        crate::cash_locations::CashLocationError::NotFound => BillingError::CashLocationUnavailable,
        crate::cash_locations::CashLocationError::Conflict => BillingError::CashLocationConflict,
        crate::cash_locations::CashLocationError::CutoverBlocked => {
            BillingError::CashLocationConflict
        }
        crate::cash_locations::CashLocationError::InsufficientFunds => {
            BillingError::CashLocationInsufficientFunds
        }
        crate::cash_locations::CashLocationError::Database => BillingError::Database,
    }
}

#[cfg(test)]
mod tests {
    use super::valid_date;

    #[test]
    fn validates_calendar_dates_for_invoice_fields() {
        assert!(valid_date("2024-02-29"));
        assert!(valid_date("2026-10-31"));
        assert!(!valid_date("2025-02-29"));
        assert!(!valid_date("2026-04-31"));
        assert!(!valid_date("2026-13-01"));
        assert!(!valid_date("2026-1-01"));
    }
}
