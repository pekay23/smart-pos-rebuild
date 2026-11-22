// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_AMOUNT: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReimbursementError {
    InvalidInput,
    NotFound,
    Conflict,
    InsufficientFunds,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReimbursementInput {
    amount_minor: i64,
    return_method: String,
    cash_location_id: Option<String>,
    external_reference: Option<String>,
    reason: String,
    returned_on: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedExpenseReimbursement {
    pub id: String,
    pub expense_id: String,
    pub amount_minor: i64,
    pub return_method: String,
    pub cash_location_id: Option<String>,
    pub external_reference: Option<String>,
    pub reason: String,
    pub returned_on: String,
    pub recorded_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReimbursementWorkspace {
    pub expense_id: String,
    pub amount_minor: i64,
    pub reimbursed_minor: i64,
    pub remaining_minor: i64,
    pub reimbursements: Vec<HostedExpenseReimbursement>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateReimbursementResult {
    pub reimbursement: HostedExpenseReimbursement,
    pub replayed: bool,
}

pub(crate) async fn workspace(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    expense_id: &str,
) -> Result<ReimbursementWorkspace, ReimbursementError> {
    let expense_id = Uuid::parse_str(expense_id).map_err(|_| ReimbursementError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let amount_minor = sqlx::query_scalar::<_, i64>(
        "SELECT amount_minor FROM hosted_expenses WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(expense_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(ReimbursementError::NotFound)?;
    let reimbursements = list(&mut tx, tenant, branch, expense_id).await?;
    let reimbursed_minor = reimbursements.iter().try_fold(0_i64, |sum, row| {
        sum.checked_add(row.amount_minor)
            .ok_or(ReimbursementError::Database)
    })?;
    tx.commit().await.map_err(database_error)?;
    Ok(ReimbursementWorkspace {
        expense_id: expense_id.to_string(),
        amount_minor,
        reimbursed_minor,
        remaining_minor: amount_minor.saturating_sub(reimbursed_minor),
        reimbursements,
    })
}

pub(crate) async fn create(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    expense_id: &str,
    key: &str,
    raw: &[u8],
    input: ReimbursementInput,
) -> Result<CreateReimbursementResult, ReimbursementError> {
    let expense_id = Uuid::parse_str(expense_id).map_err(|_| ReimbursementError::InvalidInput)?;
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Err(ReimbursementError::InvalidInput);
    }
    let method = input.return_method.trim();
    let reference = input
        .external_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let location_id = input
        .cash_location_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| ReimbursementError::InvalidInput)?;
    let reason = input.reason.trim();
    let returned_on = input.returned_on.trim();
    if !(1..=MAX_AMOUNT).contains(&input.amount_minor)
        || !("cash" == method
            || matches!(method, "card" | "mobile_money" | "bank_transfer" | "other"))
        || reason.chars().count() < 5
        || reason.chars().count() > 500
        || reference.is_some_and(|value| value.chars().count() > 160)
        || !crate::expenses::valid_date(returned_on)
        || (method == "cash" && (location_id.is_none() || reference.is_some()))
        || (method != "cash" && (location_id.is_some() || reference.is_none()))
    {
        return Err(ReimbursementError::InvalidInput);
    }
    let mut hash = Sha256::new();
    hash.update(raw);
    hash.update(b"\nexpense:");
    hash.update(expense_id.as_bytes());
    let request_hash = hash.finalize().to_vec();
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let lock_key = format!("hosted-expense-reimbursement:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(old_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM hosted_expense_reimbursements WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    ).bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(database_error)? {
        if old_hash != request_hash { return Err(ReimbursementError::Conflict); }
        let reimbursement = load_by_key(&mut tx, tenant, branch, membership, key).await?;
        tx.commit().await.map_err(database_error)?;
        return Ok(CreateReimbursementResult { reimbursement, replayed: true });
    }
    let expense_lock = format!("hosted-expense-reimbursements:{tenant}:{branch}:{expense_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(expense_lock)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    let (amount, category, currency) = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT amount_minor,category_code,currency_code FROM hosted_expenses WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
    ).bind(tenant).bind(branch).bind(expense_id).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(ReimbursementError::NotFound)?;
    let already_reimbursed: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(amount_minor),0)::bigint FROM hosted_expense_reimbursements WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND expense_id=$3",
    ).bind(tenant).bind(branch).bind(expense_id).fetch_one(&mut *tx).await.map_err(database_error)?;
    let remaining = amount
        .checked_sub(already_reimbursed)
        .ok_or(ReimbursementError::Database)?;
    if input.amount_minor > remaining {
        return Err(ReimbursementError::InsufficientFunds);
    }
    let branch_currency = sqlx::query_scalar::<_, String>(
        "SELECT currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(ReimbursementError::NotFound)?;
    if branch_currency != currency {
        return Err(ReimbursementError::Conflict);
    }
    if let Some(location_id) = location_id {
        crate::cash_locations::validate_source_inflow(
            &mut tx,
            tenant,
            branch,
            location_id,
            &currency,
        )
        .await
        .map_err(map_location_error)?;
    }
    let effective_at: String = sqlx::query_scalar("SELECT (($1::date::timestamp) AT TIME ZONE timezone)::text FROM branches WHERE tenant_id=$2::uuid AND id=$3::uuid AND status='active'")
        .bind(returned_on).bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(database_error)?.ok_or(ReimbursementError::NotFound)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_expense_reimbursements(id,tenant_id,branch_id,expense_id,currency_code,amount_minor,return_method,cash_location_id,external_reference,reason,returned_on,actor_membership_id,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7,$8,$9,$10,$11::date,$12::uuid,$13,$14)")
        .bind(id).bind(tenant).bind(branch).bind(expense_id).bind(&currency).bind(input.amount_minor).bind(method).bind(location_id).bind(reference).bind(reason).bind(returned_on).bind(membership).bind(key).bind(&request_hash)
        .execute(&mut *tx).await.map_err(map_insert_error)?;
    if let Some(location_id) = location_id {
        crate::cash_locations::record_expense_reimbursement_inflow(
            &mut tx,
            tenant,
            branch,
            membership,
            id,
            location_id,
            input.amount_minor,
        )
        .await
        .map_err(map_location_error)?;
    }
    let expense_account =
        crate::expenses::category_account(&category).ok_or(ReimbursementError::Database)?;
    let payment_account = match method {
        "cash" => 1000,
        "card" => 1010,
        "mobile_money" => 1020,
        "bank_transfer" => 1030,
        "other" => 1090,
        _ => return Err(ReimbursementError::InvalidInput),
    };
    insert_journal(
        &mut tx,
        tenant,
        branch,
        membership,
        id,
        &currency,
        &effective_at,
        expense_account,
        payment_account,
        input.amount_minor,
    )
    .await?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'hosted_expense_reimbursed','expense_reimbursement',$3,$4)")
        .bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"expenseId":expense_id.to_string(),"amountMinor":input.amount_minor,"returnMethod":method,"cashLocationId":location_id.map(|value|value.to_string()),"reference":reference,"reason":reason}))
        .execute(&mut *tx).await.map_err(database_error)?;
    let reimbursement = load_by_key(&mut tx, tenant, branch, membership, key).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(CreateReimbursementResult {
        reimbursement,
        replayed: false,
    })
}

async fn list(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    expense: Uuid,
) -> Result<Vec<HostedExpenseReimbursement>, ReimbursementError> {
    sqlx::query_as::<_, HostedExpenseReimbursement>("SELECT r.id::text,r.expense_id::text,r.amount_minor,r.return_method,r.cash_location_id::text AS cash_location_id,r.external_reference,r.reason,r.returned_on::text AS returned_on,COALESCE(NULLIF(i.display_name,''),'Staff') AS recorded_by,r.created_at::text AS created_at FROM hosted_expense_reimbursements r LEFT JOIN memberships m ON m.tenant_id=r.tenant_id AND m.id=r.actor_membership_id LEFT JOIN person_identities i ON i.id=m.identity_id WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.expense_id=$3 ORDER BY r.returned_on DESC,r.created_at DESC,r.id DESC")
        .bind(tenant).bind(branch).bind(expense).fetch_all(&mut **tx).await.map_err(database_error)
}

async fn load_by_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<HostedExpenseReimbursement, ReimbursementError> {
    sqlx::query_as::<_, HostedExpenseReimbursement>("SELECT r.id::text,r.expense_id::text,r.amount_minor,r.return_method,r.cash_location_id::text AS cash_location_id,r.external_reference,r.reason,r.returned_on::text AS returned_on,COALESCE(NULLIF(i.display_name,''),'Staff') AS recorded_by,r.created_at::text AS created_at FROM hosted_expense_reimbursements r LEFT JOIN memberships m ON m.tenant_id=r.tenant_id AND m.id=r.actor_membership_id LEFT JOIN person_identities i ON i.id=m.identity_id WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.actor_membership_id=$3::uuid AND r.idempotency_key=$4")
        .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut **tx).await.map_err(database_error)?.ok_or(ReimbursementError::NotFound)
}

async fn insert_journal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    id: Uuid,
    currency: &str,
    effective_at: &str,
    expense_account: i32,
    payment_account: i32,
    amount: i64,
) -> Result<(), ReimbursementError> {
    let journal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description) VALUES($1,$2::uuid,$3::uuid,'expense_reimbursement',$4,$5,$6::timestamptz,$7::uuid,'Expense reimbursement received')")
        .bind(journal_id).bind(tenant).bind(branch).bind(id).bind(currency).bind(effective_at).bind(membership).execute(&mut **tx).await.map_err(database_error)?;
    for (line, account, description, debit, credit) in [
        (
            1_i16,
            payment_account,
            "Cash or clearing received",
            amount,
            0_i64,
        ),
        (
            2_i16,
            expense_account,
            "Reduce original expense",
            0_i64,
            amount,
        ),
    ] {
        sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(branch).bind(journal_id).bind(line).bind(format!("{account:04}")).bind(description).bind(debit).bind(credit).execute(&mut **tx).await.map_err(database_error)?;
    }
    Ok(())
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<(), ReimbursementError> {
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

fn map_location_error(error: crate::cash_locations::CashLocationError) -> ReimbursementError {
    match error {
        crate::cash_locations::CashLocationError::InvalidInput => ReimbursementError::InvalidInput,
        crate::cash_locations::CashLocationError::NotFound => ReimbursementError::NotFound,
        crate::cash_locations::CashLocationError::Conflict => ReimbursementError::Conflict,
        crate::cash_locations::CashLocationError::CutoverBlocked => ReimbursementError::Conflict,
        crate::cash_locations::CashLocationError::InsufficientFunds => {
            ReimbursementError::InsufficientFunds
        }
        crate::cash_locations::CashLocationError::Database => ReimbursementError::Database,
    }
}
fn map_insert_error(error: sqlx::Error) -> ReimbursementError {
    if let sqlx::Error::Database(database) = &error {
        if database.code().as_deref() == Some("23505") {
            return ReimbursementError::Conflict;
        }
    }
    database_error(error)
}
fn database_error(error: sqlx::Error) -> ReimbursementError {
    #[cfg(test)]
    eprintln!("expense reimbursement SQL failed: {error}");
    #[cfg(not(test))]
    let _ = error;
    ReimbursementError::Database
}
