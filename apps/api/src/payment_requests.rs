// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaymentRequestError {
    InvalidInput,
    Conflict,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PaymentRequestInput {
    pub country_code: String,
    pub provider_name: String,
    pub terminal_model: Option<String>,
    pub capability: String,
    pub business_impact: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaymentIntegrationRequest {
    pub id: String,
    pub branch_id: String,
    pub country_code: String,
    pub provider_name: String,
    pub terminal_model: Option<String>,
    pub capability: String,
    pub business_impact: String,
    pub details: Option<String>,
    pub status: String,
    pub customer_visible_update: Option<String>,
    pub revision: i32,
    pub created_at: String,
    pub updated_at: String,
}

fn normalize(input: PaymentRequestInput) -> Result<PaymentRequestInput, PaymentRequestError> {
    let country_code = input.country_code.trim().to_ascii_uppercase();
    let provider_name = input.provider_name.trim().to_owned();
    let terminal_model = input
        .terminal_model
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let capability = input.capability.trim().to_owned();
    let business_impact = input.business_impact.trim().to_owned();
    let details = input
        .details
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if country_code.len() != 2
        || !country_code.bytes().all(|value| value.is_ascii_uppercase())
        || provider_name.is_empty()
        || provider_name.chars().count() > 120
        || terminal_model
            .as_ref()
            .is_some_and(|value| value.chars().count() > 120)
        || capability.is_empty()
        || capability.chars().count() > 100
        || !matches!(
            business_impact.as_str(),
            "business_blocked" | "workaround_available" | "exploring"
        )
        || details
            .as_ref()
            .is_some_and(|value| value.chars().count() > 1000 || contains_sensitive_data(value))
    {
        return Err(PaymentRequestError::InvalidInput);
    }
    Ok(PaymentRequestInput {
        country_code,
        provider_name,
        terminal_model,
        capability,
        business_impact,
        details,
    })
}

fn contains_sensitive_data(value: &str) -> bool {
    let tokens = value
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "password" | "passphrase" | "secret" | "cvv" | "pin"
        )
    }) || tokens.windows(2).any(|pair| {
        matches!(
            (pair[0].as_str(), pair[1].as_str()),
            ("api", "key")
                | ("access", "token")
                | ("refresh", "token")
                | ("card", "number")
                | ("account", "number")
        )
    })
}

fn valid_idempotency_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<(), PaymentRequestError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| PaymentRequestError::Database)?;
    }
    Ok(())
}

const SELECT_COLUMNS: &str =
    "id::text,branch_id::text,country_code,provider_name,terminal_model,capability,business_impact,details,status,customer_visible_update,revision,created_at::text,updated_at::text";

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<Vec<PaymentIntegrationRequest>, PaymentRequestError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let sql = format!("SELECT {SELECT_COLUMNS} FROM hosted_payment_integration_requests WHERE tenant_id=$1::uuid AND branch_id=$2::uuid ORDER BY created_at DESC,id DESC LIMIT 100");
    let requests =
        sqlx::query_as::<_, PaymentIntegrationRequest>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(tenant_id)
            .bind(branch_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|_| PaymentRequestError::Database)?;
    tx.commit()
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    Ok(requests)
}

pub(crate) async fn create(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: PaymentRequestInput,
) -> Result<PaymentIntegrationRequest, PaymentRequestError> {
    let input = normalize(input)?;
    if !valid_idempotency_key(idempotency_key) {
        return Err(PaymentRequestError::InvalidInput);
    }
    let request_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let lock_key = format!("{tenant_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    if let Some((id, existing_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text,request_hash FROM hosted_payment_integration_requests WHERE tenant_id=$1::uuid AND created_by_membership_id=$2::uuid AND idempotency_key=$3",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| PaymentRequestError::Database)?
    {
        if existing_hash != request_hash {
            return Err(PaymentRequestError::Conflict);
        }
        let sql = format!("SELECT {SELECT_COLUMNS} FROM hosted_payment_integration_requests WHERE tenant_id=$1::uuid AND id=$2::uuid");
        let request = sqlx::query_as::<_, PaymentIntegrationRequest>(
            sqlx::AssertSqlSafe(sql.as_str()),
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PaymentRequestError::Database)?;
        tx.commit().await.map_err(|_| PaymentRequestError::Database)?;
        return Ok(request);
    }

    let request_id = Uuid::new_v4();
    let sql = format!("INSERT INTO hosted_payment_integration_requests (id,tenant_id,branch_id,created_by_membership_id,country_code,provider_name,terminal_model,capability,business_impact,details,status,idempotency_key,request_hash) VALUES ($1,$2::uuid,$3::uuid,$4::uuid,$5,$6,$7,$8,$9,$10,'submitted',$11,$12) RETURNING {SELECT_COLUMNS}");
    let request = sqlx::query_as::<_, PaymentIntegrationRequest>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(request_id)
        .bind(tenant_id)
        .bind(branch_id)
        .bind(membership_id)
        .bind(&input.country_code)
        .bind(&input.provider_name)
        .bind(&input.terminal_model)
        .bind(&input.capability)
        .bind(&input.business_impact)
        .bind(&input.details)
        .bind(idempotency_key)
        .bind(request_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'payment_integration_requested','payment_integration_request',$3::uuid,$4)")
        .bind(tenant_id)
        .bind(membership_id)
        .bind(&request.id)
        .bind(serde_json::json!({"branchId":branch_id,"countryCode":input.country_code,"providerName":input.provider_name,"businessImpact":input.business_impact}))
        .execute(&mut *tx)
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    tx.commit()
        .await
        .map_err(|_| PaymentRequestError::Database)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> PaymentRequestInput {
        PaymentRequestInput {
            country_code: " gh ".to_owned(),
            provider_name: " Paystack ".to_owned(),
            terminal_model: Some("  ".to_owned()),
            capability: "  card terminal confirmation  ".to_owned(),
            business_impact: "workaround_available".to_owned(),
            details: Some("Need to confirm approved transactions from my device.".to_owned()),
        }
    }

    #[test]
    fn payment_request_normalizes_values_and_rejects_credentials() {
        let input = normalize(valid_input()).unwrap();
        assert_eq!(input.country_code, "GH");
        assert_eq!(input.provider_name, "Paystack");
        assert_eq!(input.terminal_model, None);
        assert_eq!(input.capability, "card terminal confirmation");

        let mut input = valid_input();
        input.details = Some("terminal PIN is 1234".to_owned());
        assert_eq!(normalize(input), Err(PaymentRequestError::InvalidInput));
    }

    #[test]
    fn payment_request_requires_iso_country_provider_and_capability() {
        let mut input = valid_input();
        input.country_code = "Ghana".to_owned();
        assert_eq!(normalize(input), Err(PaymentRequestError::InvalidInput));
        let mut input = valid_input();
        input.provider_name = " ".to_owned();
        assert_eq!(normalize(input), Err(PaymentRequestError::InvalidInput));
        let mut input = valid_input();
        input.capability = "x".repeat(101);
        assert_eq!(normalize(input), Err(PaymentRequestError::InvalidInput));
        assert!(valid_idempotency_key("integration-request-1"));
        assert!(!valid_idempotency_key("bad\nkey"));
    }
}
