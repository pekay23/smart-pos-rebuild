// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BranchSettingsError {
    InvalidInput,
    NotFound,
    Conflict,
    Database,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpeningInterval {
    pub open: String,
    pub close: String,
}

pub(crate) type OpeningHours = BTreeMap<String, Vec<OpeningInterval>>;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchProfile {
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub opening_hours: sqlx::types::Json<OpeningHours>,
    pub settings_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateBranchProfileInput {
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub opening_hours: OpeningHours,
    pub expected_revision: i64,
}

fn normalize_optional(
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, BranchSettingsError> {
    let value = value
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty());
    if value
        .as_ref()
        .is_some_and(|item| item.chars().count() > max)
    {
        return Err(BranchSettingsError::InvalidInput);
    }
    Ok(value)
}

fn minutes(value: &str) -> Option<u16> {
    let (hour, minute) = value.split_once(':')?;
    if hour.len() != 2
        || minute.len() != 2
        || !hour.bytes().all(|b| b.is_ascii_digit())
        || !minute.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let hour = hour.parse::<u16>().ok()?;
    let minute = minute.parse::<u16>().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

fn validate_opening_hours(hours: &OpeningHours) -> Result<(), BranchSettingsError> {
    if hours.len() != WEEKDAYS.len() || WEEKDAYS.iter().any(|day| !hours.contains_key(*day)) {
        return Err(BranchSettingsError::InvalidInput);
    }
    for intervals in hours.values() {
        if intervals.len() > 4 {
            return Err(BranchSettingsError::InvalidInput);
        }
        let mut previous_close = 0;
        for interval in intervals {
            let open = minutes(&interval.open).ok_or(BranchSettingsError::InvalidInput)?;
            let close = minutes(&interval.close).ok_or(BranchSettingsError::InvalidInput)?;
            if close <= open || open < previous_close {
                return Err(BranchSettingsError::InvalidInput);
            }
            previous_close = close;
        }
    }
    Ok(())
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<(), BranchSettingsError> {
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
            .map_err(|_| BranchSettingsError::Database)?;
    }
    Ok(())
}

pub(crate) async fn get(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<BranchProfile, BranchSettingsError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| BranchSettingsError::Database)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let profile = sqlx::query_as::<_, BranchProfile>(
        "SELECT address,phone,email,opening_hours AS opening_hours,settings_revision FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| BranchSettingsError::Database)?
    .ok_or(BranchSettingsError::NotFound)?;
    tx.commit()
        .await
        .map_err(|_| BranchSettingsError::Database)?;
    Ok(profile)
}

pub(crate) async fn update(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    input: UpdateBranchProfileInput,
) -> Result<BranchProfile, BranchSettingsError> {
    if input.expected_revision < 1 {
        return Err(BranchSettingsError::InvalidInput);
    }
    let address = normalize_optional(input.address, 500)?;
    let phone = normalize_optional(input.phone, 40)?;
    let email = normalize_optional(input.email, 254)?;
    if email
        .as_ref()
        .is_some_and(|value| !value.contains('@') || value.starts_with('@') || value.ends_with('@'))
    {
        return Err(BranchSettingsError::InvalidInput);
    }
    validate_opening_hours(&input.opening_hours)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|_| BranchSettingsError::Database)?;
    set_context(&mut tx, identity, tenant, membership, branch).await?;
    let result = sqlx::query_as::<_, BranchProfile>(
        "UPDATE branches SET address=$4,phone=$5,email=$6,opening_hours=$7,settings_revision=settings_revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active' AND settings_revision=$3 RETURNING address,phone,email,opening_hours AS opening_hours,settings_revision",
    )
    .bind(tenant)
    .bind(branch)
    .bind(input.expected_revision)
    .bind(address)
    .bind(phone)
    .bind(email)
    .bind(sqlx::types::Json(input.opening_hours))
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| BranchSettingsError::Database)?
    .ok_or(BranchSettingsError::Conflict)?;

    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'branch_profile_updated','branch',$3::uuid,$4)")
        .bind(tenant)
        .bind(membership)
        .bind(branch)
        .bind(serde_json::json!({"revision":result.settings_revision}))
        .execute(&mut *tx)
        .await
        .map_err(|_| BranchSettingsError::Database)?;
    tx.commit()
        .await
        .map_err(|_| BranchSettingsError::Database)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{validate_opening_hours, OpeningHours, OpeningInterval, WEEKDAYS};
    use std::collections::BTreeMap;

    fn hours(monday: Vec<(&str, &str)>) -> OpeningHours {
        WEEKDAYS
            .iter()
            .map(|day| {
                let intervals = if *day == "monday" {
                    monday
                        .iter()
                        .map(|(open, close)| OpeningInterval {
                            open: (*open).into(),
                            close: (*close).into(),
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                ((*day).to_owned(), intervals)
            })
            .collect::<BTreeMap<_, _>>()
    }

    #[test]
    fn opening_hours_allow_closed_days_and_ordered_split_shifts() {
        assert!(
            validate_opening_hours(&hours(vec![("08:00", "12:00"), ("13:00", "18:00")])).is_ok()
        );
        assert!(validate_opening_hours(&hours(vec![])).is_ok());
    }

    #[test]
    fn opening_hours_reject_overlaps_invalid_times_and_unknown_days() {
        assert!(
            validate_opening_hours(&hours(vec![("08:00", "12:00"), ("11:00", "14:00")])).is_err()
        );
        assert!(validate_opening_hours(&hours(vec![("08:00", "25:00")])).is_err());
        let mut extra_day = hours(vec![]);
        extra_day.insert("holiday".into(), vec![]);
        assert!(validate_opening_hours(&extra_day).is_err());
    }
}
