// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaymentOptionError {
    InvalidInput,
    Conflict,
    Database,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PaymentOptionInput {
    pub method_key: String,
    pub label: String,
    pub tender_kind: TenderKind,
    pub provider_name: Option<String>,
    pub country_code: Option<String>,
    pub requires_reference: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub(crate) enum TenderKind {
    Cash,
    Card,
    MobileMoney,
    BankTransfer,
    Other,
}

impl TenderKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::Card => "card",
            Self::MobileMoney => "mobile_money",
            Self::BankTransfer => "bank_transfer",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdatePaymentOptionInput {
    pub label: String,
    pub tender_kind: TenderKind,
    pub provider_name: Option<String>,
    pub country_code: Option<String>,
    pub requires_reference: bool,
    pub enabled: bool,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaymentOption {
    pub id: String,
    pub method_key: String,
    pub label: String,
    pub tender_kind: String,
    pub provider_name: Option<String>,
    pub country_code: Option<String>,
    pub requires_reference: bool,
    pub enabled: bool,
    pub recording_mode: String,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidPaymentOption {
    method_key: Option<String>,
    label: String,
    tender_kind: String,
    provider_name: Option<String>,
    country_code: Option<String>,
    requires_reference: bool,
    enabled: bool,
    expected_revision: Option<i64>,
}

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<Vec<PaymentOption>, PaymentOptionError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| PaymentOptionError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let options = sqlx::query_as::<_, PaymentOption>(
        "SELECT id::text, method_key, label, tender_kind, provider_name,
                NULLIF(btrim(country_code), '') AS country_code, requires_reference,
                enabled, recording_mode, revision
         FROM branch_payment_options
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid
         ORDER BY enabled DESC, lower(label), id",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| PaymentOptionError::Database)?;
    tx.commit()
        .await
        .map_err(|_| PaymentOptionError::Database)?;
    Ok(options)
}

pub(crate) async fn create(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    input: PaymentOptionInput,
) -> Result<PaymentOption, PaymentOptionError> {
    let valid = validate_create(input)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| PaymentOptionError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;
    let option = sqlx::query_as::<_, PaymentOption>(
        "INSERT INTO branch_payment_options
            (tenant_id, branch_id, method_key, label, tender_kind, provider_name,
             country_code, requires_reference, created_by_membership_id,
             updated_by_membership_id)
         VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9::uuid, $9::uuid)
         RETURNING id::text, method_key, label, tender_kind, provider_name,
                   NULLIF(btrim(country_code), '') AS country_code, requires_reference,
                   enabled, recording_mode, revision",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(
        valid
            .method_key
            .as_deref()
            .ok_or(PaymentOptionError::InvalidInput)?,
    )
    .bind(&valid.label)
    .bind(&valid.tender_kind)
    .bind(&valid.provider_name)
    .bind(&valid.country_code)
    .bind(valid.requires_reference)
    .bind(membership_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_write_error)?;
    audit(
        &mut tx,
        tenant_id,
        membership_id,
        "payment_option_created",
        &option,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|_| PaymentOptionError::Database)?;
    Ok(option)
}

pub(crate) async fn update(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    option_id: &str,
    input: UpdatePaymentOptionInput,
) -> Result<PaymentOption, PaymentOptionError> {
    let valid = validate_update(input)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| PaymentOptionError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;
    let option = sqlx::query_as::<_, PaymentOption>(
        "UPDATE branch_payment_options
         SET label = $5, tender_kind = $6, provider_name = $7, country_code = $8,
             requires_reference = $9, enabled = $10, revision = revision + 1,
             updated_by_membership_id = $4::uuid, updated_at = now()
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND id = $3::uuid
           AND revision = $11
         RETURNING id::text, method_key, label, tender_kind, provider_name,
                   NULLIF(btrim(country_code), '') AS country_code, requires_reference,
                   enabled, recording_mode, revision",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(option_id)
    .bind(membership_id)
    .bind(&valid.label)
    .bind(&valid.tender_kind)
    .bind(&valid.provider_name)
    .bind(&valid.country_code)
    .bind(valid.requires_reference)
    .bind(valid.enabled)
    .bind(valid.expected_revision)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_write_error)?
    .ok_or(PaymentOptionError::Conflict)?;
    audit(
        &mut tx,
        tenant_id,
        membership_id,
        "payment_option_updated",
        &option,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|_| PaymentOptionError::Database)?;
    Ok(option)
}

fn validate_create(input: PaymentOptionInput) -> Result<ValidPaymentOption, PaymentOptionError> {
    let method_key = input.method_key.trim().to_ascii_lowercase();
    let value = ValidPaymentOption {
        method_key: Some(method_key),
        label: input.label,
        tender_kind: input.tender_kind.as_str().to_owned(),
        provider_name: input.provider_name,
        country_code: input.country_code,
        requires_reference: input.requires_reference,
        enabled: true,
        expected_revision: None,
    };
    validate(value)
}

fn validate_update(
    input: UpdatePaymentOptionInput,
) -> Result<ValidPaymentOption, PaymentOptionError> {
    validate(ValidPaymentOption {
        method_key: None,
        label: input.label,
        tender_kind: input.tender_kind.as_str().to_owned(),
        provider_name: input.provider_name,
        country_code: input.country_code,
        requires_reference: input.requires_reference,
        enabled: input.enabled,
        expected_revision: Some(input.expected_revision),
    })
}

fn validate(mut value: ValidPaymentOption) -> Result<ValidPaymentOption, PaymentOptionError> {
    value.label = value.label.trim().to_owned();
    if !(1..=96).contains(&value.label.chars().count())
        || !matches!(
            value.tender_kind.as_str(),
            "cash" | "card" | "mobile_money" | "bank_transfer" | "other"
        )
        || (value.tender_kind == "cash" && value.requires_reference)
        || value.expected_revision.is_some_and(|revision| revision < 1)
    {
        return Err(PaymentOptionError::InvalidInput);
    }
    if let Some(method_key) = value.method_key.as_mut() {
        if !(2..=48).contains(&method_key.len())
            || !method_key.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            })
            || !method_key.as_bytes()[0].is_ascii_alphanumeric()
        {
            return Err(PaymentOptionError::InvalidInput);
        }
    }
    value.provider_name = normalize_optional(value.provider_name, 120)?;
    value.country_code = match value.country_code {
        None => None,
        Some(country) => {
            let country = country.trim().to_ascii_uppercase();
            if country.len() != 2 || !country.bytes().all(|byte| byte.is_ascii_uppercase()) {
                return Err(PaymentOptionError::InvalidInput);
            }
            Some(country)
        }
    };
    Ok(value)
}

fn normalize_optional(
    value: Option<String>,
    max_chars: usize,
) -> Result<Option<String>, PaymentOptionError> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(PaymentOptionError::InvalidInput);
    }
    Ok(Some(value.to_owned()))
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<(), PaymentOptionError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", station_id.unwrap_or_default()),
    ] {
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| PaymentOptionError::Database)?;
    }
    Ok(())
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    membership_id: &str,
    action: &str,
    option: &PaymentOption,
) -> Result<(), PaymentOptionError> {
    sqlx::query(
        "INSERT INTO audit_events (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, $3, 'branch_payment_option', $4,
                 jsonb_build_object('methodKey', $5, 'enabled', $6, 'recordingMode', $7))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(action)
    .bind(&option.id)
    .bind(&option.method_key)
    .bind(option.enabled)
    .bind(&option.recording_mode)
    .execute(&mut **tx)
    .await
    .map(|_| ())
    .map_err(|_| PaymentOptionError::Database)
}

fn map_write_error(error: sqlx::Error) -> PaymentOptionError {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        PaymentOptionError::Conflict
    } else {
        PaymentOptionError::Database
    }
}

#[cfg(test)]
mod tests {
    use super::{
        validate_create, validate_update, PaymentOptionError, PaymentOptionInput, TenderKind,
        UpdatePaymentOptionInput,
    };

    fn input() -> PaymentOptionInput {
        PaymentOptionInput {
            method_key: "mtn-momo".to_owned(),
            label: " MTN Mobile Money ".to_owned(),
            tender_kind: TenderKind::MobileMoney,
            provider_name: Some(" Paystack ".to_owned()),
            country_code: Some(" gh ".to_owned()),
            requires_reference: true,
        }
    }

    #[test]
    fn payment_method_input_is_normalized_and_keeps_manual_provider_disclosure() {
        let option = validate_create(input()).unwrap();
        assert_eq!(option.method_key.as_deref(), Some("mtn-momo"));
        assert_eq!(option.label, "MTN Mobile Money");
        assert_eq!(option.provider_name.as_deref(), Some("Paystack"));
        assert_eq!(option.country_code.as_deref(), Some("GH"));
        assert!(option.requires_reference);
    }

    #[test]
    fn payment_method_input_rejects_invalid_codes_cash_references_and_oversized_values() {
        let mut invalid = input();
        invalid.method_key = " bad key ".to_owned();
        assert_eq!(
            validate_create(invalid).unwrap_err(),
            PaymentOptionError::InvalidInput
        );

        let mut invalid = input();
        invalid.country_code = Some("Ghana".to_owned());
        assert_eq!(
            validate_create(invalid).unwrap_err(),
            PaymentOptionError::InvalidInput
        );

        let mut invalid = input();
        invalid.tender_kind = TenderKind::Cash;
        assert_eq!(
            validate_create(invalid).unwrap_err(),
            PaymentOptionError::InvalidInput
        );

        assert_eq!(
            validate_update(UpdatePaymentOptionInput {
                label: " ".to_owned(),
                tender_kind: TenderKind::Card,
                provider_name: Some("x".repeat(121)),
                country_code: None,
                requires_reference: true,
                enabled: true,
                expected_revision: 1,
            })
            .unwrap_err(),
            PaymentOptionError::InvalidInput
        );
    }
}
