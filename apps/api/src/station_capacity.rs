// SPDX-License-Identifier: AGPL-3.0-or-later

use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationCapacitySnapshot {
    pub entitlement_active: bool,
    pub station_limit: Option<i32>,
    pub active_stations: i32,
    pub available_slots: i32,
}

#[derive(Debug)]
pub enum StationCapacityError {
    Database,
    CountOutOfRange,
}

fn remaining_slots(station_limit: Option<i32>, active_stations: i32) -> i32 {
    station_limit
        .map(|limit| limit.saturating_sub(active_stations).max(0))
        .unwrap_or(0)
}

pub async fn load(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
) -> Result<StationCapacitySnapshot, StationCapacityError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| StationCapacityError::Database)?;
    sqlx::query("SELECT set_config('pos.identity_id', $1, true)")
        .bind(identity_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| StationCapacityError::Database)?;

    // RLS hides entitlements outside their effective window. The activation
    // count and entitlement are read in one transaction under the same identity.
    let entitlement = sqlx::query_as::<_, (bool, i32)>(
        "SELECT enabled, numeric_limit FROM tenant_entitlements
         WHERE tenant_id = $1::uuid AND entitlement_key = 'stations.active'",
    )
    .bind(tenant_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StationCapacityError::Database)?;
    let active_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM station_activations
         WHERE tenant_id = $1::uuid AND status = 'active'",
    )
    .bind(tenant_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| StationCapacityError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| StationCapacityError::Database)?;

    let active_stations =
        i32::try_from(active_count).map_err(|_| StationCapacityError::CountOutOfRange)?;
    let station_limit = entitlement
        .filter(|(enabled, limit)| *enabled && *limit > 0)
        .map(|(_, limit)| limit);
    let available_slots = remaining_slots(station_limit, active_stations);

    Ok(StationCapacitySnapshot {
        entitlement_active: station_limit.is_some(),
        station_limit,
        active_stations,
        available_slots,
    })
}

#[cfg(test)]
mod tests {
    use super::remaining_slots;

    #[test]
    fn remaining_station_slots_never_become_negative_after_a_downgrade() {
        assert_eq!(remaining_slots(Some(3), 2), 1);
        assert_eq!(remaining_slots(Some(2), 3), 0);
        assert_eq!(remaining_slots(None, 0), 0);
    }
}
