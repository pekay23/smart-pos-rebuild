// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_DRAFT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeldTicket {
    pub id: String,
    pub revision: i64,
    pub status: String,
    pub payload: Value,
    pub checkout_body: Option<Value>,
    pub checkout_key: Option<String>,
    pub claim_lease_id: Option<String>,
    pub completed_sale_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketError {
    InvalidInput,
    NotFound,
    Conflict,
    InvalidLease,
    Database,
}

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
) -> Result<Vec<HeldTicket>, TicketError> {
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        "",
        "",
    )
    .await?;
    let tickets = sqlx::query_as::<_, HeldTicket>(
        "SELECT id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,
                completed_sale_id::text,
                to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
         FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
           AND status IN ('held','checkout_pending')
         ORDER BY updated_at DESC,id LIMIT 100",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(tickets)
}

pub(crate) async fn create(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    payload: Value,
) -> Result<HeldTicket, TicketError> {
    validate_payload(&payload)?;
    if idempotency_key.is_empty() || idempotency_key.len() > 128 {
        return Err(TicketError::InvalidInput);
    }
    let fingerprint =
        Sha256::digest(serde_json::to_vec(&payload).map_err(|_| TicketError::InvalidInput)?)
            .to_vec();
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
        lease_id,
    )
    .await?;
    validate_active_lease(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        lease_id,
        lease_token,
    )
    .await?;
    if let Some(existing) = sqlx::query_as::<_, HeldTicket>(
        "SELECT id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,
                completed_sale_id::text,
                to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
         FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
           AND created_by=$3::uuid AND create_key=$4",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    {
        let previous = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT create_fingerprint FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(&existing.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| TicketError::Database)?;
        if previous != fingerprint {
            return Err(TicketError::Conflict);
        }
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(existing);
    }
    let ticket = sqlx::query_as::<_, HeldTicket>(
        "INSERT INTO branch_held_tickets(id,tenant_id,branch_id,origin_station_id,status,payload,
                create_key,create_fingerprint,claim_lease_id,claim_expires_at,created_by,updated_by)
         VALUES($1,$2::uuid,$3::uuid,$4::uuid,'held',$5,$6,$7,$8::uuid,
                (SELECT expires_at FROM station_actor_leases WHERE tenant_id=$2::uuid AND id=$8::uuid),$9::uuid,$9::uuid)
         RETURNING id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,
                completed_sale_id::text,
                to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(payload)
    .bind(idempotency_key)
    .bind(fingerprint)
    .bind(lease_id)
    .bind(membership_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(ticket)
}

pub(crate) async fn claim(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    ticket_id: &str,
    expected_revision: i64,
    lease_id: &str,
    lease_token: &str,
) -> Result<HeldTicket, TicketError> {
    if expected_revision < 1 {
        return Err(TicketError::InvalidInput);
    }
    let ticket_id = Uuid::parse_str(ticket_id).map_err(|_| TicketError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
        lease_id,
    )
    .await?;
    validate_active_lease(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        lease_id,
        lease_token,
    )
    .await?;
    let current = sqlx::query_as::<_, (i64, String, Option<String>, bool)>(
        "SELECT revision,status,claim_lease_id::text,COALESCE(claim_expires_at>clock_timestamp(),false) FROM branch_held_tickets
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    .ok_or(TicketError::NotFound)?;
    if !matches!(current.1.as_str(), "held" | "checkout_pending") {
        return Err(TicketError::Conflict);
    }
    if current.2.as_deref() == Some(lease_id) {
        let ticket = fetch_ticket(&mut tx, tenant_id, branch_id, ticket_id).await?;
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(ticket);
    }
    if current.0 != expected_revision {
        return Err(TicketError::Conflict);
    }
    if let Some(claim_id) = current.2.as_deref() {
        if claim_id != lease_id && current.3 {
            return Err(TicketError::Conflict);
        }
    }
    let ticket = sqlx::query_as::<_, HeldTicket>("UPDATE branch_held_tickets SET claim_lease_id=$6::uuid,claim_expires_at=(SELECT expires_at FROM station_actor_leases WHERE tenant_id=$1::uuid AND id=$6::uuid),updated_by=$5::uuid,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND revision=$4 RETURNING id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,completed_sale_id::text,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at")
        .bind(tenant_id).bind(branch_id).bind(ticket_id).bind(expected_revision).bind(membership_id).bind(lease_id)
        .fetch_optional(&mut *tx).await.map_err(|_|TicketError::Database)?.ok_or(TicketError::Conflict)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(ticket)
}

pub(crate) async fn save(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    ticket_id: &str,
    expected_revision: i64,
    lease_id: &str,
    lease_token: &str,
    payload: Value,
) -> Result<HeldTicket, TicketError> {
    validate_payload(&payload)?;
    if expected_revision < 1 {
        return Err(TicketError::InvalidInput);
    }
    let ticket_id = Uuid::parse_str(ticket_id).map_err(|_| TicketError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
        lease_id,
    )
    .await?;
    validate_active_lease(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        lease_id,
        lease_token,
    )
    .await?;
    let current = sqlx::query_as::<_, (i64, String, Option<Value>, Option<String>)>(
        "SELECT revision,status,payload,claim_lease_id::text FROM branch_held_tickets
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    .ok_or(TicketError::NotFound)?;
    if current.1 != "held" || current.3.as_deref() != Some(lease_id) {
        return Err(TicketError::Conflict);
    }
    if current.2.as_ref() == Some(&payload) {
        let ticket = fetch_ticket(&mut tx, tenant_id, branch_id, ticket_id).await?;
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(ticket);
    }
    if current.0 != expected_revision {
        return Err(TicketError::Conflict);
    }
    let ticket = sqlx::query_as::<_, HeldTicket>("UPDATE branch_held_tickets SET payload=$5,updated_by=$6::uuid,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND revision=$4 RETURNING id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,completed_sale_id::text,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at")
        .bind(tenant_id).bind(branch_id).bind(ticket_id).bind(expected_revision).bind(payload).bind(membership_id).fetch_optional(&mut *tx).await.map_err(|_|TicketError::Database)?.ok_or(TicketError::Conflict)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(ticket)
}

pub(crate) async fn prepare_checkout(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    ticket_id: &str,
    expected_revision: i64,
    lease_id: &str,
    lease_token: &str,
    body: Value,
    idempotency_key: &str,
) -> Result<HeldTicket, TicketError> {
    if expected_revision < 1 || idempotency_key.is_empty() || idempotency_key.len() > 128 {
        return Err(TicketError::InvalidInput);
    }
    let body_bytes = serde_json::to_vec(&body).map_err(|_| TicketError::InvalidInput)?;
    if body_bytes.len() > MAX_DRAFT_BYTES {
        return Err(TicketError::InvalidInput);
    }
    let fingerprint = Sha256::digest(&body_bytes).to_vec();
    let ticket_id = Uuid::parse_str(ticket_id).map_err(|_| TicketError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
        lease_id,
    )
    .await?;
    validate_active_lease(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        lease_id,
        lease_token,
    )
    .await?;
    let row = sqlx::query_as::<_, (i64, String, Option<Value>, Option<String>, Option<Vec<u8>>, Option<String>)>(
        "SELECT revision,status,checkout_body,checkout_key,checkout_fingerprint,claim_lease_id::text
         FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    .ok_or(TicketError::NotFound)?;
    let exact_attempt = row.2.as_ref() == Some(&body)
        && row.3.as_deref() == Some(idempotency_key)
        && row.4.as_deref() == Some(fingerprint.as_slice());
    if exact_attempt && row.1 == "completed" {
        let ticket = fetch_ticket(&mut tx, tenant_id, branch_id, ticket_id).await?;
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(ticket);
    }
    if exact_attempt && row.1 == "checkout_pending" && row.5.as_deref() == Some(lease_id) {
        // The prepare response may have been lost after the server committed it.
        // Replay the same immutable attempt even though its first request advanced revision.
        let ticket = fetch_ticket(&mut tx, tenant_id, branch_id, ticket_id).await?;
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(ticket);
    }
    if row.0 != expected_revision
        || row.1 != "held"
        || row.2.is_some()
        || row.5.as_deref() != Some(lease_id)
    {
        return Err(TicketError::Conflict);
    }
    let ticket = sqlx::query_as::<_, HeldTicket>("UPDATE branch_held_tickets SET status='checkout_pending',checkout_body=$5,checkout_key=$6,checkout_fingerprint=$7,updated_by=$8::uuid,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND revision=$4 AND claim_lease_id=$9::uuid RETURNING id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,completed_sale_id::text,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at")
        .bind(tenant_id).bind(branch_id).bind(ticket_id).bind(expected_revision).bind(body).bind(idempotency_key).bind(fingerprint).bind(membership_id).bind(lease_id).fetch_optional(&mut *tx).await.map_err(|_|TicketError::Database)?.ok_or(TicketError::Conflict)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(ticket)
}

pub(crate) async fn clear_checkout_attempt(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    ticket_id: &str,
    expected_revision: i64,
    lease_id: &str,
    lease_token: &str,
) -> Result<HeldTicket, TicketError> {
    let ticket_id = Uuid::parse_str(ticket_id).map_err(|_| TicketError::InvalidInput)?;
    let mut tx = begin_claimed(
        pool,
        identity_id,
        membership_id,
        tenant_id,
        branch_id,
        station_id,
        ticket_id,
        expected_revision,
        lease_id,
        lease_token,
    )
    .await?;
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM branch_held_tickets
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    .ok_or(TicketError::NotFound)?;
    if status != "checkout_pending" {
        return Err(TicketError::Conflict);
    }
    let sale_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_sales
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND held_ticket_id=$3)",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?;
    if sale_exists {
        return Err(TicketError::Conflict);
    }
    let ticket = sqlx::query_as::<_, HeldTicket>(
        "UPDATE branch_held_tickets SET status='held',checkout_body=NULL,checkout_key=NULL,
                checkout_fingerprint=NULL,updated_by=$5::uuid,revision=revision+1,
                updated_at=clock_timestamp()
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND revision=$4
           AND status='checkout_pending' AND claim_lease_id=$6::uuid
         RETURNING id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,
                completed_sale_id::text,
                to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,
                to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .bind(expected_revision)
    .bind(membership_id)
    .bind(lease_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    .ok_or(TicketError::Conflict)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(ticket)
}

pub(crate) async fn release(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    ticket_id: &str,
    expected_revision: i64,
    lease_id: &str,
    lease_token: &str,
    discard: bool,
) -> Result<HeldTicket, TicketError> {
    if expected_revision < 1 {
        return Err(TicketError::InvalidInput);
    }
    let ticket_id = Uuid::parse_str(ticket_id).map_err(|_| TicketError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
        lease_id,
    )
    .await?;
    validate_active_lease(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        lease_id,
        lease_token,
    )
    .await?;
    let current = sqlx::query_as::<_, (i64, String, Option<Value>, Option<String>)>(
        "SELECT revision,status,checkout_body,claim_lease_id::text FROM branch_held_tickets
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(ticket_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TicketError::Database)?
    .ok_or(TicketError::NotFound)?;
    if !discard && current.1 == "held" && current.3.is_none() {
        let ticket = fetch_ticket(&mut tx, tenant_id, branch_id, ticket_id).await?;
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(ticket);
    }
    if discard && current.1 == "discarded" && current.3.is_none() {
        let ticket = fetch_ticket(&mut tx, tenant_id, branch_id, ticket_id).await?;
        tx.commit().await.map_err(|_| TicketError::Database)?;
        return Ok(ticket);
    }
    if current.0 != expected_revision
        || current.1 != "held"
        || current.2.is_some()
        || current.3.as_deref() != Some(lease_id)
    {
        return Err(TicketError::Conflict);
    }
    let status = if discard { "discarded" } else { "held" };
    let ticket = sqlx::query_as::<_, HeldTicket>("UPDATE branch_held_tickets SET status=$5,claim_lease_id=NULL,claim_expires_at=NULL,updated_by=$6::uuid,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND revision=$4 AND claim_lease_id=$7::uuid RETURNING id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,completed_sale_id::text,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at")
        .bind(tenant_id).bind(branch_id).bind(ticket_id).bind(expected_revision).bind(status).bind(membership_id).bind(lease_id).fetch_optional(&mut *tx).await.map_err(|_|TicketError::Database)?.ok_or(TicketError::Conflict)?;
    tx.commit().await.map_err(|_| TicketError::Database)?;
    Ok(ticket)
}

async fn begin_claimed<'a>(
    pool: &'a PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    ticket_id: Uuid,
    expected_revision: i64,
    lease_id: &str,
    lease_token: &str,
) -> Result<Transaction<'a, Postgres>, TicketError> {
    if expected_revision < 1 {
        return Err(TicketError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| TicketError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
        lease_id,
    )
    .await?;
    validate_active_lease(
        &mut tx,
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        lease_id,
        lease_token,
    )
    .await?;
    let row = sqlx::query_as::<_, (i64,String,Option<String>)>("SELECT revision,status,claim_lease_id::text FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 FOR UPDATE")
        .bind(tenant_id).bind(branch_id).bind(ticket_id).fetch_optional(&mut *tx).await.map_err(|_|TicketError::Database)?.ok_or(TicketError::NotFound)?;
    if row.0 != expected_revision || row.2.as_deref() != Some(lease_id) {
        return Err(TicketError::Conflict);
    }
    if !matches!(row.1.as_str(), "held" | "checkout_pending") {
        return Err(TicketError::Conflict);
    }
    Ok(tx)
}

async fn fetch_ticket(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    branch_id: &str,
    ticket_id: Uuid,
) -> Result<HeldTicket, TicketError> {
    sqlx::query_as::<_, HeldTicket>("SELECT id::text,revision,status,payload,checkout_body,checkout_key,claim_lease_id::text,completed_sale_id::text,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3")
        .bind(tenant_id).bind(branch_id).bind(ticket_id).fetch_optional(&mut **tx).await.map_err(|_|TicketError::Database)?.ok_or(TicketError::NotFound)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    lease_id: &str,
) -> Result<(), TicketError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", station_id),
        ("pos.lease_id", lease_id),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| TicketError::Database)?;
    }
    Ok(())
}

async fn validate_active_lease(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    membership_id: &str,
    identity_id: &str,
    lease_id: &str,
    lease_token: &str,
) -> Result<(), TicketError> {
    let token_hash = Sha256::digest(lease_token.as_bytes()).to_vec();
    let active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM station_actor_leases l
            JOIN station_actor_lease_locks k ON k.station_id=l.station_id AND k.lease_id=l.id
            WHERE l.id=$1::uuid AND l.tenant_id=$2::uuid AND l.branch_id=$3::uuid
              AND l.station_id=$4::uuid AND l.membership_id=$5::uuid AND l.identity_id=$6::uuid
              AND l.lease_token_hash=$7 AND l.status='issued' AND l.expires_at>clock_timestamp()
        )",
    )
    .bind(lease_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(token_hash)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| TicketError::Database)?;
    if active {
        Ok(())
    } else {
        Err(TicketError::InvalidLease)
    }
}

fn validate_payload(payload: &Value) -> Result<(), TicketError> {
    if !payload.is_object()
        || serde_json::to_vec(payload)
            .map_err(|_| TicketError::InvalidInput)?
            .len()
            > MAX_DRAFT_BYTES
    {
        return Err(TicketError::InvalidInput);
    }
    Ok(())
}
