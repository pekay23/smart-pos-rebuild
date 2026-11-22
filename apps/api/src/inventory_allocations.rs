// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use sqlx::{Postgres, Transaction};

/// Serializes every mutation of a branch/product's physical stock or its
/// commitments. Callers pass all keys before taking product or aggregate row
/// locks; sorting here gives multi-product operations one global lock order.
pub(crate) async fn lock_products<I, S>(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    branch_id: &str,
    product_ids: I,
) -> Result<(), sqlx::Error>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut product_ids = product_ids
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect::<Vec<_>>();
    product_ids.sort_unstable();
    product_ids.dedup();
    for product_id in product_ids {
        let lock_key = format!("advanced-pos:inventory:v1:{tenant_id}:{branch_id}:{product_id}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Returns current active allocations after any inventory lock wait. The
/// database clock is evaluated in this query so expiry is not based on a stale
/// transaction-start timestamp.
pub(crate) async fn allocated_quantity(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    branch_id: &str,
    product_id: &str,
    exclude_transfer_id: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let amount = sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(SUM(quantity),0)::text
         FROM inventory_allocations
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid
           AND released_at IS NULL
           AND (expires_at IS NULL OR expires_at > clock_timestamp())
           AND ($4::uuid IS NULL OR stock_transfer_id IS DISTINCT FROM $4::uuid)",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(exclude_transfer_id)
    .fetch_one(&mut **tx)
    .await?;
    amount.parse::<i64>().map_err(|_| {
        sqlx::Error::Protocol(
            "Active inventory allocation total exceeds the supported quantity range.".to_owned(),
        )
    })
}

pub(crate) async fn allocated_quantity_excluding_pickup(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
    branch_id: &str,
    product_id: &str,
    pickup_reservation_id: uuid::Uuid,
) -> Result<i64, sqlx::Error> {
    let amount = sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(SUM(quantity),0)::text
         FROM inventory_allocations
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid
           AND released_at IS NULL
           AND (expires_at IS NULL OR expires_at > clock_timestamp())
           AND pickup_reservation_id IS DISTINCT FROM $4::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(pickup_reservation_id)
    .fetch_one(&mut **tx)
    .await?;
    amount.parse::<i64>().map_err(|_| {
        sqlx::Error::Protocol(
            "Active inventory allocation total exceeds the supported quantity range.".to_owned(),
        )
    })
}

pub(crate) fn can_consume(on_hand: i64, committed: i64, quantity: i64) -> bool {
    on_hand >= 0
        && committed >= 0
        && quantity >= 0
        && on_hand
            .checked_sub(committed)
            .is_some_and(|available| available >= quantity)
}

#[cfg(test)]
pub(crate) fn available_to_promise(on_hand: i64, committed: i64) -> i64 {
    on_hand.saturating_sub(committed).max(0)
}

#[cfg(test)]
pub(crate) fn commitment_shortfall(on_hand: i64, committed: i64) -> i64 {
    committed.saturating_sub(on_hand).max(0)
}

#[cfg(test)]
mod tests {
    use super::{available_to_promise, can_consume, commitment_shortfall};

    #[test]
    fn availability_never_promises_committed_or_overcommitted_units() {
        assert_eq!(available_to_promise(12, 5), 7);
        assert_eq!(available_to_promise(4, 7), 0);
        assert_eq!(commitment_shortfall(4, 7), 3);
        assert_eq!(commitment_shortfall(12, 5), 0);
        assert!(can_consume(12, 5, 7));
        assert!(!can_consume(12, 5, 8));
        assert!(!can_consume(4, 7, 0));
    }
}
