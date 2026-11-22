// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProofError {
    InvalidInput,
    LeaseUnavailable,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutChallenge {
    pub challenge_id: String,
    pub nonce: String,
    pub expires_at: String,
    pub message_to_sign: String,
}

pub(crate) async fn create_financial_challenge(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    lease_id: &str,
    lease_token: &str,
    operation: &str,
    resource_id: Option<&str>,
    idempotency_key: &str,
    payload_hash_hex: &str,
) -> Result<CheckoutChallenge, ProofError> {
    let target = financial_target_path(tenant_id, branch_id, station_id, operation, resource_id)
        .ok_or(ProofError::InvalidInput)?;
    if !valid_idempotency_key(idempotency_key) {
        return Err(ProofError::InvalidInput);
    }
    let payload_hash = decode_hex(payload_hash_hex, 32).ok_or(ProofError::InvalidInput)?;
    let lease_token_hash = Sha256::digest(lease_token.as_bytes()).to_vec();

    let mut transaction = pool.begin().await.map_err(|_| ProofError::Database)?;
    set_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(station_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ProofError::Database)?;

    let activation_id = sqlx::query_scalar::<_, String>(
        "SELECT a.id::text
         FROM station_actor_leases l
         JOIN station_actor_lease_locks k
           ON k.tenant_id = l.tenant_id AND k.branch_id = l.branch_id
          AND k.station_id = l.station_id AND k.lease_id = l.id
         JOIN station_activations a
           ON a.tenant_id = l.tenant_id AND a.branch_id = l.branch_id
          AND a.station_id = l.station_id AND a.id = l.activation_id
         JOIN memberships m
           ON m.id = l.membership_id AND m.tenant_id = l.tenant_id
          AND m.identity_id = l.identity_id AND m.status = 'active'
         JOIN membership_role_grants g
           ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
         WHERE l.id = $1::uuid AND l.tenant_id = $2::uuid
           AND l.branch_id = $3::uuid AND l.station_id = $4::uuid
           AND l.membership_id = $5::uuid AND l.identity_id = $6::uuid
           AND l.lease_token_hash = $7 AND l.status = 'issued' AND l.expires_at > now()
           AND a.status = 'active' AND g.role_key = 'cashier'
           AND (g.branch_id IS NULL OR g.branch_id = l.branch_id)
           AND (g.station_id IS NULL OR g.station_id = l.station_id)
         LIMIT 1",
    )
    .bind(lease_id)
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(lease_token_hash)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| ProofError::Database)?
    .ok_or(ProofError::LeaseUnavailable)?;

    let nonce = [Uuid::new_v4().into_bytes(), Uuid::new_v4().into_bytes()].concat();
    let (challenge_id, expires_at) = sqlx::query_as::<_, (String, String)>(
        r#"INSERT INTO station_financial_proof_challenges
             (tenant_id, branch_id, station_id, membership_id, identity_id,
              activation_id, lease_id, operation, idempotency_key, payload_hash, nonce)
             VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid,
                   $6::uuid, $7::uuid, $8, $9, $10, $11)
           RETURNING id::text,
                     to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')"#,
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(membership_id)
    .bind(identity_id)
    .bind(&activation_id)
    .bind(lease_id)
    .bind(operation)
    .bind(idempotency_key)
    .bind(&payload_hash)
    .bind(&nonce)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ProofError::Database)?;

    let message_to_sign = format!(
        "advanced-pos/financial-operation/v1\nPOST\n{target}\n{operation}\n{tenant_id}\n{branch_id}\n{station_id}\n{lease_id}\n{membership_id}\n{activation_id}\n{challenge_id}\n{idempotency_key}\n{}\n{}",
        encode_hex(&payload_hash),
        encode_hex(&nonce)
    );

    transaction
        .commit()
        .await
        .map_err(|_| ProofError::Database)?;
    Ok(CheckoutChallenge {
        challenge_id,
        nonce: encode_hex(&nonce),
        expires_at,
        message_to_sign,
    })
}

pub(crate) fn financial_target_path(
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    operation: &str,
    resource_id: Option<&str>,
) -> Option<String> {
    let base = format!("/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}");
    match (operation, resource_id) {
        ("sale.commit", None) => Some(format!("{base}/sales")),
        ("sale.refund", Some(id)) if Uuid::parse_str(id).is_ok() => {
            Some(format!("{base}/sales/{id}/refunds"))
        }
        ("drawer.open", None) => Some(format!("{base}/drawer-shifts")),
        ("drawer.movement", Some(id)) if Uuid::parse_str(id).is_ok() => {
            Some(format!("{base}/drawer-shifts/{id}/movements"))
        }
        ("drawer.close", Some(id)) if Uuid::parse_str(id).is_ok() => {
            Some(format!("{base}/drawer-shifts/{id}/close"))
        }
        ("supplier.payment", Some(id)) if Uuid::parse_str(id).is_ok() => {
            Some(format!("{base}/supplier-payments"))
        }
        ("expense.payment", Some(id)) if Uuid::parse_str(id).is_ok() => {
            Some(format!("{base}/expenses"))
        }
        ("receivable.collection", Some(id)) if Uuid::parse_str(id).is_ok() => {
            Some(format!("{base}/customer-collections"))
        }
        _ => None,
    }
}

fn valid_idempotency_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
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

async fn set_context(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
) -> Result<(), ProofError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", station_id),
    ] {
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind(key)
            .bind(value)
            .execute(&mut **transaction)
            .await
            .map_err(|_| ProofError::Database)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_hex, encode_hex, financial_target_path, valid_idempotency_key};

    #[test]
    fn financial_targets_are_allowlisted_and_resource_bound() {
        assert_eq!(
            financial_target_path("t", "b", "s", "sale.commit", None).as_deref(),
            Some("/v1/tenants/t/branches/b/stations/s/sales")
        );
        assert_eq!(
            financial_target_path(
                "t",
                "b",
                "s",
                "sale.refund",
                Some("00000000-0000-4000-8000-000000000001")
            )
            .as_deref(),
            Some("/v1/tenants/t/branches/b/stations/s/sales/00000000-0000-4000-8000-000000000001/refunds")
        );
        assert!(financial_target_path("t", "b", "s", "drawer.close", Some("bad")).is_none());
        assert_eq!(
            financial_target_path(
                "t",
                "b",
                "s",
                "supplier.payment",
                Some("00000000-0000-4000-8000-000000000001")
            )
            .as_deref(),
            Some("/v1/tenants/t/branches/b/stations/s/supplier-payments")
        );
        assert_eq!(
            financial_target_path(
                "t",
                "b",
                "s",
                "expense.payment",
                Some("00000000-0000-4000-8000-000000000001")
            )
            .as_deref(),
            Some("/v1/tenants/t/branches/b/stations/s/expenses")
        );
        assert!(financial_target_path("t", "b", "s", "unknown", None).is_none());
    }

    #[test]
    fn idempotency_keys_are_visible_ascii_and_bounded() {
        assert!(valid_idempotency_key("sale-01"));
        assert!(valid_idempotency_key(&"x".repeat(128)));
        assert!(!valid_idempotency_key(""));
        assert!(!valid_idempotency_key("contains spaces"));
        assert!(!valid_idempotency_key(&"x".repeat(129)));
    }

    #[test]
    fn payload_hash_hex_requires_exactly_32_bytes() {
        let bytes = decode_hex(&"ab".repeat(32), 32).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(encode_hex(&bytes), "ab".repeat(32));
        assert!(decode_hex("deadbeef", 32).is_none());
        assert!(decode_hex(&"zz".repeat(32), 32).is_none());
    }
}
