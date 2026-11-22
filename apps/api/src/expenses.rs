// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpenseError {
    InvalidInput,
    NotFound,
    IdempotencyConflict,
    DrawerProof,
    DrawerLease,
    DrawerConflict,
    CashLocationUnavailable,
    CashLocationInsufficientFunds,
    CashLocationConflict,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExpenseInput {
    category_code: String,
    description: String,
    payee: Option<String>,
    amount_minor: i64,
    payment_method: String,
    external_reference: Option<String>,
    occurred_on: String,
    #[serde(default)]
    drawer_shift_id: Option<String>,
    #[serde(default)]
    cash_location_id: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct DrawerExpenseProof<'a> {
    pub station_id: &'a str,
    pub lease_id: &'a str,
    pub lease_token: &'a str,
    pub challenge_id: &'a str,
    pub proof_hex: &'a str,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExpenseQuery {
    pub offset: Option<i64>,
    pub page_size: Option<i64>,
}

impl ExpenseQuery {
    pub(crate) fn page(self) -> Result<(i64, i64), ExpenseError> {
        let offset = self.offset.unwrap_or(0);
        let page_size = self.page_size.unwrap_or(50);
        if !(0..=100_000).contains(&offset) || !(1..=100).contains(&page_size) {
            return Err(ExpenseError::InvalidInput);
        }
        Ok((offset, page_size))
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedExpense {
    pub id: String,
    pub category_code: String,
    pub description: String,
    pub payee: Option<String>,
    pub amount_minor: i64,
    pub reimbursed_minor: i64,
    pub remaining_reimbursable_minor: i64,
    pub payment_method: String,
    pub drawer_shift_id: Option<String>,
    pub cash_location_id: Option<String>,
    pub external_reference: Option<String>,
    pub occurred_on: String,
    pub currency_code: String,
    pub cost_center_kind: String,
    pub cost_center_station_id: Option<String>,
    pub cost_center_station_name_snapshot: Option<String>,
    pub cost_center_revision: i64,
    pub recorded_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExpenseWorkspace {
    pub currency_code: String,
    pub expenses: Vec<HostedExpense>,
    pub offset: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateExpenseResult {
    #[serde(flatten)]
    pub expense: HostedExpense,
    pub replayed: bool,
}

pub(crate) async fn workspace(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    offset: i64,
    page_size: i64,
) -> Result<ExpenseWorkspace, ExpenseError> {
    if !(0..=100_000).contains(&offset) || !(1..=100).contains(&page_size) {
        return Err(ExpenseError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(&mut tx, identity, tenant, membership, branch, None).await?;
    let currency_code = sqlx::query_scalar::<_, String>(
        "SELECT currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(ExpenseError::NotFound)?;
    let mut rows = sqlx::query_as::<_, HostedExpense>(
        "SELECT e.id::text AS id,e.category_code,e.description,e.payee,e.amount_minor,COALESCE(r.amount_minor,0)::bigint AS reimbursed_minor,GREATEST(e.amount_minor-COALESCE(r.amount_minor,0),0)::bigint AS remaining_reimbursable_minor,e.payment_method,e.drawer_shift_id::text AS drawer_shift_id,e.cash_location_id::text AS cash_location_id,e.external_reference,e.occurred_on::text AS occurred_on,e.currency_code,COALESCE(cc.next_kind,'legacy_unassigned') AS cost_center_kind,cc.next_station_id::text AS cost_center_station_id,cc.next_station_name_snapshot AS cost_center_station_name_snapshot,COALESCE(cc.revision,0)::bigint AS cost_center_revision,COALESCE(NULLIF(i.display_name,''),'Staff') AS recorded_by,e.created_at::text AS created_at
         FROM hosted_expenses e LEFT JOIN memberships m ON m.tenant_id=e.tenant_id AND m.id=e.actor_membership_id
         LEFT JOIN LATERAL (SELECT sum(amount_minor) AS amount_minor FROM hosted_expense_reimbursements r WHERE r.tenant_id=e.tenant_id AND r.branch_id=e.branch_id AND r.expense_id=e.id) r ON true
         LEFT JOIN LATERAL (SELECT next_kind,next_station_id,next_station_name_snapshot,revision FROM hosted_expense_cost_center_reclassifications cc WHERE cc.tenant_id=e.tenant_id AND cc.branch_id=e.branch_id AND cc.expense_id=e.id ORDER BY revision DESC LIMIT 1) cc ON true
         LEFT JOIN person_identities i ON i.id=m.identity_id
         WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid
         ORDER BY e.occurred_on DESC,e.created_at DESC,e.id DESC LIMIT $3 OFFSET $4",
    )
    .bind(tenant)
    .bind(branch)
    .bind(page_size + 1)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let has_more = rows.len() > page_size as usize;
    rows.truncate(page_size as usize);
    tx.commit().await.map_err(database_error)?;
    Ok(ExpenseWorkspace {
        currency_code,
        expenses: rows,
        offset,
        page_size,
        has_more,
    })
}

pub(crate) async fn create(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: ExpenseInput,
    drawer_proof: Option<DrawerExpenseProof<'_>>,
) -> Result<CreateExpenseResult, ExpenseError> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(ExpenseError::InvalidInput);
    }
    let category = input.category_code.trim();
    let category_account = category_account(category).ok_or(ExpenseError::InvalidInput)?;
    let description = input.description.trim();
    let payee = input
        .payee
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reference = input
        .external_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let method = input.payment_method.trim();
    let payment_account = payment_account(method).ok_or(ExpenseError::InvalidInput)?;
    let drawer_shift = input.drawer_shift_id.as_deref();
    let cash_location_id = input
        .cash_location_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| ExpenseError::InvalidInput)?;
    match (method, drawer_shift, cash_location_id, drawer_proof) {
        ("cash", Some(shift), None, Some(proof))
            if Uuid::parse_str(shift).is_ok() && Uuid::parse_str(proof.station_id).is_ok() => {}
        ("cash", None, Some(_), None) => {}
        ("cash", _, _, _) => return Err(ExpenseError::InvalidInput),
        (_, None, None, None) => {}
        _ => return Err(ExpenseError::InvalidInput),
    }
    if description.is_empty()
        || description.chars().count() > 240
        || payee.is_some_and(|value| value.chars().count() > 160)
        || reference.is_some_and(|value| value.chars().count() > 160)
        || !(1..=MAX_SAFE_INTEGER).contains(&input.amount_minor)
        || !valid_date(&input.occurred_on)
    {
        return Err(ExpenseError::InvalidInput);
    }
    let occurred_on = input.occurred_on;
    let mut hasher = Sha256::new();
    hasher.update(raw);
    hasher.update(b"\nbranch:");
    hasher.update(branch.as_bytes());
    if let Some(proof) = drawer_proof {
        hasher.update(b"\nstation:");
        hasher.update(proof.station_id.as_bytes());
    }
    let request_hash = hasher.finalize().to_vec();

    let mut tx = pool.begin().await.map_err(database_error)?;
    set_context(
        &mut tx,
        identity,
        tenant,
        membership,
        branch,
        drawer_proof.map(|proof| proof.station_id),
    )
    .await?;
    let lock_key = format!("hosted-expense:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(previous) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM hosted_expenses WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4",
    )
    .bind(tenant)
    .bind(branch)
    .bind(membership)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    {
        if previous != request_hash {
            return Err(ExpenseError::IdempotencyConflict);
        }
        let expense = load_by_key(&mut tx, tenant, branch, membership, key).await?;
        if let Some(proof) = drawer_proof {
            crate::drawer::record_expense_outflow(
                &mut tx,
                identity,
                membership,
                tenant,
                branch,
                proof.station_id,
                drawer_shift.expect("cash validation requires shift"),
                proof.lease_id,
                proof.lease_token,
                key,
                proof.challenge_id,
                proof.proof_hex,
                raw,
                Uuid::parse_str(&expense.id).map_err(|_| ExpenseError::Database)?,
                input.amount_minor,
            )
            .await
            .map_err(map_drawer_error)?;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(CreateExpenseResult {
            expense,
            replayed: true,
        });
    }

    let (currency_code, timezone) = sqlx::query_as::<_, (String, String)>(
        "SELECT currency_code,timezone FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database_error)?
    .ok_or(ExpenseError::NotFound)?;
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
    let id = Uuid::new_v4();
    let (created_at, effective_at) = sqlx::query_as::<_, (String, String)>(
        "INSERT INTO hosted_expenses(id,tenant_id,branch_id,currency_code,category_code,description,payee,amount_minor,payment_method,drawer_shift_id,cash_location_id,external_reference,occurred_on,actor_membership_id,idempotency_key,request_hash)
         VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7,$8,$9,$10::uuid,$11,$12,$13::date,$14::uuid,$15,$16)
         RETURNING created_at::text,((occurred_on::timestamp) AT TIME ZONE $17)::text",
    )
    .bind(id)
    .bind(tenant)
    .bind(branch)
    .bind(&currency_code)
    .bind(category)
    .bind(description)
    .bind(payee)
    .bind(input.amount_minor)
    .bind(method)
    .bind(drawer_shift)
    .bind(cash_location_id)
    .bind(reference)
    .bind(&occurred_on)
    .bind(membership)
    .bind(key)
    .bind(&request_hash)
    .bind(timezone)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_insert_error)?;
    insert_journal(
        &mut tx,
        tenant,
        branch,
        membership,
        id,
        &currency_code,
        &effective_at,
        category,
        category_account,
        payment_account,
        input.amount_minor,
    )
    .await?;
    if let Some(location_id) = cash_location_id {
        crate::cash_locations::record_expense_outflow(
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
    if let Some(proof) = drawer_proof {
        crate::drawer::record_expense_outflow(
            &mut tx,
            identity,
            membership,
            tenant,
            branch,
            proof.station_id,
            drawer_shift.expect("cash validation requires shift"),
            proof.lease_id,
            proof.lease_token,
            key,
            proof.challenge_id,
            proof.proof_hex,
            raw,
            id,
            input.amount_minor,
        )
        .await
        .map_err(map_drawer_error)?;
    }
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'hosted_expense_recorded','expense',$3,$4)")
        .bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"categoryCode":category,"paymentMethod":input.payment_method,"cashLocationId":cash_location_id.map(|value|value.to_string()),"amountMinor":input.amount_minor,"occurredOn":occurred_on}))
        .execute(&mut *tx).await.map_err(database_error)?;
    let expense = load_by_key(&mut tx, tenant, branch, membership, key).await?;
    let _ = created_at;
    tx.commit().await.map_err(database_error)?;
    Ok(CreateExpenseResult {
        expense,
        replayed: false,
    })
}

async fn insert_journal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    expense_id: Uuid,
    currency: &str,
    effective_at: &str,
    category: &str,
    expense_account: i32,
    payment_account: i32,
    amount: i64,
) -> Result<(), ExpenseError> {
    let journal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description) VALUES($1,$2::uuid,$3::uuid,'expense',$4,$5,$6::timestamptz,$7::uuid,$8)")
        .bind(journal_id).bind(tenant).bind(branch).bind(expense_id).bind(currency).bind(effective_at).bind(membership).bind(format!("Expense: {category}"))
        .execute(&mut **tx).await.map_err(database_error)?;
    for (line_number, account, description, debit, credit) in [
        (1_i16, expense_account, "Expense", amount, 0_i64),
        (2_i16, payment_account, "Payment clearing", 0_i64, amount),
    ] {
        sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(branch).bind(journal_id).bind(line_number).bind(format!("{account:04}")).bind(description)
            .bind(debit).bind(credit).execute(&mut **tx).await.map_err(database_error)?;
    }
    Ok(())
}

async fn load_by_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<HostedExpense, ExpenseError> {
    sqlx::query_as::<_, HostedExpense>(
        "SELECT e.id::text AS id,e.category_code,e.description,e.payee,e.amount_minor,COALESCE(r.amount_minor,0)::bigint AS reimbursed_minor,GREATEST(e.amount_minor-COALESCE(r.amount_minor,0),0)::bigint AS remaining_reimbursable_minor,e.payment_method,e.drawer_shift_id::text AS drawer_shift_id,e.cash_location_id::text AS cash_location_id,e.external_reference,e.occurred_on::text AS occurred_on,e.currency_code,COALESCE(cc.next_kind,'legacy_unassigned') AS cost_center_kind,cc.next_station_id::text AS cost_center_station_id,cc.next_station_name_snapshot AS cost_center_station_name_snapshot,COALESCE(cc.revision,0)::bigint AS cost_center_revision,COALESCE(NULLIF(i.display_name,''),'Staff') AS recorded_by,e.created_at::text AS created_at
         FROM hosted_expenses e LEFT JOIN memberships m ON m.tenant_id=e.tenant_id AND m.id=e.actor_membership_id
         LEFT JOIN LATERAL (SELECT sum(amount_minor) AS amount_minor FROM hosted_expense_reimbursements r WHERE r.tenant_id=e.tenant_id AND r.branch_id=e.branch_id AND r.expense_id=e.id) r ON true
         LEFT JOIN LATERAL (SELECT next_kind,next_station_id,next_station_name_snapshot,revision FROM hosted_expense_cost_center_reclassifications cc WHERE cc.tenant_id=e.tenant_id AND cc.branch_id=e.branch_id AND cc.expense_id=e.id ORDER BY revision DESC LIMIT 1) cc ON true
         LEFT JOIN person_identities i ON i.id=m.identity_id
         WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid AND e.actor_membership_id=$3::uuid AND e.idempotency_key=$4",
    )
    .bind(tenant)
    .bind(branch)
    .bind(membership)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .ok_or(ExpenseError::NotFound)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    station: Option<&str>,
) -> Result<(), ExpenseError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", branch),
        ("pos.station_id", station.unwrap_or("")),
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

pub(crate) fn category_account(category: &str) -> Option<i32> {
    Some(match category {
        "rent" => 5100,
        "utilities" => 5200,
        "wages" => 5300,
        "transport" => 5400,
        "supplies" => 5500,
        "maintenance" => 5600,
        "marketing" => 5700,
        "bank_fees" => 5800,
        "taxes_fees" => 5900,
        "other" => 6000,
        _ => return None,
    })
}

fn payment_account(method: &str) -> Option<i32> {
    Some(match method {
        "card" => 1010,
        "mobile_money" => 1020,
        "bank_transfer" => 1030,
        "other" => 1090,
        "cash" => 1000,
        _ => return None,
    })
}

fn map_drawer_error(error: crate::drawer::DrawerError) -> ExpenseError {
    match error {
        crate::drawer::DrawerError::InvalidProof => ExpenseError::DrawerProof,
        crate::drawer::DrawerError::LeaseUnavailable => ExpenseError::DrawerLease,
        crate::drawer::DrawerError::IdempotencyConflict => ExpenseError::IdempotencyConflict,
        crate::drawer::DrawerError::NoOpenShift
        | crate::drawer::DrawerError::ShiftUnavailable
        | crate::drawer::DrawerError::ShiftAlreadyOpen
        | crate::drawer::DrawerError::RegisterCutoverRequired => ExpenseError::DrawerConflict,
        _ => ExpenseError::Database,
    }
}

fn map_cash_location_error(error: crate::cash_locations::CashLocationError) -> ExpenseError {
    match error {
        crate::cash_locations::CashLocationError::InvalidInput => ExpenseError::InvalidInput,
        crate::cash_locations::CashLocationError::NotFound => ExpenseError::CashLocationUnavailable,
        crate::cash_locations::CashLocationError::Conflict => ExpenseError::CashLocationConflict,
        crate::cash_locations::CashLocationError::CutoverBlocked => {
            ExpenseError::CashLocationConflict
        }
        crate::cash_locations::CashLocationError::InsufficientFunds => {
            ExpenseError::CashLocationInsufficientFunds
        }
        crate::cash_locations::CashLocationError::Database => ExpenseError::Database,
    }
}

pub(crate) fn valid_date(value: &str) -> bool {
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

fn database_error(error: sqlx::Error) -> ExpenseError {
    #[cfg(test)]
    eprintln!("hosted expense SQL failed: {error}");
    #[cfg(not(test))]
    let _ = error;
    ExpenseError::Database
}

fn map_insert_error(error: sqlx::Error) -> ExpenseError {
    if error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation())
    {
        ExpenseError::IdempotencyConflict
    } else {
        database_error(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{category_account, payment_account, valid_date, ExpenseQuery};

    #[test]
    fn validates_date_category_and_payment_method_domain() {
        assert!(valid_date("2024-02-29"));
        assert!(!valid_date("2025-02-29"));
        assert!(!valid_date("2026-04-31"));
        assert_eq!(category_account("utilities"), Some(5200));
        assert_eq!(category_account("other"), Some(6000));
        assert_eq!(category_account("unknown"), None);
        assert_eq!(payment_account("bank_transfer"), Some(1030));
        assert_eq!(payment_account("cash"), Some(1000));
    }

    #[test]
    fn validates_expense_page_bounds() {
        assert_eq!(ExpenseQuery::default().page(), Ok((0, 50)));
        assert!(ExpenseQuery {
            offset: Some(-1),
            page_size: None
        }
        .page()
        .is_err());
        assert!(ExpenseQuery {
            offset: None,
            page_size: Some(101)
        }
        .page()
        .is_err());
    }
}
