// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::inventory_allocations;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_LINES: usize = 100;
const MAX_QUANTITY: i64 = 1_000_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequestInput {
    pub source_branch_id: String,
    pub note: Option<String>,
    pub items: Vec<RequestLineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequestLineInput {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransitionInput {
    pub action: String,
    pub expected_revision: i64,
    pub note: Option<String>,
    #[serde(default)]
    pub items: Vec<TransitionLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransitionLineInput {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferHeader {
    pub transfer_id: String,
    pub source_branch_id: String,
    pub destination_branch_id: String,
    pub status: String,
    pub revision: i64,
    pub request_note: Option<String>,
    pub requested_by_membership_id: String,
    pub requested_at: String,
    pub accepted_by_membership_id: Option<String>,
    pub accepted_at: Option<String>,
    pub rejected_by_membership_id: Option<String>,
    pub rejected_at: Option<String>,
    pub rejection_note: Option<String>,
    pub dispatched_by_membership_id: Option<String>,
    pub dispatched_at: Option<String>,
    pub received_by_membership_id: Option<String>,
    pub received_at: Option<String>,
    pub receipt_note: Option<String>,
    pub cancelled_by_membership_id: Option<String>,
    pub cancelled_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferLine {
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub requested_quantity: i64,
    pub accepted_quantity: Option<i64>,
    pub dispatched_quantity: Option<i64>,
    pub received_quantity: Option<i64>,
    pub discrepancy_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StockTransfer {
    #[serde(flatten)]
    pub header: TransferHeader,
    pub items: Vec<TransferLine>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferError {
    InvalidInput,
    NotFound,
    Conflict,
    IdempotencyConflict,
    InsufficientStock,
    Database,
}

fn database_error(stage: &'static str, error: sqlx::Error) -> TransferError {
    #[cfg(test)]
    eprintln!("stock transfer {stage} failed: {error}");
    #[cfg(not(test))]
    let _ = (stage, error);
    TransferError::Database
}

pub(crate) async fn request(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    destination_branch_id: &str,
    station_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: RequestInput,
) -> Result<StockTransfer, TransferError> {
    let input = validate_request(input, idempotency_key)?;
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| database_error("begin request", error))?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        destination_branch_id,
        Some(station_id),
    )
    .await?;

    let lock_key = format!("{tenant_id}:{destination_branch_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("request idempotency lock", error))?;

    if let Some((transfer_id, old_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text, request_hash FROM stock_transfers
         WHERE tenant_id=$1::uuid AND destination_branch_id=$2::uuid
           AND requested_by_membership_id=$3::uuid AND request_idempotency_key=$4",
    )
    .bind(tenant_id)
    .bind(destination_branch_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| database_error("request replay lookup", error))?
    {
        if old_hash != payload_hash {
            return Err(TransferError::IdempotencyConflict);
        }
        let mut result = get_transfer(&mut tx, tenant_id, &transfer_id).await?;
        result.replayed = true;
        tx.commit()
            .await
            .map_err(|error| database_error("commit replay", error))?;
        return Ok(result);
    }

    // Availability scope is read-only and transaction-local; it is needed to
    // confirm the requested source branch and canonical product assignments.
    sqlx::query("SELECT set_config('pos.inventory_availability', 'enabled', true)")
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("enable availability scope", error))?;
    let source_is_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active')",
    )
    .bind(tenant_id)
    .bind(&input.source_branch_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| database_error("source branch lookup", error))?;
    if !source_is_active || input.source_branch_id == destination_branch_id {
        return Err(TransferError::NotFound);
    }

    let transfer_id = Uuid::new_v4();
    let idempotency_hash: [u8; 32] = payload_hash
        .as_slice()
        .try_into()
        .map_err(|_| TransferError::Database)?;
    sqlx::query(
        "INSERT INTO stock_transfers
            (id, tenant_id, source_branch_id, destination_branch_id, status, revision,
             request_idempotency_key, request_hash, request_note, requested_by_membership_id)
         VALUES ($1, $2::uuid, $3::uuid, $4::uuid, 'requested', 1, $5, $6, $7, $8::uuid)",
    )
    .bind(transfer_id)
    .bind(tenant_id)
    .bind(&input.source_branch_id)
    .bind(destination_branch_id)
    .bind(idempotency_key)
    .bind(idempotency_hash.as_slice())
    .bind(&input.note)
    .bind(membership_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|db| db.is_unique_violation())
        {
            TransferError::Conflict
        } else {
            database_error("insert transfer header", error)
        }
    })?;

    for item in &input.items {
        let Some((sku, name)) = sqlx::query_as::<_, (String, String)>(
            "SELECT p.sku, p.name
             FROM catalog_products p
             JOIN branch_catalog_items src ON src.tenant_id=p.tenant_id AND src.product_id=p.id
             JOIN branch_catalog_items dst ON dst.tenant_id=p.tenant_id AND dst.product_id=p.id
             WHERE p.tenant_id=$1::uuid AND p.id=$2::uuid AND p.status='active'
               AND src.branch_id=$3::uuid AND src.status='active'
               AND dst.branch_id=$4::uuid AND dst.status='active'",
        )
        .bind(tenant_id)
        .bind(&item.product_id)
        .bind(&input.source_branch_id)
        .bind(destination_branch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| database_error("product mapping lookup", error))?
        else {
            return Err(TransferError::NotFound);
        };

        sqlx::query(
            "INSERT INTO stock_transfer_lines
                (tenant_id, transfer_id, product_id, sku_snapshot, name_snapshot, requested_quantity)
             VALUES ($1::uuid, $2, $3::uuid, $4, $5, $6)",
        )
        .bind(tenant_id)
        .bind(transfer_id)
        .bind(&item.product_id)
        .bind(sku)
        .bind(name)
        .bind(item.quantity)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("insert transfer line", error))?;
    }

    sqlx::query(
        "INSERT INTO stock_transfer_events
            (tenant_id, transfer_id, acting_branch_id, actor_membership_id, event_type,
             revision, idempotency_key, request_hash, details)
         VALUES ($1::uuid, $2, $3::uuid, $4::uuid, 'requested', 1, $5, $6, $7)",
    )
    .bind(tenant_id)
    .bind(transfer_id)
    .bind(destination_branch_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .bind(idempotency_hash.as_slice())
    .bind(serde_json::json!({
        "sourceBranchId": input.source_branch_id,
        "itemCount": input.items.len(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|error| database_error("insert transfer event", error))?;

    let result = get_transfer(&mut tx, tenant_id, &transfer_id.to_string()).await?;
    tx.commit()
        .await
        .map_err(|error| database_error("commit request", error))?;
    Ok(result)
}

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<Vec<StockTransfer>, TransferError> {
    let mut tx = pool.begin().await.map_err(|_| TransferError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id::text FROM stock_transfers
         WHERE tenant_id=$1::uuid
           AND (source_branch_id=$2::uuid OR destination_branch_id=$2::uuid)
         ORDER BY requested_at DESC, id DESC LIMIT 100",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| TransferError::Database)?;
    let mut transfers = Vec::with_capacity(ids.len());
    for id in ids {
        transfers.push(get_transfer(&mut tx, tenant_id, &id).await?);
    }
    tx.commit().await.map_err(|_| TransferError::Database)?;
    Ok(transfers)
}

pub(crate) async fn transition(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    transfer_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: TransitionInput,
) -> Result<StockTransfer, TransferError> {
    let input = validate_transition(input, idempotency_key)?;
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| database_error("begin transition", error))?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;

    let lock_key = format!("{tenant_id}:{transfer_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("transition idempotency lock", error))?;
    if let Some(old_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM stock_transfer_events WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    ).bind(tenant_id).bind(transfer_id).bind(membership_id).bind(idempotency_key)
        .fetch_optional(&mut *tx).await
        .map_err(|error| database_error("transition replay lookup", error))? {
        if old_hash != payload_hash { return Err(TransferError::IdempotencyConflict); }
        let mut result = get_transfer(&mut tx, tenant_id, transfer_id).await?;
        result.replayed = true;
        tx.commit().await.map_err(|error| database_error("commit transition replay", error))?;
        return Ok(result);
    }

    let (source_branch, destination_branch, initial_status) = sqlx::query_as::<_, (String, String, String)>(
        "SELECT source_branch_id::text, destination_branch_id::text, status FROM stock_transfers WHERE tenant_id=$1::uuid AND id=$2::uuid",
    ).bind(tenant_id).bind(transfer_id).fetch_optional(&mut *tx).await
        .map_err(|error| database_error("read transfer before inventory lock", error))?.ok_or(TransferError::NotFound)?;
    let (required_branch, allowed_statuses): (&str, &[&str]) = match input.action.as_str() {
        "accept" | "reject" => (source_branch.as_str(), &["requested"]),
        "dispatch" => (source_branch.as_str(), &["accepted"]),
        "receive" => (
            destination_branch.as_str(),
            &["in_transit", "partially_received", "discrepancy"],
        ),
        "resolve_discrepancy" => (
            destination_branch.as_str(),
            &["partially_received", "discrepancy"],
        ),
        _ => return Err(TransferError::InvalidInput),
    };
    if branch_id != required_branch {
        return Err(TransferError::NotFound);
    }
    if !allowed_statuses.contains(&initial_status.as_str()) {
        return Err(TransferError::Conflict);
    }

    let mut product_ids = sqlx::query_scalar::<_, String>(
        "SELECT product_id::text FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid ORDER BY product_id",
    )
    .bind(tenant_id)
    .bind(transfer_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|error| database_error("load transfer product keys", error))?;
    let mut requested_product_ids = input
        .items
        .iter()
        .map(|line| Uuid::parse_str(&line.product_id).map(|id| id.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransferError::InvalidInput)?;
    product_ids.sort_unstable();
    requested_product_ids.sort_unstable();
    if matches!(input.action.as_str(), "accept" | "dispatch" | "receive")
        && product_ids != requested_product_ids
    {
        return Err(TransferError::InvalidInput);
    }
    if matches!(input.action.as_str(), "reject" | "resolve_discrepancy")
        && !requested_product_ids.is_empty()
    {
        return Err(TransferError::InvalidInput);
    }
    if !matches!(input.action.as_str(), "reject" | "resolve_discrepancy") {
        let inventory_branch = if input.action == "receive" {
            &destination_branch
        } else {
            &source_branch
        };
        inventory_allocations::lock_products(&mut tx, tenant_id, inventory_branch, &product_ids)
            .await
            .map_err(|error| database_error("lock transfer inventory", error))?;
    }

    let (source_branch, destination_branch, status, revision) = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT source_branch_id::text, destination_branch_id::text, status, revision FROM stock_transfers WHERE tenant_id=$1::uuid AND id=$2::uuid FOR UPDATE",
    ).bind(tenant_id).bind(transfer_id).fetch_optional(&mut *tx).await
        .map_err(|error| database_error("lock transfer after inventory", error))?.ok_or(TransferError::NotFound)?;
    if input.expected_revision != revision {
        return Err(TransferError::Conflict);
    }
    let (required_branch, allowed_statuses): (&str, &[&str]) = match input.action.as_str() {
        "accept" | "reject" => (source_branch.as_str(), &["requested"]),
        "dispatch" => (source_branch.as_str(), &["accepted"]),
        "receive" => (
            destination_branch.as_str(),
            &["in_transit", "partially_received", "discrepancy"],
        ),
        "resolve_discrepancy" => (
            destination_branch.as_str(),
            &["partially_received", "discrepancy"],
        ),
        _ => return Err(TransferError::InvalidInput),
    };
    if branch_id != required_branch {
        return Err(TransferError::NotFound);
    }
    if !allowed_statuses.contains(&status.as_str()) {
        return Err(TransferError::Conflict);
    }

    let mut details = serde_json::json!({
        "action": input.action.clone(),
        "items": input.items.iter().map(|line| serde_json::json!({
            "productId": line.product_id.clone(),
            "quantity": line.quantity,
        })).collect::<Vec<_>>(),
    });
    if let Some(note) = &input.note {
        details["note"] = serde_json::json!(note);
    }
    match input.action.as_str() {
        "accept" => {
            let product_ids = sqlx::query_scalar::<_, String>("SELECT product_id::text FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).fetch_all(&mut *tx).await
                .map_err(|error| database_error("load acceptance lines", error))?;
            if product_ids.len() != input.items.len() {
                return Err(TransferError::InvalidInput);
            }
            for line in &input.items {
                let on_hand = sqlx::query_scalar::<_, i64>(
                    "SELECT stock_quantity FROM branch_catalog_items
                     WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND status='active'
                     FOR UPDATE",
                )
                .bind(tenant_id)
                .bind(&source_branch)
                .bind(&line.product_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| database_error("load source stock for acceptance", error))?
                .ok_or(TransferError::NotFound)?;
                let committed = inventory_allocations::allocated_quantity(
                    &mut tx,
                    tenant_id,
                    &source_branch,
                    &line.product_id,
                    None,
                )
                .await
                .map_err(|error| database_error("read source commitments for acceptance", error))?;
                if !inventory_allocations::can_consume(on_hand, committed, line.quantity) {
                    return Err(TransferError::InsufficientStock);
                }
                let result = sqlx::query("UPDATE stock_transfer_lines SET accepted_quantity=$4 WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid AND product_id=$3::uuid AND accepted_quantity IS NULL AND requested_quantity >= $4")
                    .bind(tenant_id).bind(transfer_id).bind(&line.product_id).bind(line.quantity)
                    .execute(&mut *tx).await.map_err(|error| database_error("save accepted quantity", error))?;
                if result.rows_affected() != 1 {
                    return Err(TransferError::InvalidInput);
                }
                sqlx::query(
                    "INSERT INTO inventory_allocations
                        (tenant_id,branch_id,product_id,stock_transfer_id,quantity)
                     VALUES ($1::uuid,$2::uuid,$3::uuid,$4::uuid,$5)",
                )
                .bind(tenant_id)
                .bind(&source_branch)
                .bind(&line.product_id)
                .bind(transfer_id)
                .bind(line.quantity)
                .execute(&mut *tx)
                .await
                .map_err(|error| database_error("allocate accepted transfer stock", error))?;
            }
        }
        "reject" => {}
        "dispatch" => {
            let product_ids = sqlx::query_scalar::<_, String>("SELECT product_id::text FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).fetch_all(&mut *tx).await
                .map_err(|error| database_error("load dispatch lines", error))?;
            if product_ids.len() != input.items.len() {
                return Err(TransferError::InvalidInput);
            }
            for line in &input.items {
                let (accepted, name) = sqlx::query_as::<_, (i64, String)>("SELECT accepted_quantity,name_snapshot FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid AND product_id=$3::uuid")
                    .bind(tenant_id).bind(transfer_id).bind(&line.product_id).fetch_optional(&mut *tx).await
                    .map_err(|error| database_error("load dispatch line", error))?.ok_or(TransferError::InvalidInput)?;
                if line.quantity <= 0 || line.quantity > accepted {
                    return Err(TransferError::InvalidInput);
                }
                if line.quantity < accepted && input.note.is_none() {
                    return Err(TransferError::InvalidInput);
                }
                let release_reason = if line.quantity < accepted {
                    format!(
                        "Partial dispatch: {}",
                        input.note.as_deref().unwrap_or_default()
                    )
                } else {
                    "Transfer dispatched".to_owned()
                };
                let released = sqlx::query(
                    "UPDATE inventory_allocations
                     SET released_at=clock_timestamp(),release_reason=$5
                     WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND stock_transfer_id=$3::uuid
                       AND product_id=$4::uuid AND released_at IS NULL",
                )
                .bind(tenant_id)
                .bind(branch_id)
                .bind(transfer_id)
                .bind(&line.product_id)
                .bind(release_reason)
                .execute(&mut *tx)
                .await
                .map_err(|error| database_error("release dispatched transfer allocation", error))?;
                if released.rows_affected() != 1 {
                    return Err(TransferError::Conflict);
                }
                let on_hand = sqlx::query_scalar::<_, i64>(
                    "SELECT stock_quantity FROM branch_catalog_items
                     WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND status='active'
                     FOR UPDATE",
                )
                .bind(tenant_id)
                .bind(branch_id)
                .bind(&line.product_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| database_error("load stock for dispatch", error))?
                .ok_or(TransferError::NotFound)?;
                let other_commitments = inventory_allocations::allocated_quantity(
                    &mut tx,
                    tenant_id,
                    branch_id,
                    &line.product_id,
                    None,
                )
                .await
                .map_err(|error| database_error("read commitments for dispatch", error))?;
                if !inventory_allocations::can_consume(on_hand, other_commitments, line.quantity) {
                    return Err(TransferError::InsufficientStock);
                }
                let resulting = sqlx::query_scalar::<_, i64>("UPDATE branch_catalog_items SET stock_quantity=stock_quantity-$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND status='active' AND stock_quantity >= $4 RETURNING stock_quantity")
                    .bind(tenant_id).bind(branch_id).bind(&line.product_id).bind(line.quantity).fetch_optional(&mut *tx).await
                    .map_err(|error| database_error("decrement dispatch stock", error))?.ok_or(TransferError::InsufficientStock)?;
                let movement_key = movement_idempotency_key(
                    transfer_id,
                    membership_id,
                    idempotency_key,
                    &line.product_id,
                );
                sqlx::query("INSERT INTO stock_movements (tenant_id,branch_id,product_id,actor_membership_id,movement_kind,quantity_delta,note,source_id,idempotency_key,resulting_stock_quantity) VALUES ($1::uuid,$2::uuid,$3::uuid,$4::uuid,'transfer_out',$5,$6,$7::uuid,$8,$9)")
                    .bind(tenant_id).bind(branch_id).bind(&line.product_id).bind(membership_id).bind(-line.quantity)
                    .bind(format!("Transfer {transfer_id}: {name}")).bind(transfer_id).bind(movement_key).bind(resulting)
                    .execute(&mut *tx).await.map_err(|error| database_error("record transfer-out movement", error))?;
                let updated = sqlx::query("UPDATE stock_transfer_lines SET dispatched_quantity=$4 WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid AND product_id=$3::uuid AND dispatched_quantity IS NULL")
                    .bind(tenant_id).bind(transfer_id).bind(&line.product_id).bind(line.quantity).execute(&mut *tx).await
                    .map_err(|error| database_error("save dispatched quantity", error))?;
                if updated.rows_affected() != 1 {
                    return Err(TransferError::Conflict);
                }
            }
        }
        "receive" => {
            let product_ids = sqlx::query_scalar::<_, String>("SELECT product_id::text FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).fetch_all(&mut *tx).await
                .map_err(|error| database_error("load receipt lines", error))?;
            if product_ids.len() != input.items.len() {
                return Err(TransferError::InvalidInput);
            }
            let mut received_any = false;
            for line in &input.items {
                let (dispatched, previous_received, name) = sqlx::query_as::<_, (i64, Option<i64>, String)>("SELECT dispatched_quantity,received_quantity,name_snapshot FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid AND product_id=$3::uuid")
                    .bind(tenant_id).bind(transfer_id).bind(&line.product_id).fetch_optional(&mut *tx).await
                    .map_err(|error| database_error("load receipt line", error))?.ok_or(TransferError::InvalidInput)?;
                let previous_received = previous_received.unwrap_or(0);
                let remaining = dispatched - previous_received;
                if line.quantity > remaining {
                    return Err(TransferError::InvalidInput);
                }
                received_any |= line.quantity > 0;
                if line.quantity > 0 {
                    let resulting = sqlx::query_scalar::<_, i64>("UPDATE branch_catalog_items SET stock_quantity=stock_quantity+$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND status='active' AND stock_quantity <= 9223372036854775807-$4 RETURNING stock_quantity")
                        .bind(tenant_id).bind(branch_id).bind(&line.product_id).bind(line.quantity).fetch_optional(&mut *tx).await
                        .map_err(|error| database_error("increment received stock", error))?.ok_or(TransferError::Conflict)?;
                    let movement_key = movement_idempotency_key(
                        transfer_id,
                        membership_id,
                        idempotency_key,
                        &line.product_id,
                    );
                    sqlx::query("INSERT INTO stock_movements (tenant_id,branch_id,product_id,actor_membership_id,movement_kind,quantity_delta,note,source_id,idempotency_key,resulting_stock_quantity) VALUES ($1::uuid,$2::uuid,$3::uuid,$4::uuid,'transfer_in',$5,$6,$7::uuid,$8,$9)")
                        .bind(tenant_id).bind(branch_id).bind(&line.product_id).bind(membership_id).bind(line.quantity)
                        .bind(format!("Transfer {transfer_id}: {name}")).bind(transfer_id).bind(movement_key).bind(resulting)
                        .execute(&mut *tx).await.map_err(|error| database_error("record transfer-in movement", error))?;
                }
                let received_total = previous_received + line.quantity;
                sqlx::query("UPDATE stock_transfer_lines SET received_quantity=$4,discrepancy_note=NULL WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid AND product_id=$3::uuid")
                    .bind(tenant_id).bind(transfer_id).bind(&line.product_id).bind(received_total)
                    .execute(&mut *tx).await.map_err(|error| database_error("save received quantity", error))?;
            }
            if !received_any {
                return Err(TransferError::InvalidInput);
            }
        }
        "resolve_discrepancy" => {
            let unresolved = sqlx::query_scalar::<_, bool>("SELECT bool_or(COALESCE(received_quantity,0) < dispatched_quantity) FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).fetch_one(&mut *tx).await
                .map_err(|error| database_error("check unresolved transfer discrepancy", error))?;
            if !unresolved {
                return Err(TransferError::Conflict);
            }
            sqlx::query("UPDATE stock_transfer_lines SET discrepancy_note=CASE WHEN COALESCE(received_quantity,0) < dispatched_quantity THEN $3 ELSE NULL END WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).bind(&input.note)
                .execute(&mut *tx).await.map_err(|error| database_error("save transfer discrepancy resolution", error))?;
        }
        _ => return Err(TransferError::InvalidInput),
    }

    let next_status = match input.action.as_str() {
        "accept" => "accepted",
        "reject" => "rejected",
        "dispatch" => "in_transit",
        "receive" => {
            let all_equal = sqlx::query_scalar::<_, bool>("SELECT bool_and(received_quantity=dispatched_quantity) FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).fetch_one(&mut *tx).await
                .map_err(|error| database_error("evaluate receipt", error))?;
            if all_equal {
                "received"
            } else if input.note.is_some() {
                "discrepancy"
            } else {
                "partially_received"
            }
        }
        "resolve_discrepancy" => "discrepancy_resolved",
        _ => return Err(TransferError::InvalidInput),
    };
    if input.action == "receive" {
        if next_status == "discrepancy" {
            sqlx::query("UPDATE stock_transfer_lines SET discrepancy_note=CASE WHEN received_quantity < dispatched_quantity THEN $3 ELSE NULL END WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid")
                .bind(tenant_id).bind(transfer_id).bind(&input.note)
                .execute(&mut *tx).await.map_err(|error| database_error("record receipt discrepancy details", error))?;
        }
        details["hasDiscrepancy"] = serde_json::json!(next_status == "discrepancy");
    }
    let event_type = match (input.action.as_str(), next_status) {
        ("accept", _) => "accepted",
        ("reject", _) => "rejected",
        ("dispatch", _) => "dispatched",
        ("receive", "partially_received") => "partially_received",
        ("receive", "discrepancy") => "discrepancy",
        ("receive", _) => "received",
        ("resolve_discrepancy", _) => "discrepancy_resolved",
        _ => return Err(TransferError::InvalidInput),
    };
    let changed = sqlx::query("UPDATE stock_transfers SET status=$4,revision=revision+1,accepted_by_membership_id=CASE WHEN $4='accepted' THEN $3::uuid ELSE accepted_by_membership_id END,accepted_at=CASE WHEN $4='accepted' THEN now() ELSE accepted_at END,rejected_by_membership_id=CASE WHEN $4='rejected' THEN $3::uuid ELSE rejected_by_membership_id END,rejected_at=CASE WHEN $4='rejected' THEN now() ELSE rejected_at END,rejection_note=CASE WHEN $4='rejected' THEN $6 ELSE rejection_note END,dispatched_by_membership_id=CASE WHEN $4='in_transit' THEN $3::uuid ELSE dispatched_by_membership_id END,dispatched_at=CASE WHEN $4='in_transit' THEN now() ELSE dispatched_at END,received_by_membership_id=CASE WHEN $4 IN ('received','discrepancy','discrepancy_resolved') THEN $3::uuid WHEN $4='partially_received' THEN NULL ELSE received_by_membership_id END,received_at=CASE WHEN $4 IN ('received','discrepancy','discrepancy_resolved') THEN now() WHEN $4='partially_received' THEN NULL ELSE received_at END,receipt_note=CASE WHEN $4 IN ('received','discrepancy','discrepancy_resolved') THEN $6 WHEN $4='partially_received' THEN NULL ELSE receipt_note END,updated_at=now() WHERE tenant_id=$1::uuid AND id=$2::uuid AND revision=$5 AND status=$7")
        .bind(tenant_id).bind(transfer_id).bind(membership_id).bind(next_status).bind(revision).bind(&input.note).bind(&status)
        .execute(&mut *tx).await.map_err(|error| database_error("update transfer state", error))?;
    if changed.rows_affected() != 1 {
        return Err(TransferError::Conflict);
    }
    let event_hash: [u8; 32] = payload_hash
        .as_slice()
        .try_into()
        .map_err(|_| TransferError::Database)?;
    sqlx::query("INSERT INTO stock_transfer_events (tenant_id,transfer_id,acting_branch_id,actor_membership_id,event_type,revision,idempotency_key,request_hash,details) VALUES ($1::uuid,$2::uuid,$3::uuid,$4::uuid,$5,$6,$7,$8,$9)")
        .bind(tenant_id).bind(transfer_id).bind(branch_id).bind(membership_id).bind(event_type).bind(revision+1).bind(idempotency_key).bind(event_hash.as_slice()).bind(details)
        .execute(&mut *tx).await.map_err(|error| database_error("append transfer event", error))?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,$3,'stock_transfer',$4::uuid,$5)")
        .bind(tenant_id).bind(membership_id).bind(format!("stock_transfer_{event_type}")).bind(transfer_id)
        .bind(serde_json::json!({"revision": revision+1, "branchId": branch_id}))
        .execute(&mut *tx).await.map_err(|error| database_error("audit transfer event", error))?;
    let result = get_transfer(&mut tx, tenant_id, transfer_id).await?;
    tx.commit()
        .await
        .map_err(|error| database_error("commit transfer transition", error))?;
    Ok(result)
}

fn validate_transition(
    input: TransitionInput,
    idempotency_key: &str,
) -> Result<TransitionInput, TransferError> {
    if input.expected_revision <= 0
        || idempotency_key.is_empty()
        || idempotency_key.len() > 128
        || !idempotency_key
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
        || ![
            "accept",
            "reject",
            "dispatch",
            "receive",
            "resolve_discrepancy",
        ]
        .contains(&input.action.as_str())
    {
        return Err(TransferError::InvalidInput);
    }
    let note = input
        .note
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if note
        .as_ref()
        .is_some_and(|value| value.chars().count() > 500)
    {
        return Err(TransferError::InvalidInput);
    }
    if input.action == "reject" && note.is_none() {
        return Err(TransferError::InvalidInput);
    }
    if input.action == "resolve_discrepancy" && note.is_none() {
        return Err(TransferError::InvalidInput);
    }
    let mut ids = std::collections::HashSet::new();
    for line in &input.items {
        if Uuid::parse_str(&line.product_id).is_err()
            || line.quantity < 0
            || line.quantity > MAX_QUANTITY
            || !ids.insert(line.product_id.to_ascii_lowercase())
            || (input.action != "receive" && line.quantity == 0)
        {
            return Err(TransferError::InvalidInput);
        }
    }
    let needs_lines = matches!(input.action.as_str(), "accept" | "dispatch" | "receive");
    if needs_lines != !input.items.is_empty() {
        return Err(TransferError::InvalidInput);
    }
    if input.action == "receive" && !input.items.iter().any(|line| line.quantity > 0) {
        return Err(TransferError::InvalidInput);
    }
    Ok(TransitionInput { note, ..input })
}

fn movement_idempotency_key(
    transfer_id: &str,
    membership_id: &str,
    key: &str,
    product_id: &str,
) -> String {
    let value = format!("{transfer_id}:{membership_id}:{key}:{product_id}");
    let digest = Sha256::digest(value.as_bytes());
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("transfer-{encoded}")
}

fn validate_request(
    input: RequestInput,
    idempotency_key: &str,
) -> Result<RequestInput, TransferError> {
    if Uuid::parse_str(&input.source_branch_id).is_err()
        || idempotency_key.is_empty()
        || idempotency_key.len() > 128
        || !idempotency_key
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
        || input.items.is_empty()
        || input.items.len() > MAX_LINES
    {
        return Err(TransferError::InvalidInput);
    }
    let note = input
        .note
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if note
        .as_ref()
        .is_some_and(|value| value.chars().count() > 500)
    {
        return Err(TransferError::InvalidInput);
    }
    let mut product_ids = std::collections::HashSet::with_capacity(input.items.len());
    for line in &input.items {
        if Uuid::parse_str(&line.product_id).is_err()
            || line.quantity <= 0
            || line.quantity > MAX_QUANTITY
            || !product_ids.insert(line.product_id.to_ascii_lowercase())
        {
            return Err(TransferError::InvalidInput);
        }
    }
    Ok(RequestInput { note, ..input })
}

async fn get_transfer(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    transfer_id: &str,
) -> Result<StockTransfer, TransferError> {
    let header = sqlx::query_as::<_, TransferHeader>(
        "SELECT id::text AS transfer_id, source_branch_id::text, destination_branch_id::text,
                status, revision, request_note, requested_by_membership_id::text,
                to_char(requested_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS requested_at,
                accepted_by_membership_id::text,
                CASE WHEN accepted_at IS NULL THEN NULL ELSE to_char(accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') END AS accepted_at,
                rejected_by_membership_id::text,
                CASE WHEN rejected_at IS NULL THEN NULL ELSE to_char(rejected_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') END AS rejected_at,
                rejection_note, dispatched_by_membership_id::text,
                CASE WHEN dispatched_at IS NULL THEN NULL ELSE to_char(dispatched_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') END AS dispatched_at,
                received_by_membership_id::text,
                CASE WHEN received_at IS NULL THEN NULL ELSE to_char(received_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') END AS received_at,
                receipt_note, cancelled_by_membership_id::text,
                CASE WHEN cancelled_at IS NULL THEN NULL ELSE to_char(cancelled_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') END AS cancelled_at
         FROM stock_transfers WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant_id)
    .bind(transfer_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| database_error("load transfer header", error))?
    .ok_or(TransferError::NotFound)?;
    let items = sqlx::query_as::<_, TransferLine>(
        "SELECT product_id::text, sku_snapshot AS sku, name_snapshot AS name,
                requested_quantity, accepted_quantity, dispatched_quantity,
                received_quantity, discrepancy_note
         FROM stock_transfer_lines WHERE tenant_id=$1::uuid AND transfer_id=$2::uuid
         ORDER BY lower(name_snapshot), product_id",
    )
    .bind(tenant_id)
    .bind(transfer_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| database_error("load transfer lines", error))?;
    Ok(StockTransfer {
        header,
        items,
        replayed: false,
    })
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<(), TransferError> {
    sqlx::query("SELECT set_config('pos.identity_id', $1, true), set_config('pos.tenant_id', $2, true), set_config('pos.membership_id', $3, true), set_config('pos.branch_id', $4, true), set_config('pos.station_id', $5, true)")
        .bind(identity_id)
        .bind(tenant_id)
        .bind(membership_id)
        .bind(branch_id)
        .bind(station_id.unwrap_or(""))
        .execute(&mut **tx)
        .await
        .map_err(|error| database_error("set transfer request context", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        movement_idempotency_key, validate_request, validate_transition, RequestInput,
        RequestLineInput, TransferError, TransitionInput, TransitionLineInput,
    };

    fn valid_input() -> RequestInput {
        RequestInput {
            source_branch_id: "50000000-0000-0000-0000-000000000005".to_owned(),
            note: Some(" restock ".to_owned()),
            items: vec![RequestLineInput {
                product_id: "80000000-0000-0000-0000-000000000008".to_owned(),
                quantity: 4,
            }],
        }
    }

    #[test]
    fn request_validation_normalizes_notes_and_accepts_unique_positive_items() {
        let normalized = validate_request(valid_input(), "req-1").unwrap();
        assert_eq!(normalized.note.as_deref(), Some("restock"));
        assert_eq!(normalized.items[0].quantity, 4);
    }

    #[test]
    fn request_validation_rejects_duplicate_products_invalid_ids_and_bad_keys() {
        let mut input = valid_input();
        input.items.push(RequestLineInput {
            product_id: input.items[0].product_id.clone(),
            quantity: 1,
        });
        assert!(matches!(
            validate_request(input, "req-1"),
            Err(TransferError::InvalidInput)
        ));
        assert!(matches!(
            validate_request(valid_input(), "bad\nkey"),
            Err(TransferError::InvalidInput)
        ));
        let mut input = valid_input();
        input.items[0].quantity = 0;
        assert!(matches!(
            validate_request(input, "req-1"),
            Err(TransferError::InvalidInput)
        ));
    }

    fn transition(action: &str, quantity: i64, note: Option<&str>) -> TransitionInput {
        TransitionInput {
            action: action.to_owned(),
            expected_revision: 3,
            note: note.map(str::to_owned),
            items: if matches!(action, "reject" | "resolve_discrepancy") {
                Vec::new()
            } else {
                vec![TransitionLineInput {
                    product_id: "80000000-0000-0000-0000-000000000008".to_owned(),
                    quantity,
                }]
            },
        }
    }

    #[test]
    fn transfer_transition_validation_checks_incremental_receipts_and_resolution() {
        let accepted = validate_transition(transition("accept", 2, None), "accept-1").unwrap();
        assert_eq!(accepted.items[0].quantity, 2);
        assert!(validate_transition(transition("reject", 0, None), "reject-1").is_err());
        assert!(validate_transition(transition("receive", 0, None), "receive-1").is_err());
        assert!(validate_transition(transition("receive", 1, None), "receive-1").is_ok());
        assert!(
            validate_transition(transition("receive", 1, Some("delivery note")), "receive-2")
                .is_ok()
        );
        assert!(
            validate_transition(transition("resolve_discrepancy", 0, None), "resolve-1").is_err()
        );
        assert!(validate_transition(
            transition("resolve_discrepancy", 0, Some("supplier credit recorded")),
            "resolve-1"
        )
        .is_ok());
        let mut invalid_resolution =
            transition("resolve_discrepancy", 0, Some("supplier credit recorded"));
        invalid_resolution.items.push(TransitionLineInput {
            product_id: "80000000-0000-0000-0000-000000000008".to_owned(),
            quantity: 1,
        });
        assert!(validate_transition(invalid_resolution, "resolve-2").is_err());
        let mut stale = transition("accept", 2, None);
        stale.expected_revision = 0;
        assert!(validate_transition(stale, "accept-2").is_err());
    }

    #[test]
    fn transfer_movement_keys_are_deterministic_bounded_and_product_specific() {
        let first = movement_idempotency_key("transfer", "actor", &"k".repeat(128), "product-a");
        let replay = movement_idempotency_key("transfer", "actor", &"k".repeat(128), "product-a");
        let other_product =
            movement_idempotency_key("transfer", "actor", &"k".repeat(128), "product-b");
        assert_eq!(first, replay);
        assert_ne!(first, other_product);
        assert!(first.len() <= 80);
    }
}
