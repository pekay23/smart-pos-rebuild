// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const LEASE_SECONDS: i64 = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseError {
    NotAvailable,
    AlreadyHeld,
    InvalidOrExpired,
    InvalidChallenge,
    InvalidProof,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaseSnapshot {
    pub lease_id: String,
    pub token: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaseChallenge {
    pub challenge_id: String,
    pub nonce: String,
    pub expires_at: String,
    pub message_to_sign: String,
}

pub(crate) async fn create_challenge(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<LeaseChallenge, LeaseError> {
    let mut transaction = pool.begin().await.map_err(|_| LeaseError::Database)?;
    set_identity_context(&mut transaction, identity_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(station_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| LeaseError::Database)?;

    let eligible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            JOIN stations s
              ON s.id = $5::uuid AND s.branch_id = $4::uuid AND s.tenant_id = $3::uuid
             AND s.status = 'active'
            JOIN station_activations a
              ON a.tenant_id = s.tenant_id AND a.branch_id = s.branch_id AND a.station_id = s.id
             AND a.status = 'active'
            WHERE m.id = $2::uuid AND m.tenant_id = $3::uuid
              AND m.identity_id = $1::uuid AND m.status = 'active'
              AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = $4::uuid)
              AND (g.station_id IS NULL OR g.station_id = $5::uuid)
        )",
    )
    .bind(identity_id)
    .bind(membership_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?;
    if !eligible {
        return Err(LeaseError::NotAvailable);
    }

    let activation_id = sqlx::query_scalar::<_, String>(
        "SELECT id::text FROM station_activations
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid
           AND station_id = $3::uuid AND status = 'active'",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?
    .ok_or(LeaseError::NotAvailable)?;

    let nonce = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let nonce_bytes = decode_hex(&nonce, 32).ok_or(LeaseError::Database)?;
    let (challenge_id, expires_at) = sqlx::query_as::<_, (String, String)>(
        r#"INSERT INTO station_actor_lease_challenges
            (tenant_id, branch_id, station_id, membership_id, identity_id, nonce)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6)
         RETURNING id::text,
                   to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')"#,
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(&nonce_bytes)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| LeaseError::Database)?;

    let message_to_sign = lease_message(
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        &challenge_id,
        &nonce_bytes,
        &activation_id,
    );

    Ok(LeaseChallenge {
        challenge_id,
        nonce,
        expires_at,
        message_to_sign: String::from_utf8(message_to_sign).map_err(|_| LeaseError::Database)?,
    })
}

pub(crate) async fn acquire(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    challenge_id: &str,
    signature_hex: &str,
) -> Result<LeaseSnapshot, LeaseError> {
    let mut transaction = pool.begin().await.map_err(|_| LeaseError::Database)?;
    set_identity_context(&mut transaction, identity_id).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(station_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| LeaseError::Database)?;

    let Some((activation_id, public_key)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id::text, device_public_key FROM station_activations
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid
           AND station_id = $3::uuid AND status = 'active'",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?
    else {
        return Err(LeaseError::NotAvailable);
    };

    let challenge = sqlx::query_as::<_, (Vec<u8>,)>(
        "SELECT nonce FROM station_actor_lease_challenges
         WHERE id = $1::uuid AND tenant_id = $2::uuid AND branch_id = $3::uuid
           AND station_id = $4::uuid AND membership_id = $5::uuid
           AND identity_id = $6::uuid AND consumed_at IS NULL AND expires_at > now()
         FOR UPDATE",
    )
    .bind(challenge_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?
    .ok_or(LeaseError::InvalidChallenge)?;
    let signature = decode_hex(signature_hex, 64).ok_or(LeaseError::InvalidProof)?;
    let key: [u8; 32] = public_key
        .as_slice()
        .try_into()
        .map_err(|_| LeaseError::Database)?;
    let verifying_key = VerifyingKey::from_bytes(&key).map_err(|_| LeaseError::Database)?;
    let signature: [u8; 64] = signature.try_into().map_err(|_| LeaseError::InvalidProof)?;
    let signature = Signature::from_bytes(&signature);
    let message = lease_message(
        tenant_id,
        branch_id,
        station_id,
        membership_id,
        identity_id,
        challenge_id,
        &challenge.0,
        &activation_id,
    );
    verifying_key
        .verify(&message, &signature)
        .map_err(|_| LeaseError::InvalidProof)?;
    let consumed = sqlx::query(
        "UPDATE station_actor_lease_challenges SET consumed_at = now()
         WHERE id = $1::uuid AND consumed_at IS NULL AND expires_at > now()",
    )
    .bind(challenge_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?;
    if consumed.rows_affected() != 1 {
        return Err(LeaseError::InvalidChallenge);
    }

    let eligible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1
            FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            JOIN stations s
              ON s.id = $5::uuid AND s.branch_id = $4::uuid AND s.tenant_id = $3::uuid
             AND s.status = 'active'
            WHERE m.id = $2::uuid AND m.tenant_id = $3::uuid
              AND m.identity_id = $1::uuid AND m.status = 'active'
              AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = $4::uuid)
              AND (g.station_id IS NULL OR g.station_id = $5::uuid)
        )",
    )
    .bind(identity_id)
    .bind(membership_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?;
    if !eligible {
        return Err(LeaseError::NotAvailable);
    }

    sqlx::query(
        "DELETE FROM station_actor_lease_locks lock_row
         USING station_actor_leases lease
         WHERE lock_row.lease_id = lease.id
           AND lock_row.station_id = $1::uuid AND lock_row.tenant_id = $2::uuid
           AND lease.status = 'issued' AND lease.expires_at <= now()",
    )
    .bind(station_id)
    .bind(tenant_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?;

    let occupied = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM station_actor_lease_locks lock_row
            JOIN station_actor_leases lease ON lease.id = lock_row.lease_id
            WHERE lock_row.station_id = $1::uuid AND lock_row.tenant_id = $2::uuid
              AND lease.status = 'issued' AND lease.expires_at > now()
        )",
    )
    .bind(station_id)
    .bind(tenant_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?;
    if occupied {
        return Err(LeaseError::AlreadyHeld);
    }

    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_hash = hash_token(&token);
    let (lease_id, expires_at) = sqlx::query_as::<_, (String, String)>(
        "INSERT INTO station_actor_leases
            (tenant_id, branch_id, station_id, membership_id, identity_id, activation_id,
             lease_token_hash, expires_at)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6::uuid, $7,
                 now() + ($8::bigint * interval '1 second'))
         RETURNING id::text, expires_at::text",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(&activation_id)
    .bind(token_hash)
    .bind(LEASE_SECONDS)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|database_error| database_error.is_unique_violation())
        {
            LeaseError::AlreadyHeld
        } else {
            LeaseError::Database
        }
    })?;
    sqlx::query(
        "INSERT INTO station_actor_lease_locks (station_id, tenant_id, branch_id, lease_id)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid)",
    )
    .bind(station_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&lease_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|database_error| database_error.is_unique_violation())
        {
            LeaseError::AlreadyHeld
        } else {
            LeaseError::Database
        }
    })?;
    transaction
        .commit()
        .await
        .map_err(|_| LeaseError::Database)?;

    Ok(LeaseSnapshot {
        lease_id,
        token,
        expires_at,
    })
}

pub(crate) async fn heartbeat(
    pool: &PgPool,
    identity_id: &str,
    lease_id: &str,
    lease_token: &str,
) -> Result<String, LeaseError> {
    update_lease(pool, identity_id, lease_id, lease_token, true).await
}

pub(crate) async fn release(
    pool: &PgPool,
    identity_id: &str,
    lease_id: &str,
    lease_token: &str,
) -> Result<String, LeaseError> {
    update_lease(pool, identity_id, lease_id, lease_token, false).await
}

async fn update_lease(
    pool: &PgPool,
    identity_id: &str,
    lease_id: &str,
    lease_token: &str,
    renew: bool,
) -> Result<String, LeaseError> {
    let mut transaction = pool.begin().await.map_err(|_| LeaseError::Database)?;
    set_identity_context(&mut transaction, identity_id).await?;
    let scope = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT tenant_id::text,branch_id::text,station_id::text,membership_id::text
         FROM station_actor_leases WHERE id=$1::uuid AND identity_id=$2::uuid
           AND lease_token_hash=$3 AND status='issued' AND expires_at>now()",
    )
    .bind(lease_id)
    .bind(identity_id)
    .bind(hash_token(lease_token))
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| LeaseError::Database)?
    .ok_or(LeaseError::InvalidOrExpired)?;
    for (key, value) in [
        ("pos.tenant_id", scope.0.as_str()),
        ("pos.branch_id", scope.1.as_str()),
        ("pos.station_id", scope.2.as_str()),
        ("pos.membership_id", scope.3.as_str()),
        ("pos.lease_id", lease_id),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *transaction)
            .await
            .map_err(|_| LeaseError::Database)?;
    }
    if !renew {
        sqlx::query(
            "UPDATE branch_held_tickets SET claim_lease_id=NULL,claim_expires_at=NULL,
                    updated_by=$2::uuid,revision=revision+1,updated_at=clock_timestamp()
             WHERE tenant_id=$3::uuid AND branch_id=$4::uuid AND claim_lease_id=$1::uuid
               AND status IN ('held','checkout_pending')",
        )
        .bind(lease_id)
        .bind(&scope.3)
        .bind(&scope.0)
        .bind(&scope.1)
        .execute(&mut *transaction)
        .await
        .map_err(|_| LeaseError::Database)?;
    }
    let token_hash = hash_token(lease_token);
    let update = if renew {
        "UPDATE station_actor_leases
         SET heartbeat_at = now(), expires_at = now() + ($4::bigint * interval '1 second')
         WHERE id = $3::uuid AND identity_id = $1::uuid AND lease_token_hash = $2
           AND status = 'issued' AND expires_at > now()
         RETURNING expires_at::text"
    } else {
        "UPDATE station_actor_leases
         SET status = 'released', ended_at = now(), ended_reason = 'cashier_logout'
         WHERE id = $3::uuid AND identity_id = $1::uuid AND lease_token_hash = $2
           AND status = 'issued' AND expires_at > now()
         RETURNING expires_at::text"
    };
    let mut query = sqlx::query_scalar::<_, String>(update)
        .bind(identity_id)
        .bind(token_hash)
        .bind(lease_id);
    if renew {
        query = query.bind(LEASE_SECONDS);
    }
    let expires_at = query
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| LeaseError::Database)?
        .ok_or(LeaseError::InvalidOrExpired)?;
    if renew {
        sqlx::query(
            "UPDATE branch_held_tickets SET claim_expires_at=$2::timestamptz
             WHERE tenant_id=$3::uuid AND branch_id=$4::uuid AND claim_lease_id=$1::uuid
               AND status IN ('held','checkout_pending')",
        )
        .bind(lease_id)
        .bind(&expires_at)
        .bind(&scope.0)
        .bind(&scope.1)
        .execute(&mut *transaction)
        .await
        .map_err(|_| LeaseError::Database)?;
    } else {
        sqlx::query(
            "DELETE FROM station_actor_lease_locks
             WHERE lease_id = $1::uuid AND station_id IN (
                 SELECT station_id FROM station_actor_leases WHERE id = $1::uuid
             )",
        )
        .bind(lease_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| LeaseError::Database)?;
    }
    transaction
        .commit()
        .await
        .map_err(|_| LeaseError::Database)?;
    Ok(expires_at)
}

async fn set_identity_context(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: &str,
) -> Result<(), LeaseError> {
    sqlx::query("SELECT set_config('pos.identity_id', $1, true)")
        .bind(identity_id)
        .execute(&mut **transaction)
        .await
        .map(|_| ())
        .map_err(|_| LeaseError::Database)
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn lease_message(
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    membership_id: &str,
    identity_id: &str,
    challenge_id: &str,
    nonce: &[u8],
    activation_id: &str,
) -> Vec<u8> {
    format!(
        "advanced-pos/station-lease/v1\n{tenant_id}\n{branch_id}\n{station_id}\n{membership_id}\n{identity_id}\n{activation_id}\n{challenge_id}\n{}",
        encode_hex(nonce)
    )
    .into_bytes()
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
    use super::hash_token;

    #[test]
    fn lease_tokens_are_stored_as_sha256_hashes() {
        let first = hash_token("lease-secret");
        assert_eq!(first.len(), 32);
        assert_eq!(first, hash_token("lease-secret"));
        assert_ne!(first, hash_token("another-secret"));
    }
}
