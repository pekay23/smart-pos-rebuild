// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportRequestError {
    InvalidInput,
    Conflict,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SupportRequestInput {
    pub category: String,
    pub title: String,
    pub details: Option<String>,
    #[serde(default)]
    pub diagnostics_consent: bool,
    pub diagnostics_app_version: Option<String>,
    pub diagnostics_platform: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupportRequest {
    pub id: String,
    pub branch_id: String,
    pub category: String,
    pub title: String,
    pub details: Option<String>,
    pub diagnostics_consent: bool,
    pub diagnostics_app_version: Option<String>,
    pub diagnostics_platform: Option<String>,
    pub status: String,
    pub customer_visible_update: Option<String>,
    pub revision: i32,
    pub created_at: String,
    pub updated_at: String,
}

fn normalize(input: SupportRequestInput) -> Result<SupportRequestInput, SupportRequestError> {
    let category = input.category.trim().to_ascii_lowercase();
    let title = input.title.trim().to_owned();
    let details = input
        .details
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let diagnostics_valid = if input.diagnostics_consent {
        input
            .diagnostics_app_version
            .as_deref()
            .is_some_and(valid_app_version)
            && input
                .diagnostics_platform
                .as_deref()
                .is_some_and(valid_platform)
    } else {
        input.diagnostics_app_version.is_none() && input.diagnostics_platform.is_none()
    };
    if !matches!(
        category.as_str(),
        "bug" | "feature_request" | "account_access" | "data_issue" | "training" | "other"
    ) || !(3..=120).contains(&title.chars().count())
        || contains_sensitive_data(&title)
        || details
            .as_ref()
            .is_some_and(|value| value.chars().count() > 2000 || contains_sensitive_data(value))
        || !diagnostics_valid
    {
        return Err(SupportRequestError::InvalidInput);
    }
    Ok(SupportRequestInput {
        category,
        title,
        details,
        diagnostics_consent: input.diagnostics_consent,
        diagnostics_app_version: input.diagnostics_app_version,
        diagnostics_platform: input.diagnostics_platform,
    })
}

fn valid_app_version(value: &str) -> bool {
    (1..=40).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte))
}

fn valid_platform(value: &str) -> bool {
    matches!(
        value,
        "web" | "windows" | "macos" | "linux" | "android" | "ios" | "harmonyos"
    )
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
) -> Result<(), SupportRequestError> {
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
            .map_err(|_| SupportRequestError::Database)?;
    }
    Ok(())
}

const SELECT_COLUMNS: &str = "id::text,branch_id::text,category,title,details,(diagnostics_consent_at IS NOT NULL) AS diagnostics_consent,diagnostics_app_version,diagnostics_platform,status,customer_visible_update,revision,created_at::text,updated_at::text";

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<Vec<SupportRequest>, SupportRequestError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| SupportRequestError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM hosted_support_requests WHERE tenant_id=$1::uuid AND branch_id=$2::uuid ORDER BY created_at DESC,id DESC LIMIT 100"
    );
    let requests = sqlx::query_as::<_, SupportRequest>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant_id)
        .bind(branch_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| SupportRequestError::Database)?;
    tx.commit()
        .await
        .map_err(|_| SupportRequestError::Database)?;
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
    input: SupportRequestInput,
) -> Result<SupportRequest, SupportRequestError> {
    let input = normalize(input)?;
    if !valid_idempotency_key(idempotency_key) {
        return Err(SupportRequestError::InvalidInput);
    }
    let request_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| SupportRequestError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let lock_key = format!("support-request:{tenant_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| SupportRequestError::Database)?;
    if let Some((id, existing_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text,request_hash FROM hosted_support_requests WHERE tenant_id=$1::uuid AND created_by_membership_id=$2::uuid AND idempotency_key=$3",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SupportRequestError::Database)?
    {
        if existing_hash != request_hash {
            return Err(SupportRequestError::Conflict);
        }
        let sql = format!(
            "SELECT {SELECT_COLUMNS} FROM hosted_support_requests WHERE tenant_id=$1::uuid AND id=$2::uuid"
        );
        let request = sqlx::query_as::<_, SupportRequest>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(tenant_id)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| SupportRequestError::Database)?;
        tx.commit().await.map_err(|_| SupportRequestError::Database)?;
        return Ok(request);
    }

    let sql = format!(
        "INSERT INTO hosted_support_requests (tenant_id,branch_id,created_by_membership_id,category,title,details,diagnostics_consent_at,diagnostics_app_version,diagnostics_platform,status,revision,idempotency_key,request_hash) VALUES ($1::uuid,$2::uuid,$3::uuid,$4,$5,$6,CASE WHEN $7 THEN now() ELSE NULL END,$8,$9,'submitted',1,$10,$11) RETURNING {SELECT_COLUMNS}"
    );
    let request = sqlx::query_as::<_, SupportRequest>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant_id)
        .bind(branch_id)
        .bind(membership_id)
        .bind(&input.category)
        .bind(&input.title)
        .bind(&input.details)
        .bind(input.diagnostics_consent)
        .bind(&input.diagnostics_app_version)
        .bind(&input.diagnostics_platform)
        .bind(idempotency_key)
        .bind(request_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| SupportRequestError::Database)?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'support_requested','support_request',$3::uuid,$4)")
        .bind(tenant_id)
        .bind(membership_id)
        .bind(&request.id)
        .bind(serde_json::json!({"branchId":branch_id,"category":input.category}))
        .execute(&mut *tx)
        .await
        .map_err(|_| SupportRequestError::Database)?;
    tx.commit()
        .await
        .map_err(|_| SupportRequestError::Database)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> SupportRequestInput {
        SupportRequestInput {
            category: "bug".to_owned(),
            title: "Register freezes after holding a sale".to_owned(),
            details: Some("The issue started after reconnecting the scanner.".to_owned()),
            diagnostics_consent: false,
            diagnostics_app_version: None,
            diagnostics_platform: None,
        }
    }

    #[test]
    fn support_request_normalizes_and_rejects_sensitive_details() {
        let mut input = valid_input();
        input.category = " Feature_Request ".to_owned();
        input.title = "  Add a second receipt layout  ".to_owned();
        assert_eq!(
            normalize(input.clone()).unwrap().category,
            "feature_request"
        );
        assert_eq!(
            normalize(input).unwrap().title,
            "Add a second receipt layout"
        );
        let mut input = valid_input();
        input.details = Some("The API key is abc123".to_owned());
        assert_eq!(normalize(input), Err(SupportRequestError::InvalidInput));
        assert!(valid_idempotency_key("support-request-1"));
        assert!(!valid_idempotency_key("bad\nkey"));
    }

    #[test]
    fn diagnostics_require_explicit_consent_and_allowlisted_metadata() {
        let mut input = valid_input();
        input.diagnostics_consent = true;
        input.diagnostics_app_version = Some("0.1.0+desktop".to_owned());
        input.diagnostics_platform = Some("windows".to_owned());
        assert!(normalize(input.clone()).is_ok());

        input.diagnostics_consent = false;
        assert_eq!(
            normalize(input.clone()),
            Err(SupportRequestError::InvalidInput)
        );

        input.diagnostics_consent = true;
        input.diagnostics_platform = Some("web Chrome 132".to_owned());
        assert_eq!(
            normalize(input.clone()),
            Err(SupportRequestError::InvalidInput)
        );

        input.diagnostics_platform = Some("web".to_owned());
        input.diagnostics_app_version = Some("0.1.0 C:\\Users\\Pekay".to_owned());
        assert_eq!(normalize(input), Err(SupportRequestError::InvalidInput));
    }
}
