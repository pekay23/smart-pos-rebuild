// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;
use uuid::Uuid;

const MAX_AMOUNT: i64 = 9_000_000_000_000_000;
const MAX_ITEMS: usize = 500;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RefundInput {
    pub kind: String,
    pub reason: String,
    pub items: Vec<RefundItemInput>,
    pub payment_method: String,
    pub payment_reference: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RefundItemInput {
    pub sale_line_id: String,
    pub quantity: i64,
    pub restock: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RefundReceipt {
    pub id: String,
    pub sale_id: String,
    pub receipt_number: String,
    pub kind: String,
    pub total_minor: i64,
    pub reason: String,
    pub payment_method: String,
    pub payment_reference: Option<String>,
    pub created_at: String,
    pub cashier_name: String,
    pub station_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RefundableSaleItem {
    pub sale_line_id: String,
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub sold_quantity: i64,
    pub refunded_quantity: i64,
    pub remaining_quantity: i64,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub refundable_total_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaleRefundDetail {
    pub sale_id: String,
    pub receipt_number: String,
    pub cashier_name: String,
    pub status: String,
    pub currency_code: String,
    pub total_minor: i64,
    pub on_account_minor: i64,
    pub items: Vec<RefundableSaleItem>,
    pub refunds: Vec<RefundReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefundError {
    InvalidInput,
    LeaseUnavailable,
    InvalidProof,
    NotFound,
    SaleNotRefundable,
    IdempotencyConflict,
    DrawerRequired,
    RegisterCutoverRequired,
    DrawerCashInsufficient,
    StockOverflow,
    Database,
}

pub(crate) async fn detail(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    sale_id: &str,
) -> Result<SaleRefundDetail, RefundError> {
    let sale = Uuid::parse_str(sale_id).map_err(|_| RefundError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| RefundError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let row = sqlx::query_as::<_, (String, String, String, String, i64, i64)>(
        "SELECT id::text,receipt_number,cashier_name_snapshot,status,total_minor,on_account_minor
         FROM hosted_sales WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
           AND station_id=$3::uuid AND id=$4",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(sale)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?
    .ok_or(RefundError::NotFound)?;
    let currency_code = sqlx::query_scalar::<_, String>(
        "SELECT currency_code::text FROM hosted_sales WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?;
    let lines = sqlx::query_as::<_, (String, String, String, String, i64, i64, i64, i64)>(
        "SELECT sl.id::text,sl.product_id::text,sl.sku_snapshot,sl.name_snapshot,
                sl.quantity,sl.subtotal_minor,sl.tax_minor,sl.total_minor
         FROM hosted_sale_lines sl WHERE sl.tenant_id=$1::uuid AND sl.branch_id=$2::uuid
           AND sl.sale_id=$3 ORDER BY sl.line_number",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?;
    let mut items = Vec::with_capacity(lines.len());
    for line in lines {
        let (refunded, refunded_amount) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(sum(ri.quantity),0)::bigint,COALESCE(sum(ri.amount_minor),0)::bigint
             FROM hosted_sale_refund_lines ri JOIN hosted_sale_refunds r
               ON r.tenant_id=ri.tenant_id AND r.branch_id=ri.branch_id AND r.id=ri.refund_id
             WHERE ri.tenant_id=$1::uuid AND ri.branch_id=$2::uuid
               AND ri.sale_id=$3 AND ri.sale_line_id=$4",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(sale)
        .bind(Uuid::parse_str(&line.0).map_err(|_| RefundError::Database)?)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;
        items.push(RefundableSaleItem {
            sale_line_id: line.0,
            product_id: line.1,
            sku: line.2,
            name: line.3,
            sold_quantity: line.4,
            refunded_quantity: refunded,
            remaining_quantity: line.4.saturating_sub(refunded),
            subtotal_minor: line.5,
            tax_minor: line.6,
            refundable_total_minor: line.7.saturating_sub(refunded_amount),
        });
    }
    let refunds = sqlx::query_as::<_, RefundReceipt>(
        "SELECT r.id::text,r.sale_id::text,s.receipt_number,r.refund_type AS kind,r.total_minor,
                r.reason,t.method AS payment_method,t.external_reference AS payment_reference,
                to_char(r.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                r.cashier_name_snapshot AS cashier_name,r.station_name_snapshot AS station_name
         FROM hosted_sale_refunds r JOIN hosted_sales s
           ON s.tenant_id=r.tenant_id AND s.branch_id=r.branch_id AND s.id=r.sale_id
         JOIN hosted_sale_refund_tenders t
           ON t.tenant_id=r.tenant_id AND t.branch_id=r.branch_id AND t.refund_id=r.id
         WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.sale_id=$3
         ORDER BY r.created_at DESC,r.id DESC LIMIT 100",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?;
    tx.commit().await.map_err(|_| RefundError::Database)?;
    Ok(SaleRefundDetail {
        sale_id: row.0,
        receipt_number: row.1,
        cashier_name: row.2,
        status: row.3,
        currency_code,
        total_minor: row.4,
        on_account_minor: row.5,
        items,
        refunds,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn refund(
    pool: &PgPool,
    identity_id: &str,
    cashier_display_name: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    sale_id: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    input: RefundInput,
) -> Result<RefundReceipt, RefundError> {
    let sale = Uuid::parse_str(sale_id).map_err(|_| RefundError::InvalidInput)?;
    let challenge_uuid = Uuid::parse_str(challenge_id).map_err(|_| RefundError::InvalidProof)?;
    if !valid_key(idempotency_key)
        || input.items.is_empty()
        || input.items.len() > MAX_ITEMS
        || !matches!(input.kind.as_str(), "refund" | "void")
        || input.reason.trim().chars().count() < 3
        || input.reason.trim().chars().count() > 250
        || !matches!(
            input.payment_method.as_str(),
            "cash" | "card" | "mobile_money" | "bank_transfer" | "other"
        )
    {
        return Err(RefundError::InvalidInput);
    }
    let reason = input.reason.trim();
    let reference = input
        .payment_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if reference.is_some_and(|value| value.chars().count() > 160)
        || (input.payment_method != "cash" && reference.is_none())
    {
        return Err(RefundError::InvalidInput);
    }
    let mut seen = HashSet::new();
    let mut line_ids = Vec::with_capacity(input.items.len());
    for item in &input.items {
        if item.quantity <= 0
            || item.quantity > MAX_AMOUNT
            || !seen.insert(item.sale_line_id.as_str())
        {
            return Err(RefundError::InvalidInput);
        }
        line_ids.push(Uuid::parse_str(&item.sale_line_id).map_err(|_| RefundError::InvalidInput)?);
    }
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let lease_token_hash = Sha256::digest(lease_token.as_bytes()).to_vec();
    let signature = decode_hex(proof_hex, 64).ok_or(RefundError::InvalidProof)?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| RefundError::InvalidProof)?;

    let mut tx = pool.begin().await.map_err(|_| RefundError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("refund:{tenant_id}:{sale_id}"))
        .execute(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;

    let lease = sqlx::query_as::<_, (String, Vec<u8>, String, String)>(
        "SELECT l.activation_id::text,a.device_public_key,
                COALESCE(NULLIF(btrim($8),''),'Cashier'),s.name
         FROM station_actor_leases l
         JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id
           AND k.station_id=l.station_id AND k.lease_id=l.id
         JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id
           AND a.station_id=l.station_id AND a.id=l.activation_id
         JOIN memberships m ON m.id=l.membership_id AND m.tenant_id=l.tenant_id
         JOIN stations s ON s.id=l.station_id AND s.tenant_id=l.tenant_id AND s.branch_id=l.branch_id
         JOIN membership_role_grants cashier_grant ON cashier_grant.tenant_id=m.tenant_id
           AND cashier_grant.membership_id=m.id AND cashier_grant.role_key='cashier'
         JOIN membership_role_grants refund_grant ON refund_grant.tenant_id=m.tenant_id
           AND refund_grant.membership_id=m.id AND refund_grant.role_key IN ('owner','admin','manager')
         WHERE l.id=$1::uuid AND l.tenant_id=$2::uuid AND l.branch_id=$3::uuid AND l.station_id=$4::uuid
           AND l.membership_id=$5::uuid AND l.identity_id=$6::uuid AND l.lease_token_hash=$7
           AND l.status='issued' AND l.expires_at>now() AND a.status='active' AND m.status='active'
           AND (cashier_grant.branch_id IS NULL OR cashier_grant.branch_id=l.branch_id)
           AND (cashier_grant.station_id IS NULL OR cashier_grant.station_id=l.station_id)
           AND (refund_grant.branch_id IS NULL OR refund_grant.branch_id=l.branch_id)
           AND refund_grant.station_id IS NULL
         LIMIT 1 FOR UPDATE OF l",
    )
    .bind(lease_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(lease_token_hash)
    .bind(cashier_display_name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?
    .ok_or(RefundError::LeaseUnavailable)?;
    let challenge = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        "SELECT nonce,payload_hash FROM station_financial_proof_challenges
         WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid
           AND membership_id=$5::uuid AND identity_id=$6::uuid AND activation_id=$7::uuid AND lease_id=$8::uuid
           AND operation='sale.refund' AND idempotency_key=$9 AND consumed_at IS NULL AND expires_at>now()
         FOR UPDATE",
    )
    .bind(challenge_uuid)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(&lease.0)
    .bind(lease_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?
    .ok_or(RefundError::InvalidProof)?;
    if challenge.1 != payload_hash {
        return Err(RefundError::InvalidProof);
    }
    let public_key: [u8; 32] = lease
        .1
        .as_slice()
        .try_into()
        .map_err(|_| RefundError::InvalidProof)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| RefundError::InvalidProof)?;
    let message = format!(
        "advanced-pos/financial-operation/v1\nPOST\n/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales/{sale_id}/refunds\nsale.refund\n{tenant_id}\n{branch_id}\n{station_id}\n{lease_id}\n{membership_id}\n{}\n{challenge_id}\n{idempotency_key}\n{}\n{}",
        lease.0,
        encode_hex(&payload_hash),
        encode_hex(&challenge.0),
    );
    verifying_key
        .verify(message.as_bytes(), &Signature::from_bytes(&signature))
        .map_err(|_| RefundError::InvalidProof)?;

    // Check a signed retry before locking the sale for a new mutation. Once a
    // full void changes the sale status, the update policy intentionally hides
    // it from a fresh mutation, while the append-only refund remains replayable.
    if let Some(existing) = sqlx::query_as::<_, RefundReceipt>(
        "SELECT r.id::text,r.sale_id::text,s.receipt_number,r.refund_type AS kind,r.total_minor,
                r.reason,t.method AS payment_method,t.external_reference AS payment_reference,
                to_char(r.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                r.cashier_name_snapshot AS cashier_name,r.station_name_snapshot AS station_name
         FROM hosted_sale_refunds r JOIN hosted_sales s
           ON s.tenant_id=r.tenant_id AND s.branch_id=r.branch_id AND s.id=r.sale_id
         JOIN hosted_sale_refund_tenders t
           ON t.tenant_id=r.tenant_id AND t.branch_id=r.branch_id AND t.refund_id=r.id
         WHERE r.tenant_id=$1::uuid AND r.station_id=$2::uuid
           AND r.actor_membership_id=$3::uuid AND r.idempotency_key=$4",
    )
    .bind(tenant_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?
    {
        let request_hash = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT request_hash FROM hosted_sale_refunds WHERE id=$1::uuid",
        )
        .bind(&existing.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;
        if existing.sale_id != sale_id || request_hash != payload_hash {
            return Err(RefundError::IdempotencyConflict);
        }
        consume_challenge(&mut tx, challenge_uuid, &payload_hash).await?;
        tx.commit().await.map_err(|_| RefundError::Database)?;
        return Ok(existing);
    }

    let sale_row = sqlx::query_as::<_, (String, String, String, String, String, i64, i64)>(
        "SELECT receipt_number,status,currency_code::text,cashier_name_snapshot,station_name_snapshot,total_minor,on_account_minor
         FROM hosted_sales WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND station_id=$3::uuid
           AND id=$4 AND cashier_membership_id=$5::uuid FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(sale)
    .bind(membership_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?
    .ok_or(RefundError::NotFound)?;

    if sale_row.6 > 0
        || !matches!(sale_row.1.as_str(), "completed" | "partially_refunded")
        || (input.kind == "void" && sale_row.1 != "completed")
    {
        return Err(RefundError::SaleNotRefundable);
    }

    let mut refund_lines = Vec::with_capacity(input.items.len());
    let mut refund_total = 0i64;
    for (item, line_id) in input.items.iter().zip(line_ids) {
        let line = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            "SELECT product_id::text,quantity,subtotal_minor,tax_minor,total_minor
             FROM hosted_sale_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
               AND sale_id=$3 AND id=$4",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(sale)
        .bind(line_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?
        .ok_or(RefundError::InvalidInput)?;
        let (already_refunded, _) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(sum(ri.quantity),0)::bigint,COALESCE(sum(ri.amount_minor),0)::bigint
             FROM hosted_sale_refund_lines ri JOIN hosted_sale_refunds r
               ON r.tenant_id=ri.tenant_id AND r.branch_id=ri.branch_id AND r.id=ri.refund_id
             WHERE ri.tenant_id=$1::uuid AND ri.branch_id=$2::uuid
               AND ri.sale_id=$3 AND ri.sale_line_id=$4",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(sale)
        .bind(line_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;
        let returned_quantity = already_refunded
            .checked_add(item.quantity)
            .filter(|quantity| *quantity <= line.1)
            .ok_or(RefundError::InvalidInput)?;
        if line.1 <= 0 || line.2.checked_add(line.3) != Some(line.4) {
            return Err(RefundError::Database);
        }
        let subtotal = cumulative_part(line.2, line.1, returned_quantity)?
            - cumulative_part(line.2, line.1, already_refunded)?;
        let tax = cumulative_part(line.3, line.1, returned_quantity)?
            - cumulative_part(line.3, line.1, already_refunded)?;
        let amount = subtotal
            .checked_add(tax)
            .filter(|value| *value <= MAX_AMOUNT)
            .ok_or(RefundError::InvalidInput)?;
        refund_total = refund_total
            .checked_add(amount)
            .filter(|value| *value <= MAX_AMOUNT)
            .ok_or(RefundError::InvalidInput)?;
        refund_lines.push((
            line_id,
            Uuid::parse_str(&line.0).map_err(|_| RefundError::Database)?,
            item.quantity,
            item.restock,
            subtotal,
            tax,
            amount,
        ));
    }
    if refund_total <= 0 || refund_total > sale_row.5 {
        return Err(RefundError::InvalidInput);
    }
    if input.kind == "void" {
        let all_lines = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT sl.id::text,sl.quantity,COALESCE(sum(prior.quantity),0)::bigint
             FROM hosted_sale_lines sl LEFT JOIN hosted_sale_refund_lines prior
               ON prior.tenant_id=sl.tenant_id AND prior.branch_id=sl.branch_id
              AND prior.sale_id=sl.sale_id AND prior.sale_line_id=sl.id
             WHERE sl.tenant_id=$1::uuid AND sl.branch_id=$2::uuid AND sl.sale_id=$3
             GROUP BY sl.id ORDER BY sl.id",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(sale)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;
        let requested = input
            .items
            .iter()
            .map(|item| (item.sale_line_id.as_str(), item.quantity))
            .collect::<std::collections::HashMap<_, _>>();
        if all_lines.len() != input.items.len()
            || all_lines.iter().any(|(id, sold, previously_refunded)| {
                *previously_refunded != 0 || requested.get(id.as_str()) != Some(sold)
            })
            || refund_total != sale_row.5
        {
            return Err(RefundError::InvalidInput);
        }
    }

    let mut cash_shift: Option<(Uuid, i64)> = None;
    if input.payment_method == "cash" {
        let shift = sqlx::query_as::<_, (Uuid, i64)>(
            "SELECT id,opening_cash_minor FROM hosted_drawer_shifts
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND station_id=$3::uuid AND status='open'
             FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(station_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?
        .ok_or(RefundError::DrawerRequired)?;
        let cash_sales = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(sum(t.amount_minor),0)::bigint FROM hosted_sales s
             JOIN hosted_sale_tenders t ON t.tenant_id=s.tenant_id AND t.branch_id=s.branch_id AND t.sale_id=s.id
             WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid AND s.station_id=$3::uuid
               AND s.cash_shift_id=$4 AND t.method_key_snapshot='cash'",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(station_id)
        .bind(shift.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;
        let movements = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT COALESCE(sum(amount_minor) FILTER (WHERE movement_type='paid_in'),0)::bigint,
                    COALESCE(sum(amount_minor) FILTER (WHERE movement_type='paid_out'),0)::bigint,
                    COALESCE(sum(amount_minor) FILTER (WHERE movement_type='cash_drop'),0)::bigint
             FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND shift_id=$2",
        )
        .bind(tenant_id)
        .bind(shift.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| RefundError::Database)?;
        let expected = shift
            .1
            .checked_add(cash_sales)
            .and_then(|value| value.checked_add(movements.0))
            .and_then(|value| value.checked_sub(movements.1))
            .and_then(|value| value.checked_sub(movements.2))
            .ok_or(RefundError::Database)?;
        if expected < refund_total {
            return Err(RefundError::DrawerCashInsufficient);
        }
        cash_shift = Some(shift);
    }
    let cash_location_id = if cash_shift.is_some() {
        let configured = sqlx::query_as::<_, (Uuid, bool)>(
            "SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
             FROM hosted_cash_locations l WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.station_id=$3::uuid AND l.location_type='register' AND l.status='active'",
        )
        .bind(tenant_id).bind(branch_id).bind(station_id)
        .fetch_optional(&mut *tx).await.map_err(|_| RefundError::Database)?;
        match configured {
            Some((_, false)) => return Err(RefundError::RegisterCutoverRequired),
            Some((id, true)) => Some(id),
            None => None,
        }
    } else {
        None
    };

    let refund_id = Uuid::new_v4();
    consume_challenge(&mut tx, challenge_uuid, &payload_hash).await?;
    let refund_id_text = refund_id.to_string();
    for (key, value) in [
        ("pos.refund_challenge_id", challenge_id),
        ("pos.refund_id", refund_id_text.as_str()),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|_| RefundError::Database)?;
    }
    let created_at = sqlx::query_scalar::<_, String>(
        "INSERT INTO hosted_sale_refunds(id,tenant_id,branch_id,station_id,sale_id,actor_membership_id,
             lease_id,activation_id,cash_shift_id,refund_type,total_minor,reason,idempotency_key,
             request_hash,cashier_name_snapshot,station_name_snapshot)
         VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5,$6::uuid,$7::uuid,$8::uuid,$9,$10,$11,$12,$13,$14,$15,$16)
         RETURNING to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(refund_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(sale)
    .bind(membership_id)
    .bind(lease_id)
    .bind(&lease.0)
    .bind(cash_shift.as_ref().map(|shift| shift.0))
    .bind(&input.kind)
    .bind(refund_total)
    .bind(reason)
    .bind(idempotency_key)
    .bind(&payload_hash)
    .bind(&lease.2)
    .bind(&lease.3)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| {
        if error.as_database_error().is_some_and(|db| db.is_unique_violation()) {
            RefundError::IdempotencyConflict
        } else {
            RefundError::Database
        }
    })?;
    for (line_id, product_id, quantity, restock, subtotal, tax, amount) in &refund_lines {
        sqlx::query("INSERT INTO hosted_sale_refund_lines(tenant_id,branch_id,sale_id,refund_id,sale_line_id,product_id,quantity,restock,subtotal_minor,tax_minor,amount_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6::uuid,$7,$8,$9,$10,$11)")
            .bind(tenant_id).bind(branch_id).bind(sale).bind(refund_id).bind(line_id).bind(product_id).bind(quantity).bind(restock).bind(subtotal).bind(tax).bind(amount)
            .execute(&mut *tx).await.map_err(|_| RefundError::Database)?;
        if *restock {
            let current = sqlx::query_scalar::<_, i64>(
                "SELECT stock_quantity FROM branch_catalog_items
                 WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid FOR UPDATE",
            )
            .bind(tenant_id)
            .bind(branch_id)
            .bind(product_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| RefundError::Database)?
            .ok_or(RefundError::NotFound)?;
            let new_stock = current
                .checked_add(*quantity)
                .filter(|value| *value <= MAX_AMOUNT)
                .ok_or(RefundError::StockOverflow)?;
            sqlx::query("UPDATE branch_catalog_items SET stock_quantity=$1,revision=revision+1,updated_at=now() WHERE tenant_id=$2::uuid AND branch_id=$3::uuid AND product_id=$4::uuid")
                .bind(new_stock).bind(tenant_id).bind(branch_id).bind(product_id)
                .execute(&mut *tx).await.map_err(|_| RefundError::Database)?;
            let movement_key = format!("{}-{}", refund_id.simple(), line_id.simple());
            sqlx::query("INSERT INTO stock_movements(tenant_id,branch_id,product_id,actor_membership_id,movement_kind,quantity_delta,note,source_id,idempotency_key,resulting_stock_quantity) VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,'sale_return',$5,'Resalable customer return',$6,$7,$8)")
                .bind(tenant_id).bind(branch_id).bind(product_id).bind(membership_id).bind(quantity).bind(refund_id).bind(movement_key).bind(new_stock)
                .execute(&mut *tx).await.map_err(|_| RefundError::Database)?;
        }
    }
    let refund_tender_id = sqlx::query_scalar::<_, Uuid>("INSERT INTO hosted_sale_refund_tenders(tenant_id,branch_id,refund_id,method,amount_minor,external_reference) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6) RETURNING id")
        .bind(tenant_id).bind(branch_id).bind(refund_id).bind(&input.payment_method).bind(refund_total).bind(reference)
        .fetch_one(&mut *tx).await.map_err(|_| RefundError::Database)?;
    if input.payment_method == "cash" {
        if let Some(location_id) = cash_location_id {
            sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_refund_tender_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'sale_refund_outflow',$4,$5,$6::uuid)")
                .bind(tenant_id).bind(branch_id).bind(location_id).bind(refund_tender_id).bind(-refund_total).bind(membership_id)
                .execute(&mut *tx).await.map_err(|_| RefundError::Database)?;
        }
    }
    if let Some((shift_id, _)) = cash_shift {
        sqlx::query("INSERT INTO hosted_drawer_movements(id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,source_refund_id) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5,$6::uuid,$7::uuid,$8::uuid,'paid_out',$9,$10,$11,$12,$13)")
            .bind(Uuid::new_v4()).bind(tenant_id).bind(branch_id).bind(station_id).bind(shift_id).bind(membership_id).bind(lease_id).bind(&lease.0).bind(refund_total).bind(format!("Customer refund {}", sale_row.0)).bind(idempotency_key).bind(&payload_hash).bind(refund_id)
            .execute(&mut *tx).await.map_err(|_| RefundError::Database)?;
    }
    crate::sale_accounting::post_refund(&mut tx, tenant_id, branch_id, membership_id, refund_id)
        .await
        .map_err(|error| {
            #[cfg(test)]
            eprintln!("hosted refund accounting journal failed: {error}");
            #[cfg(not(test))]
            let _ = error;
            RefundError::Database
        })?;
    let total_refunded = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(sum(total_minor),0)::bigint FROM hosted_sale_refunds
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND sale_id=$3",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?;
    if total_refunded > sale_row.5 {
        return Err(RefundError::InvalidInput);
    }
    let new_status = if input.kind == "void" {
        "voided"
    } else if total_refunded == sale_row.5 {
        "refunded"
    } else {
        "partially_refunded"
    };
    let changed = sqlx::query(
        "UPDATE hosted_sales SET status=$1 WHERE tenant_id=$2::uuid AND branch_id=$3::uuid
           AND station_id=$4::uuid AND id=$5 AND cashier_membership_id=$6::uuid",
    )
    .bind(new_status)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(sale)
    .bind(membership_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| RefundError::Database)?;
    if changed.rows_affected() != 1 {
        return Err(RefundError::NotFound);
    }
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,$3,'sale_refund',$4,jsonb_build_object('saleId',$5::text,'receiptNumber',$6,'kind',$7,'totalMinor',$8,'paymentMethod',$9,'reason',$10,'restockedItems',$11))")
        .bind(tenant_id).bind(membership_id).bind(if input.kind == "void" { "sale_voided" } else { "sale_refunded" }).bind(refund_id.to_string()).bind(sale_id).bind(&sale_row.0).bind(&input.kind).bind(refund_total).bind(&input.payment_method).bind(reason).bind(refund_lines.iter().filter(|line| line.3).count() as i64)
        .execute(&mut *tx).await.map_err(|_| RefundError::Database)?;
    tx.commit().await.map_err(|_| RefundError::Database)?;
    Ok(RefundReceipt {
        id: refund_id.to_string(),
        sale_id: sale_id.to_owned(),
        receipt_number: sale_row.0,
        kind: input.kind,
        total_minor: refund_total,
        reason: reason.to_owned(),
        payment_method: input.payment_method,
        payment_reference: reference.map(str::to_owned),
        created_at,
        cashier_name: lease.2,
        station_name: lease.3,
    })
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<(), RefundError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", station_id),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| RefundError::Database)?;
    }
    Ok(())
}

async fn consume_challenge(
    tx: &mut Transaction<'_, Postgres>,
    challenge: Uuid,
    payload_hash: &[u8],
) -> Result<(), RefundError> {
    let result = sqlx::query(
        "UPDATE station_financial_proof_challenges SET consumed_at=now()
         WHERE id=$1 AND consumed_at IS NULL AND expires_at>now() AND payload_hash=$2",
    )
    .bind(challenge)
    .bind(payload_hash)
    .execute(&mut **tx)
    .await
    .map_err(|_| RefundError::Database)?;
    if result.rows_affected() != 1 {
        return Err(RefundError::InvalidProof);
    }
    Ok(())
}

fn cumulative_part(
    total: i64,
    sold_quantity: i64,
    returned_quantity: i64,
) -> Result<i64, RefundError> {
    if total < 0 || sold_quantity <= 0 || returned_quantity < 0 || returned_quantity > sold_quantity
    {
        return Err(RefundError::InvalidInput);
    }
    i64::try_from(i128::from(total) * i128::from(returned_quantity) / i128::from(sold_quantity))
        .map_err(|_| RefundError::InvalidInput)
}

fn valid_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn decode_hex(value: &str, len: usize) -> Option<Vec<u8>> {
    if value.len() != len * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

fn encode_hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::cumulative_part;

    #[test]
    fn cumulative_rounding_allocates_all_minor_units_across_partial_returns() {
        let portions = (0..3)
            .map(|index| {
                let start = cumulative_part(100, 3, index).unwrap();
                let end = cumulative_part(100, 3, index + 1).unwrap();
                end - start
            })
            .collect::<Vec<_>>();
        assert_eq!(portions, [33, 33, 34]);
        assert_eq!(portions.iter().sum::<i64>(), 100);
    }

    #[test]
    fn cumulative_allocation_rejects_quantities_outside_sold_range() {
        assert!(cumulative_part(100, 0, 0).is_err());
        assert!(cumulative_part(100, 2, 3).is_err());
        assert!(cumulative_part(-1, 2, 1).is_err());
    }
}
