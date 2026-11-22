// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreditTermError {
    InvalidInput,
    NotFound,
    Conflict,
    Database,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomerCreditTerm {
    pub currency_code: String,
    pub credit_limit_minor: String,
    pub due_days: i32,
    pub revision: i64,
    pub reason: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomerCreditTermEvent {
    pub currency_code: String,
    pub revision: i64,
    pub previous_credit_limit_minor: Option<String>,
    pub previous_due_days: Option<i32>,
    pub credit_limit_minor: String,
    pub due_days: i32,
    pub reason: String,
    pub actor_name: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetCustomerCreditTermInput {
    pub expected_revision: i64,
    pub credit_limit_minor: i64,
    pub due_days: i32,
    pub reason: String,
}

const MAX_CREDIT_LIMIT: i64 = 9_007_199_254_740_991;

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    customer_id: &str,
) -> Result<Vec<CustomerCreditTerm>, CreditTermError> {
    let customer = Uuid::parse_str(customer_id).map_err(|_| CreditTermError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| CreditTermError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id).await?;
    let active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2 AND status='active')",
    )
    .bind(tenant_id)
    .bind(customer)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    if !active {
        return Err(CreditTermError::NotFound);
    }
    let rows = sqlx::query_as::<_, CustomerCreditTerm>(
        "SELECT currency_code::text,credit_limit_minor::text,due_days,revision,last_reason AS reason,
                to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
         FROM hosted_customer_credit_terms WHERE tenant_id=$1::uuid AND customer_id=$2
         ORDER BY currency_code",
    )
    .bind(tenant_id)
    .bind(customer)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    tx.commit().await.map_err(|_| CreditTermError::Database)?;
    Ok(rows)
}

pub(crate) async fn history(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    customer_id: &str,
    currency_code: &str,
) -> Result<Vec<CustomerCreditTermEvent>, CreditTermError> {
    let customer = Uuid::parse_str(customer_id).map_err(|_| CreditTermError::InvalidInput)?;
    let currency = currency_code.trim().to_ascii_uppercase();
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(CreditTermError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| CreditTermError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id).await?;
    sqlx::query("SELECT set_config('pos.staff_roster_tenant_id',$1,true)")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| CreditTermError::Database)?;
    let active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2 AND status='active')",
    )
    .bind(tenant_id)
    .bind(customer)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    if !active {
        return Err(CreditTermError::NotFound);
    }
    let rows = sqlx::query_as::<_, CustomerCreditTermEvent>(
        "SELECT e.currency_code::text,e.revision,e.previous_credit_limit_minor::text,
                e.previous_due_days,e.credit_limit_minor::text,e.due_days,e.reason,
                COALESCE(NULLIF(btrim(i.display_name),''),'Staff') AS actor_name,
                to_char(e.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at
         FROM hosted_customer_credit_term_events e
         LEFT JOIN memberships m ON m.tenant_id=e.tenant_id AND m.id=e.actor_membership_id
         LEFT JOIN person_identities i ON i.id=m.identity_id
         WHERE e.tenant_id=$1::uuid AND e.customer_id=$2 AND e.currency_code=$3
         ORDER BY e.revision DESC LIMIT 100",
    )
    .bind(tenant_id)
    .bind(customer)
    .bind(currency)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    tx.commit().await.map_err(|_| CreditTermError::Database)?;
    Ok(rows)
}

pub(crate) async fn set(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    customer_id: &str,
    currency_code: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: SetCustomerCreditTermInput,
) -> Result<CustomerCreditTerm, CreditTermError> {
    let customer = Uuid::parse_str(customer_id).map_err(|_| CreditTermError::InvalidInput)?;
    let currency = currency_code.trim().to_ascii_uppercase();
    let reason = input.reason.trim();
    if currency.len() != 3
        || !currency.bytes().all(|byte| byte.is_ascii_uppercase())
        || input.expected_revision < 0
        || input.credit_limit_minor < 0
        || input.credit_limit_minor > MAX_CREDIT_LIMIT
        || !(0..=365).contains(&input.due_days)
        || reason.is_empty()
        || reason.chars().count() > 500
        || !valid_key(idempotency_key)
    {
        return Err(CreditTermError::InvalidInput);
    }
    let hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| CreditTermError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id).await?;
    let lock_key = format!("{tenant_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| CreditTermError::Database)?;

    if let Some((old_hash, currency_code, credit_limit_minor, due_days, revision, reason, updated_at)) = sqlx::query_as::<_, (Vec<u8>, String, String, i32, i64, String, String)>(
        "SELECT e.request_hash,e.currency_code::text,e.credit_limit_minor::text,e.due_days,e.revision,e.reason,
                to_char(e.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         FROM hosted_customer_credit_term_events e
         WHERE e.tenant_id=$1::uuid AND e.customer_id=$2 AND e.currency_code=$3
           AND e.actor_membership_id=$4::uuid AND e.idempotency_key=$5",
    )
    .bind(tenant_id)
    .bind(customer)
    .bind(&currency)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?
    {
        if old_hash != hash {
            return Err(CreditTermError::Conflict);
        }
        tx.commit().await.map_err(|_| CreditTermError::Database)?;
        return Ok(CustomerCreditTerm { currency_code, credit_limit_minor, due_days, revision, reason, updated_at });
    }

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "customer-credit-terms:{tenant_id}:{customer}:{currency}"
        ))
        .execute(&mut *tx)
        .await
        .map_err(|_| CreditTermError::Database)?;

    let customer_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2 AND status='active')",
    )
    .bind(tenant_id)
    .bind(customer)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    if !customer_active {
        return Err(CreditTermError::NotFound);
    }

    let previous = sqlx::query_as::<_, (i64, i32, i64)>(
        "SELECT credit_limit_minor,due_days,revision FROM hosted_customer_credit_terms
         WHERE tenant_id=$1::uuid AND customer_id=$2 AND currency_code=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(customer)
    .bind(&currency)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;

    let (revision, previous_limit, previous_due) = match (input.expected_revision, previous) {
        (0, None) => {
            let inserted = sqlx::query(
                "INSERT INTO hosted_customer_credit_terms
                 (tenant_id,customer_id,currency_code,credit_limit_minor,due_days,revision,last_reason,updated_by_membership_id)
                 VALUES($1::uuid,$2,$3,$4,$5,1,$6,$7::uuid)",
            )
            .bind(tenant_id)
            .bind(customer)
            .bind(&currency)
            .bind(input.credit_limit_minor)
            .bind(input.due_days)
            .bind(reason)
            .bind(membership_id)
            .execute(&mut *tx)
            .await;
            if let Err(error) = inserted {
                return Err(
                    if error
                        .as_database_error()
                        .is_some_and(|db| db.is_unique_violation())
                    {
                        CreditTermError::Conflict
                    } else {
                        CreditTermError::Database
                    },
                );
            }
            (1, None, None)
        }
        (expected, Some((old_limit, old_due, old_revision))) if expected == old_revision => {
            if input.credit_limit_minor == old_limit && input.due_days == old_due {
                return Err(CreditTermError::InvalidInput);
            }
            let revision = old_revision
                .checked_add(1)
                .ok_or(CreditTermError::Database)?;
            sqlx::query(
                "UPDATE hosted_customer_credit_terms SET credit_limit_minor=$4,due_days=$5,
                 revision=$6,last_reason=$7,updated_by_membership_id=$8::uuid,updated_at=now()
                 WHERE tenant_id=$1::uuid AND customer_id=$2 AND currency_code=$3 AND revision=$9",
            )
            .bind(tenant_id)
            .bind(customer)
            .bind(&currency)
            .bind(input.credit_limit_minor)
            .bind(input.due_days)
            .bind(revision)
            .bind(reason)
            .bind(membership_id)
            .bind(old_revision)
            .execute(&mut *tx)
            .await
            .map_err(|_| CreditTermError::Database)?;
            (revision, Some(old_limit), Some(old_due))
        }
        _ => return Err(CreditTermError::Conflict),
    };

    sqlx::query(
        "INSERT INTO hosted_customer_credit_term_events
         (tenant_id,customer_id,currency_code,revision,previous_credit_limit_minor,previous_due_days,
          credit_limit_minor,due_days,reason,actor_membership_id,idempotency_key,request_hash)
         VALUES($1::uuid,$2,$3,$4,$5,$6,$7,$8,$9,$10::uuid,$11,$12)",
    )
    .bind(tenant_id)
    .bind(customer)
    .bind(&currency)
    .bind(revision)
    .bind(previous_limit)
    .bind(previous_due)
    .bind(input.credit_limit_minor)
    .bind(input.due_days)
    .bind(reason)
    .bind(membership_id)
    .bind(idempotency_key)
    .bind(&hash)
    .execute(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    sqlx::query(
        "INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata)
         VALUES($1::uuid,$2::uuid,'customer_credit_terms_changed','customer',$3,
                jsonb_build_object('currencyCode',$4,'revision',$5,'creditLimitMinor',$6,'dueDays',$7))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(customer)
    .bind(&currency)
    .bind(revision)
    .bind(input.credit_limit_minor)
    .bind(input.due_days)
    .execute(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    let row = sqlx::query_as::<_, CustomerCreditTerm>(
        "SELECT currency_code::text,credit_limit_minor::text,due_days,revision,last_reason AS reason,
                to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at
         FROM hosted_customer_credit_terms WHERE tenant_id=$1::uuid AND customer_id=$2 AND currency_code=$3",
    )
    .bind(tenant_id)
    .bind(customer)
    .bind(&currency)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CreditTermError::Database)?;
    tx.commit().await.map_err(|_| CreditTermError::Database)?;
    Ok(row)
}

fn valid_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
) -> Result<(), CreditTermError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", ""),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| CreditTermError::Database)?;
    }
    Ok(())
}
