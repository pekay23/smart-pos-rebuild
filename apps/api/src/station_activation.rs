// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationError {
    InvalidDeviceKey,
    InvalidChallenge,
    InvalidProof,
    InvalidRevocationReason,
    ActivationNotFound,
    NoStationEntitlement,
    StationUnavailable,
    StationAlreadyActivated,
    DeviceAlreadyActivated,
    CapacityReached,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivationResult {
    pub activation_id: String,
    pub active_stations: i32,
    pub station_limit: i32,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivationChallenge {
    pub challenge_id: String,
    pub nonce: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RevocationResult {
    pub activation_id: String,
    pub active_stations: i32,
    pub revoked_at: String,
}

pub(crate) async fn revoke_station(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    reason: &str,
) -> Result<RevocationResult, ActivationError> {
    let reason = reason.trim();
    if !(3..=500).contains(&reason.chars().count()) {
        return Err(ActivationError::InvalidRevocationReason);
    }

    let mut transaction = pool.begin().await.map_err(|_| ActivationError::Database)?;
    set_identity_context(&mut transaction, identity_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(tenant_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ActivationError::Database)?;
    // Use the same station lock as cashier lease acquisition so a lease cannot
    // appear between the revocation cleanup and commit.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(station_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ActivationError::Database)?;

    let revoked = sqlx::query_as::<_, (String, String)>(
        r#"UPDATE station_activations
           SET status = 'revoked', revoked_at = now(), revoked_by_membership_id = $4::uuid,
               revocation_reason = $5
           WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND station_id = $3::uuid
             AND status = 'active'
           RETURNING id::text, to_char(revoked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')"#,
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(reason)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?
    .ok_or(ActivationError::ActivationNotFound)?;

    sqlx::query(
        "UPDATE station_activation_challenges SET consumed_at = now()
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND station_id = $3::uuid
           AND consumed_at IS NULL",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;

    sqlx::query(
        "UPDATE station_actor_leases l
         SET status = 'released', ended_at = now(), expires_at = now(),
             ended_reason = 'station_activation_revoked'
         WHERE l.tenant_id = $1::uuid AND l.branch_id = $2::uuid AND l.station_id = $3::uuid
           AND l.status = 'issued'
           AND EXISTS (
               SELECT 1 FROM station_actor_lease_locks k
               WHERE k.lease_id = l.id AND k.station_id = l.station_id
           )",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    sqlx::query(
        "DELETE FROM station_actor_lease_locks k
         USING station_actor_leases l
         WHERE k.lease_id = l.id AND k.station_id = $1::uuid AND k.tenant_id = $2::uuid
           AND l.ended_reason = 'station_activation_revoked' AND l.status = 'released'",
    )
    .bind(station_id)
    .bind(tenant_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;

    let active_stations = count_active_stations(&mut transaction, tenant_id).await?;
    transaction
        .commit()
        .await
        .map_err(|_| ActivationError::Database)?;
    Ok(RevocationResult {
        activation_id: revoked.0,
        active_stations,
        revoked_at: revoked.1,
    })
}

pub(crate) async fn create_challenge(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<ActivationChallenge, ActivationError> {
    let mut transaction = pool.begin().await.map_err(|_| ActivationError::Database)?;
    set_identity_context(&mut transaction, identity_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(station_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ActivationError::Database)?;
    let station_is_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM stations
         WHERE id = $1::uuid AND branch_id = $2::uuid AND tenant_id = $3::uuid AND status = 'active')",
    )
    .bind(station_id)
    .bind(branch_id)
    .bind(tenant_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    if !station_is_active {
        return Err(ActivationError::StationUnavailable);
    }

    let nonce = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let nonce_bytes = decode_hex(&nonce, 32).ok_or(ActivationError::Database)?;
    let (challenge_id, expires_at) = sqlx::query_as::<_, (String, String)>(
        r#"INSERT INTO station_activation_challenges
            (tenant_id, branch_id, station_id, created_by_membership_id, nonce)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5)
         RETURNING id::text,
                   to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')"#,
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(nonce_bytes)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| ActivationError::Database)?;
    Ok(ActivationChallenge {
        challenge_id,
        nonce,
        expires_at,
    })
}

pub(crate) async fn activate_station(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    device_public_key_hex: &str,
    challenge_id: &str,
    signature_hex: &str,
) -> Result<ActivationResult, ActivationError> {
    let public_key = decode_device_key(device_public_key_hex)?;
    let signature = decode_hex(signature_hex, 64).ok_or(ActivationError::InvalidProof)?;
    let mut transaction = pool.begin().await.map_err(|_| ActivationError::Database)?;
    set_identity_context(&mut transaction, identity_id).await?;
    // Serialize proof consumption and revocation before locking a challenge row.
    // This gives both flows the same tenant -> station/challenge lock order.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(tenant_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ActivationError::Database)?;

    let challenge = sqlx::query_as::<_, (Vec<u8>,)>(
        "SELECT nonce FROM station_activation_challenges
         WHERE id = $1::uuid AND tenant_id = $2::uuid AND branch_id = $3::uuid
           AND station_id = $4::uuid AND created_by_membership_id = $5::uuid
           AND consumed_at IS NULL AND expires_at > now()
         FOR UPDATE",
    )
    .bind(challenge_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?
    .ok_or(ActivationError::InvalidChallenge)?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| ActivationError::InvalidProof)?;
    let key: [u8; 32] = public_key
        .clone()
        .try_into()
        .map_err(|_| ActivationError::InvalidDeviceKey)?;
    let verifying_key =
        VerifyingKey::from_bytes(&key).map_err(|_| ActivationError::InvalidDeviceKey)?;
    let signature = Signature::from_bytes(&signature);
    let message = activation_message(
        tenant_id,
        branch_id,
        station_id,
        challenge_id,
        &challenge.0,
        &public_key,
    );
    verifying_key
        .verify(&message, &signature)
        .map_err(|_| ActivationError::InvalidProof)?;
    let consumed = sqlx::query(
        "UPDATE station_activation_challenges SET consumed_at = now()
         WHERE id = $1::uuid AND consumed_at IS NULL AND expires_at > now()",
    )
    .bind(challenge_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    if consumed.rows_affected() != 1 {
        return Err(ActivationError::InvalidChallenge);
    }

    let entitlement = sqlx::query_as::<_, (bool, i32)>(
        "SELECT enabled, numeric_limit
         FROM tenant_entitlements
         WHERE tenant_id = $1::uuid AND entitlement_key = 'stations.active'",
    )
    .bind(tenant_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?
    .filter(|(enabled, limit)| *enabled && *limit > 0)
    .ok_or(ActivationError::NoStationEntitlement)?;
    let station_limit = entitlement.1;

    let station_is_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM stations
            WHERE id = $1::uuid AND branch_id = $2::uuid AND tenant_id = $3::uuid
              AND status = 'active'
        )",
    )
    .bind(station_id)
    .bind(branch_id)
    .bind(tenant_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    if !station_is_active {
        return Err(ActivationError::StationUnavailable);
    }

    if let Some((activation_id, existing_key)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text, device_public_key FROM station_activations
         WHERE station_id = $1::uuid AND tenant_id = $2::uuid AND status = 'active'",
    )
    .bind(station_id)
    .bind(tenant_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?
    {
        if existing_key == public_key {
            let active_stations = count_active_stations(&mut transaction, tenant_id).await?;
            transaction
                .commit()
                .await
                .map_err(|_| ActivationError::Database)?;
            return Ok(ActivationResult {
                activation_id,
                active_stations,
                station_limit,
                created: false,
            });
        }
        return Err(ActivationError::StationAlreadyActivated);
    }

    let device_in_use = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM station_activations
            WHERE tenant_id = $1::uuid AND device_public_key = $2 AND status = 'active'
        )",
    )
    .bind(tenant_id)
    .bind(&public_key)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    if device_in_use {
        return Err(ActivationError::DeviceAlreadyActivated);
    }

    let active_stations = count_active_stations(&mut transaction, tenant_id).await?;
    if active_stations >= station_limit {
        return Err(ActivationError::CapacityReached);
    }

    let activation_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO station_activations
            (tenant_id, branch_id, station_id, device_public_key, activated_by_membership_id)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5::uuid)
         RETURNING id::text",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(&public_key)
    .bind(membership_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|database_error| database_error.is_unique_violation())
        {
            ActivationError::DeviceAlreadyActivated
        } else {
            ActivationError::Database
        }
    })?;

    transaction
        .commit()
        .await
        .map_err(|_| ActivationError::Database)?;
    Ok(ActivationResult {
        activation_id,
        active_stations: active_stations + 1,
        station_limit,
        created: true,
    })
}

async fn count_active_stations(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
) -> Result<i32, ActivationError> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM station_activations
         WHERE tenant_id = $1::uuid AND status = 'active'",
    )
    .bind(tenant_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| ActivationError::Database)?;
    i32::try_from(count).map_err(|_| ActivationError::Database)
}

async fn set_identity_context(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: &str,
) -> Result<(), ActivationError> {
    sqlx::query("SELECT set_config('pos.identity_id', $1, true)")
        .bind(identity_id)
        .execute(&mut **transaction)
        .await
        .map(|_| ())
        .map_err(|_| ActivationError::Database)
}

fn decode_device_key(value: &str) -> Result<Vec<u8>, ActivationError> {
    decode_hex(value, 32).ok_or(ActivationError::InvalidDeviceKey)
}

fn decode_hex(value: &str, expected_bytes: usize) -> Option<Vec<u8>> {
    let value = value.trim();
    if value.len() != expected_bytes * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

fn activation_message(
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    challenge_id: &str,
    nonce: &[u8],
    public_key: &[u8],
) -> Vec<u8> {
    let nonce_hex = encode_hex(nonce);
    let public_key_hex = encode_hex(public_key);
    format!(
        "advanced-pos/station-activation/v1\n{tenant_id}\n{branch_id}\n{station_id}\n{challenge_id}\n{nonce_hex}\n{public_key_hex}"
    ).into_bytes()
}

fn encode_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{decode_device_key, ActivationError};

    #[test]
    fn device_key_parser_requires_exactly_32_bytes_of_hex() {
        assert_eq!(decode_device_key(&"ab".repeat(32)).unwrap(), vec![0xab; 32]);
        assert_eq!(
            decode_device_key(&"ab".repeat(31)).unwrap_err(),
            ActivationError::InvalidDeviceKey
        );
        assert_eq!(
            decode_device_key(&format!("{}zz", "ab".repeat(31))).unwrap_err(),
            ActivationError::InvalidDeviceKey
        );
    }
}
