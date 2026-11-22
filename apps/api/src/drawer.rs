// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_AMOUNT: i64 = 9_000_000_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenInput {
    pub opening_cash_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MovementInput {
    pub movement_type: String,
    pub amount_minor: i64,
    pub note: String,
    pub destination_location_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashDropDestination {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CloseInput {
    pub counted_cash_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DrawerReport {
    pub shift_id: String,
    pub station_id: String,
    pub opened_by_membership_id: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub opening_cash_minor: i64,
    pub cash_sales_minor: i64,
    pub paid_in_minor: i64,
    pub paid_out_minor: i64,
    pub cash_drops_minor: i64,
    pub expected_cash_minor: i64,
    pub counted_cash_minor: Option<i64>,
    pub difference_minor: Option<i64>,
    pub movement_count: i64,
    pub sale_count: i64,
    pub is_closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawerError {
    InvalidInput,
    LeaseUnavailable,
    InvalidProof,
    ShiftAlreadyOpen,
    ShiftUnavailable,
    IdempotencyConflict,
    NoOpenShift,
    RegisterCutoverRequired,
    AmountOverflow,
    Database,
}

pub(crate) async fn cash_drop_destinations(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<Vec<CashDropDestination>, DrawerError> {
    let mut tx = pool.begin().await.map_err(|_| DrawerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let destinations = sqlx::query_as::<_, CashDropDestination>(
        "SELECT l.id::text AS id,l.name FROM hosted_cash_locations l
         WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.location_type<>'register' AND l.status='active'
           AND EXISTS (SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id
             AND e.branch_id=l.branch_id AND e.location_id=l.id AND e.event_type='opening')
         ORDER BY l.name,l.id",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| DrawerError::Database)?;
    tx.commit().await.map_err(|_| DrawerError::Database)?;
    Ok(destinations)
}

pub(crate) async fn current(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<Option<DrawerReport>, DrawerError> {
    let mut tx = pool.begin().await.map_err(|_| DrawerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let shift_id = sqlx::query_scalar::<_, String>(
        "SELECT id::text FROM hosted_drawer_shifts WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND station_id=$3::uuid AND status='open'",
    )
    .bind(tenant_id).bind(branch_id).bind(station_id)
    .fetch_optional(&mut *tx).await.map_err(|_| DrawerError::Database)?;
    let Some(id) = shift_id else {
        tx.commit().await.map_err(|_| DrawerError::Database)?;
        return Ok(None);
    };
    let report = report(&mut tx, tenant_id, branch_id, station_id, &id).await?;
    tx.commit().await.map_err(|_| DrawerError::Database)?;
    Ok(Some(report))
}

pub(crate) async fn history(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<Vec<DrawerReport>, DrawerError> {
    let mut tx = pool.begin().await.map_err(|_| DrawerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let snapshots = sqlx::query_as::<_, (serde_json::Value,)>(
        "SELECT report_json FROM hosted_drawer_shifts
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND station_id=$3::uuid
           AND status='closed' AND report_json IS NOT NULL
         ORDER BY closed_at DESC, id DESC LIMIT 50",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| DrawerError::Database)?;
    let reports = snapshots
        .into_iter()
        .map(|(snapshot,)| serde_json::from_value(snapshot).map_err(|_| DrawerError::Database))
        .collect::<Result<Vec<DrawerReport>, _>>()?;
    tx.commit().await.map_err(|_| DrawerError::Database)?;
    Ok(reports)
}

pub(crate) async fn open(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    input: OpenInput,
) -> Result<DrawerReport, DrawerError> {
    if input.opening_cash_minor < 0
        || input.opening_cash_minor > MAX_AMOUNT
        || !valid_key(idempotency_key)
    {
        return Err(DrawerError::InvalidInput);
    }
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| DrawerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    lock_station(&mut tx, station_id).await?;
    let lease = verify_operation(
        &mut tx,
        identity_id,
        membership_id,
        tenant_id,
        branch_id,
        station_id,
        lease_id,
        lease_token,
        challenge_id,
        idempotency_key,
        &payload_hash,
        proof_hex,
        "drawer.open",
        None,
    )
    .await?;
    if let Some((id, old_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text,opening_request_hash FROM hosted_drawer_shifts WHERE tenant_id=$1::uuid AND station_id=$2::uuid AND opening_idempotency_key=$3 AND opened_by_membership_id=$4::uuid",
    ).bind(tenant_id).bind(station_id).bind(idempotency_key).bind(membership_id)
        .fetch_optional(&mut *tx).await.map_err(|_| DrawerError::Database)? {
        if old_hash != payload_hash { return Err(DrawerError::IdempotencyConflict); }
        consume_challenge(&mut tx, challenge_id, &payload_hash).await?;
        let result = report(&mut tx, tenant_id, branch_id, station_id, &id).await?;
        tx.commit().await.map_err(|_| DrawerError::Database)?;
        return Ok(result);
    }
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_drawer_shifts WHERE tenant_id=$1::uuid AND station_id=$2::uuid AND status='open')")
        .bind(tenant_id).bind(station_id).fetch_one(&mut *tx).await.map_err(|_| DrawerError::Database)? {
        return Err(DrawerError::ShiftAlreadyOpen);
    }
    let shift_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_drawer_shifts(id,tenant_id,branch_id,station_id,opened_by_membership_id,opening_lease_id,opening_activation_id,opening_idempotency_key,opening_request_hash,opening_cash_minor) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8,$9,$10)")
        .bind(shift_id).bind(tenant_id).bind(branch_id).bind(station_id).bind(membership_id).bind(lease_id).bind(lease.activation_id).bind(idempotency_key).bind(&payload_hash).bind(input.opening_cash_minor)
        .execute(&mut *tx).await.map_err(|e| if e.as_database_error().is_some_and(|d| d.is_unique_violation()) { DrawerError::ShiftAlreadyOpen } else { DrawerError::Database })?;
    consume_challenge(&mut tx, challenge_id, &payload_hash).await?;
    audit(
        &mut tx,
        tenant_id,
        membership_id,
        "drawer_shift_opened",
        "drawer_shift",
        &shift_id.to_string(),
        serde_json::json!({"openingCashMinor":input.opening_cash_minor,"stationId":station_id}),
    )
    .await?;
    let result = report(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        &shift_id.to_string(),
    )
    .await?;
    tx.commit().await.map_err(|_| DrawerError::Database)?;
    Ok(result)
}

pub(crate) async fn movement(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    shift_id: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    input: MovementInput,
) -> Result<DrawerReport, DrawerError> {
    let note = input.note.trim();
    let destination_location_id = input
        .destination_location_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| DrawerError::InvalidInput)?;
    if !matches!(
        input.movement_type.as_str(),
        "paid_in" | "paid_out" | "cash_drop"
    ) || input.amount_minor <= 0
        || input.amount_minor > MAX_AMOUNT
        || note.is_empty()
        || note.chars().count() > 200
        || !valid_key(idempotency_key)
        || Uuid::parse_str(shift_id).is_err()
        || (input.movement_type == "cash_drop") != destination_location_id.is_some()
    {
        return Err(DrawerError::InvalidInput);
    }
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| DrawerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    lock_station(&mut tx, station_id).await?;
    let lease = verify_operation(
        &mut tx,
        identity_id,
        membership_id,
        tenant_id,
        branch_id,
        station_id,
        lease_id,
        lease_token,
        challenge_id,
        idempotency_key,
        &payload_hash,
        proof_hex,
        "drawer.movement",
        Some(shift_id),
    )
    .await?;
    if let Some((old_hash,)) = sqlx::query_as::<_, (Vec<u8>,)>("SELECT request_hash FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND station_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4")
        .bind(tenant_id).bind(station_id).bind(membership_id).bind(idempotency_key).fetch_optional(&mut *tx).await.map_err(|_| DrawerError::Database)? {
        if old_hash != payload_hash { return Err(DrawerError::IdempotencyConflict); }
        consume_challenge(&mut tx, challenge_id, &payload_hash).await?;
        let result = report(&mut tx, tenant_id, branch_id, station_id, shift_id).await?;
        tx.commit().await.map_err(|_| DrawerError::Database)?;
        return Ok(result);
    }
    let open = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_drawer_shifts WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid AND status='open')")
        .bind(shift_id).bind(tenant_id).bind(branch_id).bind(station_id).fetch_one(&mut *tx).await.map_err(|_| DrawerError::Database)?;
    if !open {
        return Err(DrawerError::NoOpenShift);
    }
    if input.movement_type == "cash_drop"
        && input.amount_minor
            > report(&mut tx, tenant_id, branch_id, station_id, shift_id)
                .await?
                .expected_cash_minor
    {
        return Err(DrawerError::InvalidInput);
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_drawer_movements(id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,cash_destination_location_id) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8::uuid,$9,$10,$11,$12,$13,$14)")
        .bind(id).bind(tenant_id).bind(branch_id).bind(station_id).bind(shift_id).bind(membership_id).bind(lease_id).bind(lease.activation_id).bind(&input.movement_type).bind(input.amount_minor).bind(note).bind(idempotency_key).bind(&payload_hash).bind(destination_location_id)
        .execute(&mut *tx).await.map_err(|_| DrawerError::Database)?;
    let register = sqlx::query_as::<_, (Uuid, bool)>(
        "SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
         FROM hosted_cash_locations l WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.station_id=$3::uuid AND l.location_type='register' AND l.status='active'",
    )
    .bind(tenant_id).bind(branch_id).bind(station_id)
    .fetch_optional(&mut *tx).await.map_err(|_| DrawerError::Database)?;
    if input.movement_type == "cash_drop" && register.is_none() {
        return Err(DrawerError::RegisterCutoverRequired);
    }
    if let Some(destination) = destination_location_id {
        let eligible = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM hosted_cash_locations l
             WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.id=$3
               AND l.location_type<>'register' AND l.status='active'
               AND EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id
                 AND e.branch_id=l.branch_id AND e.location_id=l.id AND e.event_type='opening'))",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(destination)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| DrawerError::Database)?;
        if !eligible {
            return Err(DrawerError::InvalidInput);
        }
    }
    if let Some((register_location_id, register_initialized)) = register {
        if !register_initialized {
            return Err(DrawerError::RegisterCutoverRequired);
        }
        let (event_type, signed_amount) = if input.movement_type == "paid_in" {
            ("register_movement_inflow", input.amount_minor)
        } else {
            ("register_movement_outflow", -input.amount_minor)
        };
        sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_drawer_movement_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7::uuid)")
            .bind(tenant_id).bind(branch_id).bind(register_location_id).bind(event_type).bind(id).bind(signed_amount).bind(membership_id)
            .execute(&mut *tx).await.map_err(|_| DrawerError::Database)?;
        if let Some(destination) = destination_location_id {
            sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_drawer_movement_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'cash_drop_inflow',$4,$5,$6::uuid)")
                .bind(tenant_id).bind(branch_id).bind(destination).bind(id).bind(input.amount_minor).bind(membership_id)
                .execute(&mut *tx).await.map_err(|_| DrawerError::Database)?;
        }
    }
    consume_challenge(&mut tx, challenge_id, &payload_hash).await?;
    audit(&mut tx, tenant_id, membership_id, "drawer_movement_recorded", "drawer_movement", &id.to_string(), serde_json::json!({"type":input.movement_type,"amountMinor":input.amount_minor,"note":note})).await?;
    let result = report(&mut tx, tenant_id, branch_id, station_id, shift_id).await?;
    tx.commit().await.map_err(|_| DrawerError::Database)?;
    Ok(result)
}

pub(crate) async fn record_supplier_payment_outflow(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    membership: &str,
    tenant: &str,
    branch: &str,
    station: &str,
    shift: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    payment_id: Uuid,
    amount_minor: i64,
) -> Result<(), DrawerError> {
    let payload_hash = Sha256::digest(raw_body).to_vec();
    set_context(tx, identity, tenant, membership, branch, station).await?;
    lock_station(tx, station).await?;
    let lease = verify_operation(
        tx,
        identity,
        membership,
        tenant,
        branch,
        station,
        lease_id,
        lease_token,
        challenge_id,
        idempotency_key,
        &payload_hash,
        proof_hex,
        "supplier.payment",
        Some(shift),
    )
    .await?;

    if let Some((old_shift, old_amount, old_key, old_hash)) = sqlx::query_as::<_, (String, i64, String, Vec<u8>)>(
        "SELECT shift_id::text,amount_minor,idempotency_key,request_hash FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_payment_id=$2::uuid",
    )
    .bind(tenant)
    .bind(payment_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| DrawerError::Database)?
    {
        if old_shift != shift || old_amount != amount_minor || old_key != idempotency_key || old_hash != payload_hash {
            return Err(DrawerError::IdempotencyConflict);
        }
        consume_challenge(tx, challenge_id, &payload_hash).await?;
        return Ok(());
    }

    let open = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_drawer_shifts WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid AND status='open')")
        .bind(shift).bind(tenant).bind(branch).bind(station).fetch_one(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    if !open {
        return Err(DrawerError::NoOpenShift);
    }
    let movement_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_drawer_movements(id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,source_payment_id) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8::uuid,'paid_out',$9,$10,$11,$12,$13::uuid)")
        .bind(movement_id).bind(tenant).bind(branch).bind(station).bind(shift).bind(membership).bind(lease_id).bind(lease.activation_id).bind(amount_minor).bind(format!("Supplier payment {payment_id}" )).bind(idempotency_key).bind(&payload_hash).bind(payment_id)
        .execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    link_source_outflow_to_register(
        tx,
        tenant,
        branch,
        station,
        membership,
        movement_id,
        "supplier_payment_outflow",
        payment_id,
        amount_minor,
    )
    .await?;
    consume_challenge(tx, challenge_id, &payload_hash).await?;
    audit(tx, tenant, membership, "drawer_movement_recorded", "drawer_movement", &movement_id.to_string(), serde_json::json!({"type":"paid_out","amountMinor":amount_minor,"sourceType":"supplier_payment","sourceId":payment_id.to_string()})).await?;
    Ok(())
}

pub(crate) async fn record_expense_outflow(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    membership: &str,
    tenant: &str,
    branch: &str,
    station: &str,
    shift: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    expense_id: Uuid,
    amount_minor: i64,
) -> Result<(), DrawerError> {
    let payload_hash = Sha256::digest(raw_body).to_vec();
    set_context(tx, identity, tenant, membership, branch, station).await?;
    lock_station(tx, station).await?;
    let lease = verify_operation(
        tx,
        identity,
        membership,
        tenant,
        branch,
        station,
        lease_id,
        lease_token,
        challenge_id,
        idempotency_key,
        &payload_hash,
        proof_hex,
        "expense.payment",
        Some(shift),
    )
    .await?;

    if let Some((old_shift, old_amount, old_key, old_hash)) = sqlx::query_as::<_, (String, i64, String, Vec<u8>)>(
        "SELECT shift_id::text,amount_minor,idempotency_key,request_hash FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_expense_id=$2::uuid",
    )
    .bind(tenant)
    .bind(expense_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| DrawerError::Database)?
    {
        if old_shift != shift || old_amount != amount_minor || old_key != idempotency_key || old_hash != payload_hash {
            return Err(DrawerError::IdempotencyConflict);
        }
        consume_challenge(tx, challenge_id, &payload_hash).await?;
        return Ok(());
    }

    let open = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_drawer_shifts WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid AND status='open')")
        .bind(shift).bind(tenant).bind(branch).bind(station).fetch_one(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    if !open {
        return Err(DrawerError::NoOpenShift);
    }
    let movement_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_drawer_movements(id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,source_expense_id) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8::uuid,'paid_out',$9,$10,$11,$12,$13::uuid)")
        .bind(movement_id).bind(tenant).bind(branch).bind(station).bind(shift).bind(membership).bind(lease_id).bind(lease.activation_id).bind(amount_minor).bind(format!("Cash expense {expense_id}" )).bind(idempotency_key).bind(&payload_hash).bind(expense_id)
        .execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    link_source_outflow_to_register(
        tx,
        tenant,
        branch,
        station,
        membership,
        movement_id,
        "expense_outflow",
        expense_id,
        amount_minor,
    )
    .await?;
    consume_challenge(tx, challenge_id, &payload_hash).await?;
    audit(tx, tenant, membership, "drawer_movement_recorded", "drawer_movement", &movement_id.to_string(), serde_json::json!({"type":"paid_out","amountMinor":amount_minor,"sourceType":"expense","sourceId":expense_id.to_string()})).await?;
    Ok(())
}

pub(crate) async fn record_receivable_collection_inflow(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    membership: &str,
    tenant: &str,
    branch: &str,
    station: &str,
    shift: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    collection_id: Uuid,
    amount_minor: i64,
) -> Result<(), DrawerError> {
    let payload_hash = Sha256::digest(raw_body).to_vec();
    set_context(tx, identity, tenant, membership, branch, station).await?;
    lock_station(tx, station).await?;
    let lease = verify_operation(
        tx,
        identity,
        membership,
        tenant,
        branch,
        station,
        lease_id,
        lease_token,
        challenge_id,
        idempotency_key,
        &payload_hash,
        proof_hex,
        "receivable.collection",
        Some(shift),
    )
    .await?;

    if let Some((old_shift, old_amount, old_key, old_hash)) = sqlx::query_as::<_, (String, i64, String, Vec<u8>)>(
        "SELECT shift_id::text,amount_minor,idempotency_key,request_hash FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_receivable_collection_id=$2::uuid",
    )
    .bind(tenant)
    .bind(collection_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| DrawerError::Database)?
    {
        if old_shift != shift || old_amount != amount_minor || old_key != idempotency_key || old_hash != payload_hash {
            return Err(DrawerError::IdempotencyConflict);
        }
        consume_challenge(tx, challenge_id, &payload_hash).await?;
        return Ok(());
    }

    let open = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_drawer_shifts WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid AND status='open')")
        .bind(shift).bind(tenant).bind(branch).bind(station).fetch_one(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    if !open {
        return Err(DrawerError::NoOpenShift);
    }
    let movement_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_drawer_movements(id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,source_receivable_collection_id) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8::uuid,'paid_in',$9,$10,$11,$12,$13::uuid)")
        .bind(movement_id).bind(tenant).bind(branch).bind(station).bind(shift).bind(membership).bind(lease_id).bind(lease.activation_id).bind(amount_minor).bind(format!("Customer repayment {collection_id}" )).bind(idempotency_key).bind(&payload_hash).bind(collection_id)
        .execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    link_receivable_collection_to_register(
        tx,
        tenant,
        branch,
        station,
        membership,
        movement_id,
        collection_id,
        amount_minor,
    )
    .await?;
    consume_challenge(tx, challenge_id, &payload_hash).await?;
    audit(tx, tenant, membership, "drawer_movement_recorded", "drawer_movement", &movement_id.to_string(), serde_json::json!({"type":"paid_in","amountMinor":amount_minor,"sourceType":"receivable_collection","sourceId":collection_id.to_string()})).await?;
    Ok(())
}

async fn link_receivable_collection_to_register(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    station: &str,
    membership: &str,
    movement_id: Uuid,
    collection_id: Uuid,
    amount_minor: i64,
) -> Result<(), DrawerError> {
    let register = sqlx::query_as::<_, (Uuid, bool)>(
        "SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
         FROM hosted_cash_locations l WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.station_id=$3::uuid AND l.location_type='register' AND l.status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .bind(station)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| DrawerError::Database)?;
    if let Some((location_id, initialized)) = register {
        if !initialized {
            return Err(DrawerError::RegisterCutoverRequired);
        }
        sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_drawer_movement_id,source_receivable_collection_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'receivable_collection_inflow',$4,$5,$6,$7::uuid)")
            .bind(tenant).bind(branch).bind(location_id).bind(movement_id).bind(collection_id).bind(amount_minor).bind(membership)
            .execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    }
    Ok(())
}

async fn link_source_outflow_to_register(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    station: &str,
    membership: &str,
    movement_id: Uuid,
    event_type: &str,
    source_id: Uuid,
    amount_minor: i64,
) -> Result<(), DrawerError> {
    let register = sqlx::query_as::<_, (Uuid, bool)>(
        "SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
         FROM hosted_cash_locations l WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.station_id=$3::uuid AND l.location_type='register' AND l.status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .bind(station)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| DrawerError::Database)?;
    if let Some((location_id, initialized)) = register {
        if !initialized {
            return Err(DrawerError::RegisterCutoverRequired);
        }
        match event_type {
            "expense_outflow" => {
                sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_expense_id,source_drawer_movement_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,-$7,$8::uuid)")
                    .bind(tenant).bind(branch).bind(location_id).bind(event_type).bind(source_id).bind(movement_id).bind(amount_minor).bind(membership)
                    .execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
            }
            "supplier_payment_outflow" => {
                sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_supplier_payment_id,source_drawer_movement_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,-$7,$8::uuid)")
                    .bind(tenant).bind(branch).bind(location_id).bind(event_type).bind(source_id).bind(movement_id).bind(amount_minor).bind(membership)
                    .execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
            }
            _ => return Err(DrawerError::Database),
        }
    }
    Ok(())
}

pub(crate) async fn close(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    shift_id: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    input: CloseInput,
) -> Result<DrawerReport, DrawerError> {
    if input.counted_cash_minor < 0
        || input.counted_cash_minor > MAX_AMOUNT
        || !valid_key(idempotency_key)
        || Uuid::parse_str(shift_id).is_err()
    {
        return Err(DrawerError::InvalidInput);
    }
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| DrawerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    lock_station(&mut tx, station_id).await?;
    let _lease = verify_operation(
        &mut tx,
        identity_id,
        membership_id,
        tenant_id,
        branch_id,
        station_id,
        lease_id,
        lease_token,
        challenge_id,
        idempotency_key,
        &payload_hash,
        proof_hex,
        "drawer.close",
        Some(shift_id),
    )
    .await?;
    let closed = sqlx::query_as::<_, (String, Vec<u8>)>("SELECT close_idempotency_key,close_request_hash FROM hosted_drawer_shifts WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid AND status='closed'")
        .bind(shift_id).bind(tenant_id).bind(branch_id).bind(station_id).fetch_optional(&mut *tx).await.map_err(|_| DrawerError::Database)?;
    if let Some((old_key, old_hash)) = closed {
        if old_key != idempotency_key || old_hash != payload_hash {
            return Err(DrawerError::IdempotencyConflict);
        }
        consume_challenge(&mut tx, challenge_id, &payload_hash).await?;
        let result = report(&mut tx, tenant_id, branch_id, station_id, shift_id).await?;
        tx.commit().await.map_err(|_| DrawerError::Database)?;
        return Ok(result);
    }
    let mut result = report(&mut tx, tenant_id, branch_id, station_id, shift_id).await?;
    if result.is_closed {
        return Err(DrawerError::ShiftUnavailable);
    }
    let difference = input
        .counted_cash_minor
        .checked_sub(result.expected_cash_minor)
        .ok_or(DrawerError::AmountOverflow)?;
    let closed_at = sqlx::query_scalar::<_, String>(
        "SELECT to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| DrawerError::Database)?;
    result.closed_at = Some(closed_at.clone());
    result.counted_cash_minor = Some(input.counted_cash_minor);
    result.difference_minor = Some(difference);
    result.is_closed = true;
    let snapshot = serde_json::to_value(&result).map_err(|_| DrawerError::Database)?;
    let changed = sqlx::query("UPDATE hosted_drawer_shifts SET status='closed',closed_at=now(),closed_by_membership_id=$1::uuid,closing_lease_id=$2::uuid,counted_cash_minor=$3,expected_cash_minor=$4,difference_minor=$5,close_idempotency_key=$6,close_request_hash=$7,report_json=$8 WHERE id=$9::uuid AND tenant_id=$10::uuid AND status='open'")
        .bind(membership_id).bind(lease_id).bind(input.counted_cash_minor).bind(result.expected_cash_minor).bind(difference).bind(idempotency_key).bind(&payload_hash).bind(snapshot).bind(shift_id).bind(tenant_id)
        .execute(&mut *tx).await.map_err(|_| DrawerError::Database)?;
    if changed.rows_affected() != 1 {
        return Err(DrawerError::ShiftUnavailable);
    }
    consume_challenge(&mut tx, challenge_id, &payload_hash).await?;
    audit(&mut tx, tenant_id, membership_id, "drawer_shift_closed", "drawer_shift", shift_id, serde_json::json!({"expectedCashMinor":result.expected_cash_minor,"countedCashMinor":input.counted_cash_minor,"differenceMinor":difference})).await?;
    tx.commit().await.map_err(|_| DrawerError::Database)?;
    Ok(result)
}

async fn report(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    station: &str,
    shift: &str,
) -> Result<DrawerReport, DrawerError> {
    let row = sqlx::query_as::<_, (String, String, String, String, i64, String, Option<String>, Option<i64>, Option<i64>, Option<serde_json::Value>)>(
        "SELECT id::text,station_id::text,opened_by_membership_id::text,to_char(opened_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),opening_cash_minor,status,to_char(closed_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),counted_cash_minor,difference_minor,report_json FROM hosted_drawer_shifts WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid"
    ).bind(shift).bind(tenant).bind(branch).bind(station).fetch_optional(&mut **tx).await.map_err(|_| DrawerError::Database)?.ok_or(DrawerError::ShiftUnavailable)?;
    if row.5 == "closed" {
        return serde_json::from_value(row.9.ok_or(DrawerError::Database)?)
            .map_err(|_| DrawerError::Database);
    }
    let sales = sqlx::query_as::<_, (i64, i64)>("SELECT COALESCE(SUM(t.amount_minor),0)::bigint,COUNT(DISTINCT s.id)::bigint FROM hosted_sales s JOIN hosted_sale_tenders t ON t.tenant_id=s.tenant_id AND t.branch_id=s.branch_id AND t.sale_id=s.id WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid AND s.station_id=$3::uuid AND s.cash_shift_id=$4::uuid AND t.method_key_snapshot='cash'")
        .bind(tenant).bind(branch).bind(station).bind(shift).fetch_one(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    let movements = sqlx::query_as::<_, (i64, i64, i64, i64)>("SELECT COALESCE(SUM(amount_minor) FILTER (WHERE movement_type='paid_in'),0)::bigint,COALESCE(SUM(amount_minor) FILTER (WHERE movement_type='paid_out'),0)::bigint,COALESCE(SUM(amount_minor) FILTER (WHERE movement_type='cash_drop'),0)::bigint,COUNT(*)::bigint FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND shift_id=$2::uuid")
        .bind(tenant).bind(shift).fetch_one(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    let expected = row
        .4
        .checked_add(sales.0)
        .and_then(|v| v.checked_add(movements.0))
        .and_then(|v| v.checked_sub(movements.1))
        .and_then(|v| v.checked_sub(movements.2))
        .ok_or(DrawerError::AmountOverflow)?;
    Ok(DrawerReport {
        shift_id: row.0,
        station_id: row.1,
        opened_by_membership_id: row.2,
        opened_at: row.3,
        closed_at: row.6,
        opening_cash_minor: row.4,
        cash_sales_minor: sales.0,
        paid_in_minor: movements.0,
        paid_out_minor: movements.1,
        cash_drops_minor: movements.2,
        expected_cash_minor: expected,
        counted_cash_minor: row.7,
        difference_minor: row.8,
        movement_count: movements.3,
        sale_count: sales.1,
        is_closed: false,
    })
}

struct LeaseProof {
    activation_id: String,
}
async fn verify_operation(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    membership: &str,
    tenant: &str,
    branch: &str,
    station: &str,
    lease_id: &str,
    token: &str,
    challenge_id: &str,
    key: &str,
    payload_hash: &[u8],
    proof_hex: &str,
    operation: &str,
    resource: Option<&str>,
) -> Result<LeaseProof, DrawerError> {
    use sha2::Digest;
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();
    let lease = sqlx::query_as::<_, (String, Vec<u8>)>("SELECT l.activation_id::text,a.device_public_key FROM station_actor_leases l JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id AND k.station_id=l.station_id AND k.lease_id=l.id JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id AND a.station_id=l.station_id AND a.id=l.activation_id JOIN memberships m ON m.id=l.membership_id AND m.tenant_id=l.tenant_id JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id WHERE l.id=$1::uuid AND l.tenant_id=$2::uuid AND l.branch_id=$3::uuid AND l.station_id=$4::uuid AND l.membership_id=$5::uuid AND l.identity_id=$6::uuid AND l.lease_token_hash=$7 AND l.status='issued' AND l.expires_at>now() AND a.status='active' AND m.status='active' AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id) AND (g.station_id IS NULL OR g.station_id=l.station_id) LIMIT 1 FOR UPDATE OF l")
        .bind(lease_id).bind(tenant).bind(branch).bind(station).bind(membership).bind(identity).bind(token_hash).fetch_optional(&mut **tx).await.map_err(|_| DrawerError::Database)?.ok_or(DrawerError::LeaseUnavailable)?;
    let challenge = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>("SELECT nonce,payload_hash FROM station_financial_proof_challenges WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid AND membership_id=$5::uuid AND identity_id=$6::uuid AND activation_id=$7::uuid AND lease_id=$8::uuid AND operation=$9 AND idempotency_key=$10 AND consumed_at IS NULL AND expires_at>now() FOR UPDATE")
        .bind(challenge_id).bind(tenant).bind(branch).bind(station).bind(membership).bind(identity).bind(&lease.0).bind(lease_id).bind(operation).bind(key).fetch_optional(&mut **tx).await.map_err(|_| DrawerError::Database)?.ok_or(DrawerError::InvalidProof)?;
    if challenge.1 != payload_hash {
        return Err(DrawerError::InvalidProof);
    }
    let proof = decode_hex(proof_hex, 64).ok_or(DrawerError::InvalidProof)?;
    let signature = Signature::from_slice(&proof).map_err(|_| DrawerError::InvalidProof)?;
    let public_key: [u8; 32] = lease
        .1
        .as_slice()
        .try_into()
        .map_err(|_| DrawerError::InvalidProof)?;
    let verifying = VerifyingKey::from_bytes(&public_key).map_err(|_| DrawerError::InvalidProof)?;
    let path =
        crate::station_proof::financial_target_path(tenant, branch, station, operation, resource)
            .ok_or(DrawerError::InvalidProof)?;
    let message = format!("advanced-pos/financial-operation/v1\nPOST\n{path}\n{operation}\n{tenant}\n{branch}\n{station}\n{lease_id}\n{membership}\n{}\n{challenge_id}\n{key}\n{}\n{}",lease.0,encode_hex(payload_hash),encode_hex(&challenge.0));
    verifying
        .verify(message.as_bytes(), &signature)
        .map_err(|_| DrawerError::InvalidProof)?;
    Ok(LeaseProof {
        activation_id: lease.0,
    })
}

async fn consume_challenge(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    payload_hash: &[u8],
) -> Result<(), DrawerError> {
    let changed=sqlx::query("UPDATE station_financial_proof_challenges SET consumed_at=now() WHERE id=$1::uuid AND consumed_at IS NULL AND expires_at>now() AND payload_hash=$2")
        .bind(id).bind(payload_hash).execute(&mut **tx).await.map_err(|_| DrawerError::Database)?;
    if changed.rows_affected() != 1 {
        return Err(DrawerError::InvalidProof);
    }
    Ok(())
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    actor: &str,
    action: &str,
    kind: &str,
    id: &str,
    metadata: serde_json::Value,
) -> Result<(), DrawerError> {
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6)")
        .bind(tenant).bind(actor).bind(action).bind(kind).bind(id).bind(metadata).execute(&mut **tx).await.map_err(|_|DrawerError::Database)?;
    Ok(())
}
async fn lock_station(
    tx: &mut Transaction<'_, Postgres>,
    station: &str,
) -> Result<(), DrawerError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(station)
        .execute(&mut **tx)
        .await
        .map_err(|_| DrawerError::Database)?;
    Ok(())
}
async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    station: &str,
) -> Result<(), DrawerError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", branch),
        ("pos.station_id", station),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| DrawerError::Database)?;
    }
    Ok(())
}
fn valid_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}
fn decode_hex(value: &str, bytes: usize) -> Option<Vec<u8>> {
    if value.len() != bytes * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|p| Some(((p[0] as char).to_digit(16)? * 16 + (p[1] as char).to_digit(16)?) as u8))
        .collect()
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
