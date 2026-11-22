// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CashLocationError {
    InvalidInput,
    NotFound,
    Conflict,
    InsufficientFunds,
    CutoverBlocked,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateLocationInput {
    name: String,
    location_type: String,
    station_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpeningInput {
    amount_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransferInput {
    from_location_id: String,
    to_location_id: String,
    amount_minor: i64,
    note: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReconciliationInput {
    reason: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashLocation {
    pub id: String,
    pub name: String,
    pub location_type: String,
    pub station_id: Option<String>,
    pub status: String,
    pub currency_code: String,
    pub balance_minor: i64,
    pub initialized: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocationResult<T> {
    #[serde(flatten)]
    pub value: T,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashLocationEvent {
    pub id: String,
    pub event_type: String,
    pub signed_amount_minor: i64,
    pub source_transfer_id: Option<String>,
    pub source_expense_id: Option<String>,
    pub source_supplier_payment_id: Option<String>,
    pub source_expense_reimbursement_id: Option<String>,
    pub source_sale_tender_id: Option<String>,
    pub source_refund_tender_id: Option<String>,
    pub source_drawer_movement_id: Option<String>,
    pub actor_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashReconciliation {
    pub id: String,
    pub currency_code: String,
    pub tracked_cash_minor: i64,
    pub ledger_cash_before_minor: i64,
    pub adjustment_minor: i64,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashReconciliationStatus {
    pub currency_code: String,
    pub active_location_count: i64,
    pub tracked_cash_minor: i64,
    pub ledger_cash_minor: i64,
    pub variance_minor: i64,
    pub uninitialized_location_count: i64,
    pub open_shift_count: i64,
    pub unresolved_drawer_movement_count: i64,
    pub latest: Option<CashReconciliation>,
    pub status: &'static str,
}

pub(crate) async fn reconciliation_status(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<CashReconciliationStatus, CashLocationError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let (currency, active_locations, uninitialized, tracked, open_shifts): (String, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT b.currency_code,COUNT(DISTINCT l.id)::bigint,
          COUNT(DISTINCT l.id) FILTER (WHERE NOT EXISTS (SELECT 1 FROM hosted_cash_location_events o WHERE o.tenant_id=l.tenant_id AND o.branch_id=l.branch_id AND o.location_id=l.id AND o.event_type='opening'))::bigint,
          COALESCE(SUM(e.signed_amount_minor),0)::bigint,
          (SELECT COUNT(*)::bigint FROM hosted_drawer_shifts s WHERE s.tenant_id=b.tenant_id AND s.branch_id=b.id AND s.status='open')
         FROM branches b LEFT JOIN hosted_cash_locations l ON l.tenant_id=b.tenant_id AND l.branch_id=b.id AND l.status='active'
         LEFT JOIN hosted_cash_location_events e ON e.tenant_id=l.tenant_id AND e.branch_id=l.branch_id AND e.location_id=l.id
         WHERE b.tenant_id=$1::uuid AND b.id=$2::uuid GROUP BY b.currency_code,b.tenant_id,b.id",
    ).bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(CashLocationError::NotFound)?;
    let ledger: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.debit_minor-l.credit_minor),0)::bigint FROM hosted_accounting_journals j
         JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
         WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.currency_code=$3 AND l.account_code='1000'",
    ).bind(tenant).bind(branch).bind(&currency).fetch_one(&mut *tx).await.map_err(database_error)?;
    let latest = sqlx::query_as::<_, CashReconciliation>(
        "SELECT id::text AS id,currency_code,tracked_cash_minor,ledger_cash_before_minor,adjustment_minor,reason,created_at::text AS created_at
         FROM hosted_cash_reconciliations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid ORDER BY created_at DESC,id DESC LIMIT 1",
    ).bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(database_error)?;
    let unresolved: i64 = if let Some(snapshot) = &latest {
        sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM hosted_cash_location_events e
             WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid AND e.event_type IN ('register_movement_inflow','register_movement_outflow')
               AND e.created_at > $3::timestamptz
               AND NOT EXISTS (SELECT 1 FROM hosted_cash_location_events d WHERE d.tenant_id=e.tenant_id AND d.branch_id=e.branch_id
                 AND d.source_drawer_movement_id=e.source_drawer_movement_id AND d.event_type='cash_drop_inflow')",
        ).bind(tenant).bind(branch).bind(&snapshot.created_at).fetch_one(&mut *tx).await.map_err(database_error)?
    } else {
        0
    };
    tx.commit().await.map_err(database_error)?;
    let variance = tracked
        .checked_sub(ledger)
        .ok_or(CashLocationError::Database)?;
    let status = if latest.is_none() {
        "cutover_required"
    } else if uninitialized > 0 {
        "incomplete_locations"
    } else if open_shifts > 0 {
        "open_shifts"
    } else if unresolved > 0 {
        "unresolved_drawer_movements"
    } else if variance == 0 {
        "reconciled"
    } else {
        "variance"
    };
    Ok(CashReconciliationStatus {
        currency_code: currency,
        active_location_count: active_locations,
        tracked_cash_minor: tracked,
        ledger_cash_minor: ledger,
        variance_minor: variance,
        uninitialized_location_count: uninitialized,
        open_shift_count: open_shifts,
        unresolved_drawer_movement_count: unresolved,
        latest,
        status,
    })
}

pub(crate) async fn reconcile(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: ReconciliationInput,
) -> Result<LocationResult<CashReconciliation>, CashLocationError> {
    validate_key(key)?;
    let reason = input.reason.trim();
    if !(3..=200).contains(&reason.chars().count()) {
        return Err(CashLocationError::InvalidInput);
    }
    let hash = request_hash(raw);
    let mut tx = pool.begin().await.map_err(database_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    lock_key(
        &mut tx,
        "cash-accounting-reconciliation",
        tenant,
        branch,
        "branch",
        "cutover",
    )
    .await?;

    if let Some((previous_hash, id)) = sqlx::query_as::<_, (Vec<u8>, Uuid)>(
        "SELECT request_hash,id FROM hosted_cash_reconciliations
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    )
    .bind(tenant).bind(branch).bind(membership).bind(key)
    .fetch_optional(&mut *tx).await.map_err(database_error)? {
        if previous_hash != hash { return Err(CashLocationError::Conflict); }
        let value = load_reconciliation(&mut tx, tenant, branch, id).await?;
        tx.commit().await.map_err(database_error)?;
        return Ok(LocationResult { value, replayed: true });
    }

    let (currency, location_count, uninitialized_count, tracked_cash, open_shift): (String, i64, i64, i64, bool) =
        sqlx::query_as(
            "SELECT b.currency_code,COUNT(DISTINCT l.id)::bigint,
              COUNT(DISTINCT l.id) FILTER (WHERE NOT EXISTS (SELECT 1 FROM hosted_cash_location_events o
                WHERE o.tenant_id=l.tenant_id AND o.branch_id=l.branch_id AND o.location_id=l.id AND o.event_type='opening'))::bigint,
              COALESCE(SUM(e.signed_amount_minor),0)::bigint,
              EXISTS(SELECT 1 FROM hosted_drawer_shifts s WHERE s.tenant_id=b.tenant_id AND s.branch_id=b.id AND s.status='open')
             FROM branches b LEFT JOIN hosted_cash_locations l ON l.tenant_id=b.tenant_id AND l.branch_id=b.id AND l.status='active'
             LEFT JOIN hosted_cash_location_events e ON e.tenant_id=l.tenant_id AND e.branch_id=l.branch_id AND e.location_id=l.id
             WHERE b.tenant_id=$1::uuid AND b.id=$2::uuid GROUP BY b.currency_code,b.tenant_id,b.id",
        )
        .bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(database_error)?
        .ok_or(CashLocationError::NotFound)?;
    if location_count == 0 || uninitialized_count != 0 || open_shift {
        return Err(CashLocationError::CutoverBlocked);
    }
    if !(0..=MAX_SAFE_INTEGER).contains(&tracked_cash) {
        return Err(CashLocationError::InvalidInput);
    }
    let ledger_cash: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.debit_minor-l.credit_minor),0)::bigint
         FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l
           ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
         WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.currency_code=$3 AND l.account_code='1000'",
    )
    .bind(tenant).bind(branch).bind(&currency).fetch_one(&mut *tx).await.map_err(database_error)?;
    let adjustment = tracked_cash
        .checked_sub(ledger_cash)
        .ok_or(CashLocationError::InvalidInput)?;
    if adjustment.unsigned_abs() > MAX_SAFE_INTEGER as u64 {
        return Err(CashLocationError::InvalidInput);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO hosted_cash_reconciliations(id,tenant_id,branch_id,actor_membership_id,currency_code,tracked_cash_minor,ledger_cash_before_minor,adjustment_minor,reason,idempotency_key,request_hash)
         VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id).bind(tenant).bind(branch).bind(membership).bind(&currency)
    .bind(tracked_cash).bind(ledger_cash).bind(adjustment).bind(reason).bind(key).bind(hash)
    .execute(&mut *tx).await.map_err(map_insert_error)?;

    if adjustment != 0 {
        let amount = adjustment
            .checked_abs()
            .ok_or(CashLocationError::InvalidInput)?;
        let journal_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description)
             VALUES($1,$2::uuid,$3::uuid,'cash_reconciliation',$4,$5,now(),$6::uuid,$7)",
        )
        .bind(journal_id).bind(tenant).bind(branch).bind(id).bind(&currency).bind(membership)
        .bind(format!("Cash cutover reconciliation: {reason}"))
        .execute(&mut *tx).await.map_err(database_error)?;
        let (cash_debit, cash_credit, equity_debit, equity_credit) = if adjustment > 0 {
            (amount, 0, 0, amount)
        } else {
            (0, amount, amount, 0)
        };
        for (line, account, description, debit, credit) in [
            (
                1_i16,
                "1000",
                "Physical cash brought to ledger balance",
                cash_debit,
                cash_credit,
            ),
            (
                2_i16,
                "3000",
                "Opening equity cash cutover",
                equity_debit,
                equity_credit,
            ),
        ] {
            sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
                .bind(tenant).bind(branch).bind(journal_id).bind(line).bind(account).bind(description).bind(debit).bind(credit)
                .execute(&mut *tx).await.map_err(database_error)?;
        }
    }
    let value = load_reconciliation(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(LocationResult {
        value,
        replayed: false,
    })
}

async fn load_reconciliation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: Uuid,
) -> Result<CashReconciliation, CashLocationError> {
    sqlx::query_as::<_, CashReconciliation>(
        "SELECT id::text AS id,currency_code,tracked_cash_minor,ledger_cash_before_minor,adjustment_minor,reason,created_at::text AS created_at
         FROM hosted_cash_reconciliations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
    )
    .bind(tenant).bind(branch).bind(id).fetch_optional(&mut **tx).await.map_err(database_error)?
        .ok_or(CashLocationError::NotFound)
}

pub(crate) async fn workspace(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<Vec<CashLocation>, CashLocationError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let rows = sqlx::query_as::<_, CashLocation>(
        "SELECT l.id::text AS id,l.name,l.location_type,l.station_id::text AS station_id,l.status,b.currency_code,
          COALESCE(sum(e.signed_amount_minor),0)::bigint AS balance_minor,
          COALESCE(bool_or(e.event_type='opening'),false) AS initialized
         FROM hosted_cash_locations l JOIN branches b ON b.tenant_id=l.tenant_id AND b.id=l.branch_id
         LEFT JOIN hosted_cash_location_events e ON e.tenant_id=l.tenant_id AND e.branch_id=l.branch_id AND e.location_id=l.id
         WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid
         GROUP BY l.id,b.currency_code ORDER BY l.created_at,l.id",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(rows)
}

pub(crate) async fn create_location(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: CreateLocationInput,
) -> Result<LocationResult<CashLocation>, CashLocationError> {
    validate_key(key)?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(CashLocationError::InvalidInput);
    }
    let location_type = input.location_type.trim();
    if !matches!(location_type, "register" | "safe" | "petty_cash" | "other") {
        return Err(CashLocationError::InvalidInput);
    }
    let station_id = match (location_type, input.station_id.as_deref()) {
        ("register", Some(value)) => {
            Some(Uuid::parse_str(value).map_err(|_| CashLocationError::InvalidInput)?)
        }
        ("register", None) | (_, Some(_)) => return Err(CashLocationError::InvalidInput),
        (_, None) => None,
    };
    let hash = request_hash(raw);
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    lock_key(&mut tx, "cash-location", tenant, branch, membership, key).await?;
    if let Some((previous_hash, id)) = sqlx::query_as::<_, (Vec<u8>, Uuid)>(
        "SELECT request_hash,id FROM hosted_cash_locations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND created_by=$3::uuid AND idempotency_key=$4",
    )
    .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(database_error)? {
        if previous_hash != hash { return Err(CashLocationError::Conflict); }
        let location = load_location(&mut tx, tenant, branch, id).await?;
        tx.commit().await.map_err(database_error)?;
        return Ok(LocationResult { value: location, replayed: true });
    }
    let id = Uuid::new_v4();
    if let Some(station_id) = station_id {
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stations WHERE id=$1 AND tenant_id=$2::uuid AND branch_id=$3::uuid AND status='active')")
            .bind(station_id).bind(tenant).bind(branch).fetch_one(&mut *tx).await.map_err(database_error)?;
        if !active {
            return Err(CashLocationError::NotFound);
        }
    }
    sqlx::query("INSERT INTO hosted_cash_locations(id,tenant_id,branch_id,name,location_type,station_id,created_by,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7::uuid,$8,$9)")
        .bind(id).bind(tenant).bind(branch).bind(name).bind(location_type).bind(station_id).bind(membership).bind(key).bind(hash)
        .execute(&mut *tx).await.map_err(map_insert_error)?;
    let location = load_location(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(LocationResult {
        value: location,
        replayed: false,
    })
}

pub(crate) async fn record_opening(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    location_id: &str,
    key: &str,
    raw: &[u8],
    input: OpeningInput,
) -> Result<LocationResult<CashLocation>, CashLocationError> {
    validate_key(key)?;
    let location_id = Uuid::parse_str(location_id).map_err(|_| CashLocationError::InvalidInput)?;
    if !(0..=MAX_SAFE_INTEGER).contains(&input.amount_minor) {
        return Err(CashLocationError::InvalidInput);
    }
    let hash = request_hash(raw);
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    lock_location(&mut tx, tenant, branch, location_id).await?;
    let (location_type, station_id): (String, Option<Uuid>) = sqlx::query_as("SELECT location_type,station_id FROM hosted_cash_locations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND status='active'")
        .bind(tenant).bind(branch).bind(location_id).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(CashLocationError::NotFound)?;
    if location_type == "register" {
        let shift_open: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hosted_drawer_shifts WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND station_id=$3 AND status='open')")
            .bind(tenant).bind(branch).bind(station_id).fetch_one(&mut *tx).await.map_err(database_error)?;
        if shift_open {
            return Err(CashLocationError::Conflict);
        }
    }
    if let Some((previous_hash,)) = sqlx::query_as::<_, (Vec<u8>,)>("SELECT request_hash FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3 AND actor_membership_id=$4::uuid AND idempotency_key=$5 AND event_type='opening'")
        .bind(tenant).bind(branch).bind(location_id).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(database_error)? {
        if previous_hash != hash { return Err(CashLocationError::Conflict); }
        let location = load_location(&mut tx, tenant, branch, location_id).await?;
        tx.commit().await.map_err(database_error)?;
        return Ok(LocationResult { value: location, replayed: true });
    }
    let already_opened: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND location_id=$2 AND event_type='opening')")
        .bind(tenant).bind(location_id).fetch_one(&mut *tx).await.map_err(database_error)?;
    let any_events: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND location_id=$2)")
        .bind(tenant).bind(location_id).fetch_one(&mut *tx).await.map_err(database_error)?;
    if already_opened || any_events {
        return Err(CashLocationError::Conflict);
    }
    sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,signed_amount_minor,actor_membership_id,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,$3,'opening',$4,$5::uuid,$6,$7)")
        .bind(tenant).bind(branch).bind(location_id).bind(input.amount_minor).bind(membership).bind(key).bind(hash)
        .execute(&mut *tx).await.map_err(map_insert_error)?;
    let location = load_location(&mut tx, tenant, branch, location_id).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(LocationResult {
        value: location,
        replayed: false,
    })
}

pub(crate) async fn transfer(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: TransferInput,
) -> Result<LocationResult<Uuid>, CashLocationError> {
    validate_key(key)?;
    let from =
        Uuid::parse_str(&input.from_location_id).map_err(|_| CashLocationError::InvalidInput)?;
    let to = Uuid::parse_str(&input.to_location_id).map_err(|_| CashLocationError::InvalidInput)?;
    let note = input.note.trim();
    if from == to
        || !(1..=MAX_SAFE_INTEGER).contains(&input.amount_minor)
        || note.is_empty()
        || note.chars().count() > 240
    {
        return Err(CashLocationError::InvalidInput);
    }
    let hash = request_hash(raw);
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    lock_key(&mut tx, "cash-transfer", tenant, branch, membership, key).await?;
    if let Some((previous_hash, id)) = sqlx::query_as::<_, (Vec<u8>, Uuid)>("SELECT request_hash,id FROM hosted_cash_transfers WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4")
        .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(database_error)? {
        if previous_hash != hash { return Err(CashLocationError::Conflict); }
        tx.commit().await.map_err(database_error)?;
        return Ok(LocationResult { value: id, replayed: true });
    }
    let mut lock_ids = [from, to];
    lock_ids.sort_unstable();
    for id in lock_ids {
        lock_location(&mut tx, tenant, branch, id).await?;
    }
    let locations = sqlx::query_as::<_, (Uuid, String)>("SELECT id,location_type FROM hosted_cash_locations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id IN ($3,$4) AND status='active' ORDER BY id")
        .bind(tenant).bind(branch).bind(from).bind(to).fetch_all(&mut *tx).await.map_err(database_error)?;
    if locations.len() != 2 {
        return Err(CashLocationError::NotFound);
    }
    // Branch-level transfers cannot touch a register. Register movements must
    // remain bound to an active station lease and the drawer's X/Z trail.
    if locations.iter().any(|(_, kind)| kind == "register") {
        return Err(CashLocationError::InvalidInput);
    }
    for id in [from, to] {
        let initialized: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3 AND event_type='opening')")
            .bind(tenant).bind(branch).bind(id).fetch_one(&mut *tx).await.map_err(database_error)?;
        if !initialized {
            return Err(CashLocationError::Conflict);
        }
    }
    let balance: i64 = sqlx::query_scalar("SELECT COALESCE(sum(signed_amount_minor),0)::bigint FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3")
        .bind(tenant).bind(branch).bind(from).fetch_one(&mut *tx).await.map_err(database_error)?;
    let destination_balance: i64 = sqlx::query_scalar("SELECT COALESCE(sum(signed_amount_minor),0)::bigint FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3")
        .bind(tenant).bind(branch).bind(to).fetch_one(&mut *tx).await.map_err(database_error)?;
    if balance < input.amount_minor {
        return Err(CashLocationError::InsufficientFunds);
    }
    if destination_balance
        .checked_add(input.amount_minor)
        .is_none_or(|value| value > MAX_SAFE_INTEGER)
    {
        return Err(CashLocationError::InvalidInput);
    }
    let transfer_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_cash_transfers(id,tenant_id,branch_id,actor_membership_id,from_location_id,to_location_id,amount_minor,note,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5,$6,$7,$8,$9,$10)")
        .bind(transfer_id).bind(tenant).bind(branch).bind(membership).bind(from).bind(to).bind(input.amount_minor).bind(note).bind(key).bind(hash)
        .execute(&mut *tx).await.map_err(map_insert_error)?;
    for (id, event_type, amount) in [
        (from, "transfer_out", -input.amount_minor),
        (to, "transfer_in", input.amount_minor),
    ] {
        sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_transfer_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7::uuid)")
            .bind(tenant).bind(branch).bind(id).bind(event_type).bind(transfer_id).bind(amount).bind(membership)
            .execute(&mut *tx).await.map_err(map_insert_error)?;
    }
    tx.commit().await.map_err(database_error)?;
    Ok(LocationResult {
        value: transfer_id,
        replayed: false,
    })
}

pub(crate) async fn events(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    location_id: &str,
) -> Result<Vec<CashLocationEvent>, CashLocationError> {
    let location_id = Uuid::parse_str(location_id).map_err(|_| CashLocationError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let rows = sqlx::query_as::<_, CashLocationEvent>(
        "SELECT e.id::text AS id,e.event_type,e.signed_amount_minor,e.source_transfer_id::text AS source_transfer_id,e.source_expense_id::text AS source_expense_id,e.source_supplier_payment_id::text AS source_supplier_payment_id,e.source_expense_reimbursement_id::text AS source_expense_reimbursement_id,e.source_sale_tender_id::text AS source_sale_tender_id,e.source_refund_tender_id::text AS source_refund_tender_id,e.source_drawer_movement_id::text AS source_drawer_movement_id,
         COALESCE(NULLIF(i.display_name,''),'Staff') AS actor_name,e.created_at::text AS created_at
         FROM hosted_cash_location_events e LEFT JOIN memberships m ON m.tenant_id=e.tenant_id AND m.id=e.actor_membership_id
         LEFT JOIN person_identities i ON i.id=m.identity_id
         WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid AND e.location_id=$3 ORDER BY e.created_at DESC,e.id DESC LIMIT 500")
        .bind(tenant).bind(branch).bind(location_id).fetch_all(&mut *tx).await.map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(rows)
}

async fn load_location(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: Uuid,
) -> Result<CashLocation, CashLocationError> {
    sqlx::query_as::<_, CashLocation>("SELECT l.id::text AS id,l.name,l.location_type,l.station_id::text AS station_id,l.status,b.currency_code,COALESCE(sum(e.signed_amount_minor),0)::bigint AS balance_minor,COALESCE(bool_or(e.event_type='opening'),false) AS initialized FROM hosted_cash_locations l JOIN branches b ON b.tenant_id=l.tenant_id AND b.id=l.branch_id LEFT JOIN hosted_cash_location_events e ON e.tenant_id=l.tenant_id AND e.branch_id=l.branch_id AND e.location_id=l.id WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.id=$3 GROUP BY l.id,b.currency_code")
        .bind(tenant).bind(branch).bind(id).fetch_optional(&mut **tx).await.map_err(database_error)?.ok_or(CashLocationError::NotFound)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<(), CashLocationError> {
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

async fn lock_key(
    tx: &mut Transaction<'_, Postgres>,
    prefix: &str,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<(), CashLocationError> {
    let value = format!("{prefix}:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(value)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    Ok(())
}

async fn lock_location(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    location: Uuid,
) -> Result<(), CashLocationError> {
    let key = format!("cash-location-balance:{tenant}:{branch}:{location}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(key)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    Ok(())
}

pub(crate) async fn validate_source_outflow(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    location_id: Uuid,
    amount_minor: i64,
) -> Result<(), CashLocationError> {
    lock_location(tx, tenant, branch, location_id).await?;
    let Some((location_type, status)) = sqlx::query_as::<_, (String, String)>(
        "SELECT location_type,status FROM hosted_cash_locations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(location_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)? else {
        return Err(CashLocationError::NotFound);
    };
    if status != "active" || location_type == "register" {
        return Err(CashLocationError::Conflict);
    }
    let initialized: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3 AND event_type='opening')",
    )
    .bind(tenant)
    .bind(branch)
    .bind(location_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if !initialized {
        return Err(CashLocationError::Conflict);
    }
    let balance: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(signed_amount_minor),0)::bigint FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(location_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if balance < amount_minor {
        return Err(CashLocationError::InsufficientFunds);
    }
    Ok(())
}

pub(crate) async fn validate_source_inflow(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    location_id: Uuid,
    currency: &str,
) -> Result<(), CashLocationError> {
    lock_location(tx, tenant, branch, location_id).await?;
    let Some((location_type, status, location_currency)) = sqlx::query_as::<_, (String, String, String)>(
        "SELECT l.location_type,l.status,b.currency_code FROM hosted_cash_locations l JOIN branches b ON b.tenant_id=l.tenant_id AND b.id=l.branch_id WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(location_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)? else {
        return Err(CashLocationError::NotFound);
    };
    if status != "active" || location_type == "register" || location_currency != currency {
        return Err(CashLocationError::Conflict);
    }
    let initialized: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_id=$3 AND event_type='opening')",
    )
    .bind(tenant)
    .bind(branch)
    .bind(location_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if !initialized {
        return Err(CashLocationError::Conflict);
    }
    Ok(())
}

pub(crate) async fn record_expense_outflow(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    expense_id: Uuid,
    location_id: Uuid,
    amount_minor: i64,
) -> Result<(), CashLocationError> {
    sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_expense_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'expense_outflow',$4,-$5,$6::uuid)")
        .bind(tenant)
        .bind(branch)
        .bind(location_id)
        .bind(expense_id)
        .bind(amount_minor)
        .bind(membership)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    Ok(())
}

pub(crate) async fn record_supplier_payment_outflow(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    payment_id: Uuid,
    location_id: Uuid,
    amount_minor: i64,
) -> Result<(), CashLocationError> {
    sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_supplier_payment_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'supplier_payment_outflow',$4,-$5,$6::uuid)")
        .bind(tenant)
        .bind(branch)
        .bind(location_id)
        .bind(payment_id)
        .bind(amount_minor)
        .bind(membership)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    Ok(())
}

pub(crate) async fn record_expense_reimbursement_inflow(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    reimbursement_id: Uuid,
    location_id: Uuid,
    amount_minor: i64,
) -> Result<(), CashLocationError> {
    sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_expense_reimbursement_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'expense_reimbursement_inflow',$4,$5,$6::uuid)")
        .bind(tenant)
        .bind(branch)
        .bind(location_id)
        .bind(reimbursement_id)
        .bind(amount_minor)
        .bind(membership)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    Ok(())
}
fn validate_key(key: &str) -> Result<(), CashLocationError> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        Err(CashLocationError::InvalidInput)
    } else {
        Ok(())
    }
}
fn request_hash(raw: &[u8]) -> Vec<u8> {
    Sha256::digest(raw).to_vec()
}
fn database_error(error: sqlx::Error) -> CashLocationError {
    #[cfg(test)]
    eprintln!("cash-location SQL failed: {error}");
    #[cfg(not(test))]
    let _ = error;
    CashLocationError::Database
}
fn map_insert_error(error: sqlx::Error) -> CashLocationError {
    match &error {
        sqlx::Error::Database(e)
            if e.code().as_deref() == Some("23505") || e.code().as_deref() == Some("23514") =>
        {
            CashLocationError::Conflict
        }
        _ => CashLocationError::Database,
    }
}
