// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributionError {
    InvalidInput,
    NotFound,
    Conflict,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReclassifyInput {
    expected_revision: i64,
    cost_center_kind: String,
    station_id: Option<String>,
    reason: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reclassification {
    pub id: String,
    pub expense_id: String,
    pub revision: i64,
    pub previous_kind: String,
    pub previous_station_id: Option<String>,
    pub previous_station_name_snapshot: Option<String>,
    pub next_kind: String,
    pub next_station_id: Option<String>,
    pub next_station_name_snapshot: Option<String>,
    pub reason: String,
    pub recorded_at: String,
    pub actor_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttributionHistory {
    pub current_revision: i64,
    pub current_kind: String,
    pub current_station_id: Option<String>,
    pub current_station_name_snapshot: Option<String>,
    pub rows: Vec<Reclassification>,
}

pub(crate) async fn history(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    expense_id: &str,
) -> Result<AttributionHistory, AttributionError> {
    let expense_id = Uuid::parse_str(expense_id).map_err(|_| AttributionError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_expenses WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3)",
    )
    .bind(tenant)
    .bind(branch)
    .bind(expense_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    if !exists {
        return Err(AttributionError::NotFound);
    }
    let rows = list(&mut tx, tenant, branch, expense_id).await?;
    tx.commit().await.map_err(database_error)?;
    let latest = rows.last();
    Ok(AttributionHistory {
        current_revision: latest.map_or(0, |row| row.revision),
        current_kind: latest.map_or_else(
            || "legacy_unassigned".to_owned(),
            |row| row.next_kind.clone(),
        ),
        current_station_id: latest.and_then(|row| row.next_station_id.clone()),
        current_station_name_snapshot: latest
            .and_then(|row| row.next_station_name_snapshot.clone()),
        rows,
    })
}

pub(crate) async fn reclassify(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    expense_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    mut input: ReclassifyInput,
) -> Result<Reclassification, AttributionError> {
    let expense_id = Uuid::parse_str(expense_id).map_err(|_| AttributionError::InvalidInput)?;
    let station_id = input
        .station_id
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let station_id = station_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| AttributionError::InvalidInput)?;
    input.cost_center_kind = input.cost_center_kind.trim().to_owned();
    input.reason = input.reason.trim().to_owned();
    if !(0..MAX_SAFE_INTEGER).contains(&input.expected_revision)
        || !(1..=128).contains(&idempotency_key.len())
        || !idempotency_key
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
        || !matches!(
            input.cost_center_kind.as_str(),
            "station_direct" | "store_overhead" | "legacy_unassigned"
        )
        || (input.cost_center_kind == "station_direct") != station_id.is_some()
        || !(5..=500).contains(&input.reason.chars().count())
    {
        return Err(AttributionError::InvalidInput);
    }
    let hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let lock = format!("expense-attribution:{tenant}:{branch}:{expense_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;

    if let Some((saved_hash, saved)) =
        by_key(&mut tx, tenant, branch, membership, idempotency_key).await?
    {
        if saved.expense_id != expense_id.to_string() || saved_hash != hash {
            return Err(AttributionError::Conflict);
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(saved);
    }
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_expenses WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3)",
    )
    .bind(tenant)
    .bind(branch)
    .bind(expense_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    if !exists {
        return Err(AttributionError::NotFound);
    }
    let rows = list(&mut tx, tenant, branch, expense_id).await?;
    let latest = rows.last();
    let current_revision = latest.map_or(0, |row| row.revision);
    if current_revision != input.expected_revision {
        return Err(AttributionError::Conflict);
    }
    let previous_kind = latest.map_or("legacy_unassigned", |row| row.next_kind.as_str());
    let previous_station_id = latest
        .and_then(|row| row.next_station_id.as_deref())
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| AttributionError::Database)?;
    let previous_station_name = latest.and_then(|row| row.next_station_name_snapshot.as_deref());
    if previous_kind == input.cost_center_kind && previous_station_id == station_id {
        return Err(AttributionError::InvalidInput);
    }
    let next_station_name = if let Some(station_id) = station_id {
        Some(sqlx::query_scalar::<_, String>(
            "SELECT name FROM stations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid AND status='active'",
        )
        .bind(tenant)
        .bind(branch)
        .bind(station_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database_error)?
        .ok_or(AttributionError::NotFound)?)
    } else {
        None
    };
    let revision = current_revision
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .ok_or(AttributionError::InvalidInput)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_expense_cost_center_reclassifications(id,tenant_id,branch_id,expense_id,revision,expected_revision,previous_kind,previous_station_id,previous_station_name_snapshot,next_kind,next_station_id,next_station_name_snapshot,reason,actor_membership_id,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14::uuid,$15,$16)")
        .bind(id).bind(tenant).bind(branch).bind(expense_id).bind(revision).bind(input.expected_revision)
        .bind(previous_kind).bind(previous_station_id).bind(previous_station_name).bind(&input.cost_center_kind)
        .bind(station_id).bind(&next_station_name).bind(&input.reason).bind(membership).bind(idempotency_key).bind(&hash)
        .execute(&mut *tx).await.map_err(|error| if error.as_database_error().is_some_and(|db| db.is_unique_violation()) { AttributionError::Conflict } else { AttributionError::Database })?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'expense_cost_center_reclassified','expense',$3,$4)")
        .bind(tenant).bind(membership).bind(expense_id).bind(serde_json::json!({"reclassificationId":id.to_string(),"revision":revision,"previousKind":previous_kind,"previousStationId":previous_station_id.map(|value| value.to_string()),"nextKind":input.cost_center_kind,"nextStationId":station_id.map(|value| value.to_string()),"reason":input.reason,"effectiveAt":"original expense and reimbursement dates"}))
        .execute(&mut *tx).await.map_err(database_error)?;
    let saved = by_id(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(saved)
}

async fn list(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    expense: Uuid,
) -> Result<Vec<Reclassification>, AttributionError> {
    sqlx::query_as::<_, Reclassification>("SELECT r.id::text,r.expense_id::text,r.revision,r.previous_kind,r.previous_station_id::text AS previous_station_id,r.previous_station_name_snapshot,r.next_kind,r.next_station_id::text AS next_station_id,r.next_station_name_snapshot,r.reason,to_char(r.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at,COALESCE(NULLIF(i.display_name,''),'Staff') AS actor_name FROM hosted_expense_cost_center_reclassifications r LEFT JOIN memberships m ON m.tenant_id=r.tenant_id AND m.id=r.actor_membership_id LEFT JOIN person_identities i ON i.id=m.identity_id WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.expense_id=$3 ORDER BY r.revision")
        .bind(tenant).bind(branch).bind(expense).fetch_all(&mut **tx).await.map_err(database_error)
}

async fn by_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<Option<(Vec<u8>, Reclassification)>, AttributionError> {
    let saved = sqlx::query_as::<_,(Vec<u8>,Uuid)>("SELECT request_hash,id FROM hosted_expense_cost_center_reclassifications WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4")
        .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut **tx).await.map_err(database_error)?;
    match saved {
        Some((hash, id)) => Ok(Some((hash, by_id(tx, tenant, branch, id).await?))),
        None => Ok(None),
    }
}

async fn by_id(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: Uuid,
) -> Result<Reclassification, AttributionError> {
    sqlx::query_as::<_,Reclassification>("SELECT r.id::text,r.expense_id::text,r.revision,r.previous_kind,r.previous_station_id::text AS previous_station_id,r.previous_station_name_snapshot,r.next_kind,r.next_station_id::text AS next_station_id,r.next_station_name_snapshot,r.reason,to_char(r.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at,COALESCE(NULLIF(i.display_name,''),'Staff') AS actor_name FROM hosted_expense_cost_center_reclassifications r LEFT JOIN memberships m ON m.tenant_id=r.tenant_id AND m.id=r.actor_membership_id LEFT JOIN person_identities i ON i.id=m.identity_id WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.id=$3")
        .bind(tenant).bind(branch).bind(id).fetch_optional(&mut **tx).await.map_err(database_error)?.ok_or(AttributionError::Database)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<(), AttributionError> {
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

fn database_error(_: sqlx::Error) -> AttributionError {
    AttributionError::Database
}
