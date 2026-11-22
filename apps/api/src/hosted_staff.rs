use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedStaffMember {
    pub membership_id: String,
    pub display_name: Option<String>,
    pub roles: Vec<String>,
}

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
) -> Result<Vec<HostedStaffMember>, ()> {
    let mut tx = pool.begin().await.map_err(|_| ())?;
    set_context(&mut tx, identity_id, tenant_id, membership_id).await?;
    let staff = sqlx::query_as::<_, HostedStaffMember>(
        "SELECT m.id::text AS membership_id,
                NULLIF(btrim(i.display_name), '') AS display_name,
                COALESCE(array_agg(DISTINCT g.role_key ORDER BY g.role_key)
                    FILTER (WHERE g.role_key IS NOT NULL), ARRAY[]::text[]) AS roles
         FROM memberships m
         LEFT JOIN person_identities i ON i.id = m.identity_id
         LEFT JOIN membership_role_grants g
           ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
         WHERE m.tenant_id = $1::uuid AND m.status = 'active'
         GROUP BY m.id, i.display_name
         ORDER BY lower(COALESCE(NULLIF(btrim(i.display_name), ''), 'staff')), m.id",
    )
    .bind(tenant_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ())?;
    tx.commit().await.map_err(|_| ())?;
    Ok(staff)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
) -> Result<(), ()> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", ""),
        ("pos.station_id", ""),
        ("pos.staff_roster_tenant_id", tenant_id),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}
