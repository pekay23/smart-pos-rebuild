// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::inventory_allocations;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_LINES: usize = 100;
const MAX_QUANTITY: i64 = 1_000_000;
const DEFAULT_HOLD_MINUTES: i32 = 60;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateInput {
    pub pickup_branch_id: String,
    pub customer_id: Option<String>,
    pub contact_name: String,
    pub contact_phone: String,
    pub contact_consent: bool,
    pub hold_minutes: Option<i32>,
    pub items: Vec<CreateLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateLineInput {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransitionInput {
    pub action: String,
    pub expected_revision: i64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReservationHeader {
    pub reservation_id: String,
    pub origin_branch_id: String,
    pub origin_station_id: String,
    pub pickup_branch_id: String,
    pub requested_by_membership_id: String,
    pub customer_id: Option<String>,
    pub contact_name: String,
    pub contact_phone: String,
    pub status: String,
    pub revision: i64,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReservationLine {
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PickupReservation {
    #[serde(flatten)]
    pub header: ReservationHeader,
    pub items: Vec<ReservationLine>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationError {
    InvalidInput,
    NotFound,
    Conflict,
    Forbidden,
    IdempotencyConflict,
    InsufficientStock,
    Database,
}

fn database_error(stage: &'static str, error: sqlx::Error) -> ReservationError {
    #[cfg(test)]
    eprintln!("pickup reservation {stage} failed: {error}");
    #[cfg(not(test))]
    let _ = (stage, error);
    ReservationError::Database
}

pub(crate) async fn create(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    origin_branch_id: &str,
    station_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: CreateInput,
) -> Result<PickupReservation, ReservationError> {
    let input = validate_create(input, idempotency_key)?;
    let hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| database_error("begin create", e))?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        origin_branch_id,
        Some(station_id),
    )
    .await?;

    let idempotency_lock =
        format!("pickup:{tenant_id}:{membership_id}:{station_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(idempotency_lock)
        .execute(&mut *tx)
        .await
        .map_err(|e| database_error("idempotency lock", e))?;
    if let Some((reservation_id, previous_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text,request_hash FROM pickup_reservations
         WHERE tenant_id=$1::uuid AND requested_by_membership_id=$2::uuid
           AND origin_station_id=$3::uuid AND request_idempotency_key=$4",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(station_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| database_error("retry lookup", e))?
    {
        if previous_hash != hash {
            return Err(ReservationError::IdempotencyConflict);
        }
        let mut reservation = get_reservation(&mut tx, tenant_id, &reservation_id).await?;
        reservation.replayed = true;
        tx.commit()
            .await
            .map_err(|e| database_error("commit retry", e))?;
        return Ok(reservation);
    }

    sqlx::query("SELECT set_config('pos.inventory_availability','enabled',true)")
        .execute(&mut *tx)
        .await
        .map_err(|e| database_error("enable pickup availability", e))?;
    let pickup_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active')",
    ).bind(tenant_id).bind(&input.pickup_branch_id).fetch_one(&mut *tx).await
        .map_err(|e| database_error("pickup branch lookup", e))?;
    if !pickup_active || input.pickup_branch_id == origin_branch_id {
        return Err(ReservationError::NotFound);
    }

    inventory_allocations::lock_products(
        &mut tx,
        tenant_id,
        &input.pickup_branch_id,
        input.items.iter().map(|line| line.product_id.as_str()),
    )
    .await
    .map_err(|e| database_error("inventory locks", e))?;

    if let Some(customer_id) = &input.customer_id {
        let customer_active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active')",
        ).bind(tenant_id).bind(customer_id).fetch_one(&mut *tx).await
            .map_err(|e| database_error("customer validation", e))?;
        if !customer_active {
            return Err(ReservationError::NotFound);
        }
    }

    let now = sqlx::query_scalar::<_, String>("SELECT clock_timestamp()::text")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| database_error("database time", e))?;
    let expires_at = sqlx::query_scalar::<_, String>(
        "SELECT ($1::timestamptz + make_interval(mins => $2::int))::text",
    )
    .bind(&now)
    .bind(input.hold_minutes.unwrap_or(DEFAULT_HOLD_MINUTES))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| database_error("expiry calculation", e))?;

    let reservation_id = Uuid::new_v4();
    let digest: [u8; 32] = hash
        .as_slice()
        .try_into()
        .map_err(|_| ReservationError::Database)?;
    sqlx::query(
        "INSERT INTO pickup_reservations
          (id,tenant_id,origin_branch_id,origin_station_id,pickup_branch_id,requested_by_membership_id,
           customer_id,contact_name_snapshot,contact_phone_snapshot,contact_consent_at,status,revision,
           expires_at,request_idempotency_key,request_hash)
         VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8,$9,$10::timestamptz,'held',1,$11::timestamptz,$12,$13)",
    ).bind(reservation_id).bind(tenant_id).bind(origin_branch_id).bind(station_id)
     .bind(&input.pickup_branch_id).bind(membership_id).bind(&input.customer_id)
     .bind(input.contact_name.trim()).bind(input.contact_phone.trim()).bind(now)
     .bind(&expires_at).bind(idempotency_key).bind(digest.as_slice())
     .execute(&mut *tx).await.map_err(|e| map_insert_error("insert reservation",e))?;

    for line in &input.items {
        let Some((sku, name)) = sqlx::query_as::<_, (String, String)>(
            "SELECT p.sku,p.name FROM catalog_products p
             JOIN branch_catalog_items i ON i.tenant_id=p.tenant_id AND i.product_id=p.id
             WHERE p.tenant_id=$1::uuid AND p.id=$2::uuid AND p.status='active'
               AND i.branch_id=$3::uuid AND i.status='active'",
        )
        .bind(tenant_id)
        .bind(&line.product_id)
        .bind(&input.pickup_branch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| database_error("product mapping", e))?
        else {
            return Err(ReservationError::NotFound);
        };
        let on_hand = sqlx::query_scalar::<_,i64>(
            "SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND status='active'",
        ).bind(tenant_id).bind(&input.pickup_branch_id).bind(&line.product_id)
         .fetch_optional(&mut *tx).await.map_err(|e| database_error("stock lookup",e))?
         .ok_or(ReservationError::NotFound)?;
        let committed = inventory_allocations::allocated_quantity(
            &mut tx,
            tenant_id,
            &input.pickup_branch_id,
            &line.product_id,
            None,
        )
        .await
        .map_err(|e| database_error("commitment lookup", e))?;
        if !inventory_allocations::can_consume(on_hand, committed, line.quantity) {
            return Err(ReservationError::InsufficientStock);
        }
        sqlx::query("INSERT INTO pickup_reservation_lines(tenant_id,reservation_id,product_id,pickup_branch_id,sku_snapshot,name_snapshot,quantity) VALUES($1::uuid,$2,$3::uuid,$4::uuid,$5,$6,$7)")
            .bind(tenant_id).bind(reservation_id).bind(&line.product_id).bind(&input.pickup_branch_id).bind(sku).bind(name).bind(line.quantity)
            .execute(&mut *tx).await.map_err(|e| database_error("insert reservation line",e))?;
        sqlx::query("INSERT INTO inventory_allocations(tenant_id,branch_id,product_id,pickup_reservation_id,quantity,expires_at) VALUES($1::uuid,$2::uuid,$3::uuid,$4,$5,$6::timestamptz)")
            .bind(tenant_id).bind(&input.pickup_branch_id).bind(&line.product_id).bind(reservation_id).bind(line.quantity).bind(&expires_at)
            .execute(&mut *tx).await.map_err(|e| map_insert_error("insert allocation",e))?;
    }

    sqlx::query("INSERT INTO pickup_reservation_events(tenant_id,reservation_id,acting_branch_id,acting_station_id,actor_membership_id,event_type,revision,idempotency_key,request_hash) VALUES($1::uuid,$2,$3::uuid,$4::uuid,$5::uuid,'held',1,$6,$7)")
        .bind(tenant_id).bind(reservation_id).bind(origin_branch_id).bind(station_id).bind(membership_id).bind(idempotency_key).bind(digest.as_slice())
        .execute(&mut *tx).await.map_err(|e| database_error("insert held event",e))?;
    let result = get_reservation(&mut tx, tenant_id, &reservation_id.to_string()).await?;
    tx.commit()
        .await
        .map_err(|e| database_error("commit reservation", e))?;
    Ok(result)
}

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<Vec<PickupReservation>, ReservationError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| database_error("begin list", e))?;
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
        "SELECT id::text FROM pickup_reservations WHERE tenant_id=$1::uuid
         AND (origin_branch_id=$2::uuid OR pickup_branch_id=$2::uuid)
         ORDER BY created_at DESC,id DESC LIMIT 100",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| database_error("list reservations", e))?;
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        rows.push(get_reservation(&mut tx, tenant_id, &id).await?);
    }
    tx.commit()
        .await
        .map_err(|e| database_error("commit list", e))?;
    Ok(rows)
}

pub(crate) async fn transition(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    reservation_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: TransitionInput,
) -> Result<PickupReservation, ReservationError> {
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
        station_id,
    )
    .await?;

    let lock_key = format!("pickup:{tenant_id}:{reservation_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("transition idempotency lock", error))?;
    if let Some(previous_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM pickup_reservation_events
         WHERE tenant_id=$1::uuid AND reservation_id=$2::uuid
           AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    )
    .bind(tenant_id)
    .bind(reservation_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| database_error("transition replay lookup", error))?
    {
        if previous_hash != payload_hash {
            return Err(ReservationError::IdempotencyConflict);
        }
        let mut reservation = get_reservation(&mut tx, tenant_id, reservation_id).await?;
        reservation.replayed = true;
        tx.commit()
            .await
            .map_err(|error| database_error("commit transition replay", error))?;
        return Ok(reservation);
    }

    let (origin_branch, pickup_branch, initial_revision) =
        sqlx::query_as::<_, (String, String, i64)>(
            "SELECT origin_branch_id::text,pickup_branch_id::text,revision
             FROM pickup_reservations WHERE tenant_id=$1::uuid AND id=$2::uuid",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| database_error("read reservation before inventory lock", error))?
        .ok_or(ReservationError::NotFound)?;
    let allowed_branch = if input.action == "ready" {
        branch_id == pickup_branch
    } else {
        branch_id == origin_branch || branch_id == pickup_branch
    };
    if !allowed_branch {
        return Err(ReservationError::NotFound);
    }

    let product_ids = if input.action == "cancel" {
        sqlx::query_scalar::<_, String>(
            "SELECT product_id::text FROM pickup_reservation_lines
             WHERE tenant_id=$1::uuid AND reservation_id=$2::uuid ORDER BY product_id",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| database_error("read reservation products", error))?
    } else {
        Vec::new()
    };
    if input.action == "cancel" {
        inventory_allocations::lock_products(
            &mut tx,
            tenant_id,
            &pickup_branch,
            product_ids.iter().map(String::as_str),
        )
        .await
        .map_err(|error| database_error("lock cancellation inventory", error))?;
    }

    let (status, revision, unexpired) = sqlx::query_as::<_, (String, i64, bool)>(
        "SELECT status,revision,expires_at>clock_timestamp() FROM pickup_reservations
         WHERE tenant_id=$1::uuid AND id=$2::uuid FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(reservation_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| database_error("lock reservation", error))?
    .ok_or(ReservationError::NotFound)?;
    if revision != input.expected_revision || revision != initial_revision {
        return Err(ReservationError::Conflict);
    }
    let next_status = match input.action.as_str() {
        "ready" if status == "held" && unexpired => "ready",
        "cancel" if status == "held" || status == "ready" => "cancelled",
        _ => return Err(ReservationError::Conflict),
    };
    let changed = sqlx::query(
        "UPDATE pickup_reservations SET status=$3,revision=revision+1,updated_at=clock_timestamp()
         WHERE tenant_id=$1::uuid AND id=$2::uuid AND revision=$4 AND status=$5",
    )
    .bind(tenant_id)
    .bind(reservation_id)
    .bind(next_status)
    .bind(revision)
    .bind(&status)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .and_then(|database| database.code())
            .is_some_and(|code| code == "42501")
        {
            ReservationError::Forbidden
        } else {
            database_error("update reservation", error)
        }
    })?;
    if changed.rows_affected() != 1 {
        return Err(ReservationError::Conflict);
    }
    let next_revision = revision + 1;
    let event_hash: [u8; 32] = payload_hash
        .as_slice()
        .try_into()
        .map_err(|_| ReservationError::Database)?;
    sqlx::query(
        "INSERT INTO pickup_reservation_events
         (tenant_id,reservation_id,acting_branch_id,acting_station_id,actor_membership_id,
          event_type,revision,idempotency_key,request_hash,reason)
         VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6,$7,$8,$9,$10)",
    )
    .bind(tenant_id)
    .bind(reservation_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(next_status)
    .bind(next_revision)
    .bind(idempotency_key)
    .bind(event_hash.as_slice())
    .bind(&input.reason)
    .execute(&mut *tx)
    .await
    .map_err(|error| database_error("append transition event", error))?;

    if next_status == "cancelled" {
        let released = sqlx::query(
            "UPDATE inventory_allocations SET released_at=clock_timestamp(),release_reason=$3
             WHERE tenant_id=$1::uuid AND pickup_reservation_id=$2::uuid AND released_at IS NULL",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .bind("cancelled")
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("release cancelled pickup allocation", error))?;
        if released.rows_affected() as usize != product_ids.len() {
            return Err(ReservationError::Database);
        }
    }
    sqlx::query(
        "INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata)
         VALUES($1::uuid,$2::uuid,$3,'pickup_reservation',$4::uuid,$5)",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(format!("pickup_reservation_{next_status}"))
    .bind(reservation_id)
    .bind(serde_json::json!({"revision": next_revision, "branchId": branch_id, "reason": input.reason}))
    .execute(&mut *tx)
    .await
    .map_err(|error| database_error("audit reservation transition", error))?;

    let result = get_reservation(&mut tx, tenant_id, reservation_id).await?;
    tx.commit()
        .await
        .map_err(|error| database_error("commit reservation transition", error))?;
    Ok(result)
}

async fn get_reservation(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    id: &str,
) -> Result<PickupReservation, ReservationError> {
    let header=sqlx::query_as::<_,ReservationHeader>(
        "SELECT id::text AS reservation_id,origin_branch_id::text,origin_station_id::text,
          pickup_branch_id::text,requested_by_membership_id::text,customer_id::text,
          contact_name_snapshot AS contact_name,contact_phone_snapshot AS contact_phone,
          CASE WHEN status IN ('held','ready') AND expires_at<=clock_timestamp() THEN 'expired' ELSE status END AS status,
          revision,to_char(expires_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS expires_at,
          to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at
         FROM pickup_reservations WHERE tenant_id=$1::uuid AND id=$2::uuid",
    ).bind(tenant_id).bind(id).fetch_optional(&mut **tx).await
     .map_err(|e| database_error("load reservation",e))?.ok_or(ReservationError::NotFound)?;
    let items = sqlx::query_as::<_, ReservationLine>(
        "SELECT product_id::text,sku_snapshot AS sku,name_snapshot AS name,quantity
         FROM pickup_reservation_lines WHERE tenant_id=$1::uuid AND reservation_id=$2::uuid
         ORDER BY lower(name_snapshot),product_id",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| database_error("load reservation lines", e))?;
    Ok(PickupReservation {
        header,
        items,
        replayed: false,
    })
}

fn validate_create(mut input: CreateInput, key: &str) -> Result<CreateInput, ReservationError> {
    if Uuid::parse_str(&input.pickup_branch_id).is_err()
        || key.is_empty()
        || key.len() > 128
        || !key.bytes().all(|b| (0x21..=0x7e).contains(&b))
        || input.items.is_empty()
        || input.items.len() > MAX_LINES
        || !input.contact_consent
    {
        return Err(ReservationError::InvalidInput);
    }
    input.contact_name = input.contact_name.trim().to_owned();
    input.contact_phone = input.contact_phone.trim().to_owned();
    if input.contact_name.chars().count() == 0
        || input.contact_name.chars().count() > 160
        || input.contact_phone.is_empty()
        || input.contact_phone.len() > 40
        || input
            .hold_minutes
            .is_some_and(|minutes| !(15..=1440).contains(&minutes))
        || input
            .customer_id
            .as_ref()
            .is_some_and(|id| Uuid::parse_str(id).is_err())
    {
        return Err(ReservationError::InvalidInput);
    }
    let mut products = std::collections::HashSet::with_capacity(input.items.len());
    for line in &input.items {
        if Uuid::parse_str(&line.product_id).is_err()
            || !(1..=MAX_QUANTITY).contains(&line.quantity)
            || !products.insert(line.product_id.to_ascii_lowercase())
        {
            return Err(ReservationError::InvalidInput);
        }
    }
    Ok(input)
}

fn validate_transition(
    mut input: TransitionInput,
    idempotency_key: &str,
) -> Result<TransitionInput, ReservationError> {
    if input.expected_revision <= 0
        || !["ready", "cancel"].contains(&input.action.as_str())
        || idempotency_key.is_empty()
        || idempotency_key.len() > 128
        || !idempotency_key
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(ReservationError::InvalidInput);
    }
    input.reason = input
        .reason
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if (input.action == "cancel" && input.reason.is_none())
        || input
            .reason
            .as_ref()
            .is_some_and(|reason| reason.chars().count() > 450)
    {
        return Err(ReservationError::InvalidInput);
    }
    Ok(input)
}

fn map_insert_error(stage: &'static str, error: sqlx::Error) -> ReservationError {
    if error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation())
    {
        ReservationError::Conflict
    } else if error
        .as_database_error()
        .is_some_and(|db| db.constraint() == Some("inventory_allocation_within_stock"))
    {
        ReservationError::InsufficientStock
    } else {
        database_error(stage, error)
    }
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    station: Option<&str>,
) -> Result<(), ReservationError> {
    sqlx::query("SELECT set_config('pos.identity_id',$1,true),set_config('pos.tenant_id',$2,true),set_config('pos.membership_id',$3,true),set_config('pos.branch_id',$4,true),set_config('pos.station_id',$5,true)")
      .bind(identity).bind(tenant).bind(membership).bind(branch).bind(station.unwrap_or(""))
      .execute(&mut **tx).await.map_err(|e|database_error("set request context",e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_create, validate_transition, CreateInput, CreateLineInput, ReservationError,
        TransitionInput,
    };

    fn valid() -> CreateInput {
        CreateInput {
            pickup_branch_id: "50000000-0000-0000-0000-000000000005".into(),
            customer_id: None,
            contact_name: "Walk-in buyer".into(),
            contact_phone: "+233200000000".into(),
            contact_consent: true,
            hold_minutes: None,
            items: vec![CreateLineInput {
                product_id: "60000000-0000-0000-0000-000000000006".into(),
                quantity: 2,
            }],
        }
    }

    #[test]
    fn bounded_hold_and_contact_consent_are_required() {
        assert!(validate_create(valid(), "key").is_ok());
        let mut value = valid();
        value.hold_minutes = Some(14);
        assert_eq!(
            validate_create(value, "key").unwrap_err(),
            ReservationError::InvalidInput
        );
        let mut value = valid();
        value.contact_consent = false;
        assert_eq!(
            validate_create(value, "key").unwrap_err(),
            ReservationError::InvalidInput
        );
    }

    #[test]
    fn duplicate_products_and_bad_keys_fail_closed() {
        let mut value = valid();
        value.items.push(value.items[0].clone());
        assert_eq!(
            validate_create(value, "key").unwrap_err(),
            ReservationError::InvalidInput
        );
        assert_eq!(
            validate_create(valid(), " ").unwrap_err(),
            ReservationError::InvalidInput
        );
    }

    #[test]
    fn transition_validation_requires_revision_and_a_bounded_cancellation_reason() {
        assert!(validate_transition(
            TransitionInput {
                action: "ready".into(),
                expected_revision: 1,
                reason: None,
            },
            "ready-key"
        )
        .is_ok());
        assert_eq!(
            validate_transition(
                TransitionInput {
                    action: "cancel".into(),
                    expected_revision: 1,
                    reason: None,
                },
                "cancel-key"
            )
            .unwrap_err(),
            ReservationError::InvalidInput
        );
        assert_eq!(
            validate_transition(
                TransitionInput {
                    action: "delete".into(),
                    expected_revision: 1,
                    reason: Some("bad".into()),
                },
                "bad-key"
            )
            .unwrap_err(),
            ReservationError::InvalidInput
        );
        assert_eq!(
            validate_transition(
                TransitionInput {
                    action: "cancel".into(),
                    expected_revision: 0,
                    reason: Some("because".into()),
                },
                "cancel-key"
            )
            .unwrap_err(),
            ReservationError::InvalidInput
        );
        assert_eq!(
            validate_transition(
                TransitionInput {
                    action: "cancel".into(),
                    expected_revision: 1,
                    reason: Some("  Customer cancelled  ".into()),
                },
                "cancel-key"
            )
            .unwrap()
            .reason
            .as_deref(),
            Some("Customer cancelled")
        );
    }
}
