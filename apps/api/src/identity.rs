// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::Serialize;
use sqlx::PgPool;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityStatus {
    Active,
    Suspended,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalIdentity {
    pub identity_id: String,
    pub status: IdentityStatus,
    #[serde(skip)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantMembership {
    pub tenant_id: String,
    pub display_name: String,
    pub membership_id: String,
    pub role_grants: Vec<MembershipRoleGrant>,
    pub branches: Vec<BranchAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipRoleGrant {
    pub role_key: String,
    pub branch_id: Option<String>,
    pub station_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchAccess {
    pub branch_id: String,
    pub code: String,
    pub name: String,
    pub timezone: String,
    pub currency_code: String,
    pub stations: Vec<StationAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationAccess {
    pub station_id: String,
    pub code: String,
    pub name: String,
}

#[derive(Debug)]
pub enum IdentityLookupError {
    Database,
    InvalidStatus,
}

pub async fn find_identity(
    pool: &PgPool,
    issuer: &str,
    subject: &str,
) -> Result<Option<InternalIdentity>, IdentityLookupError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| IdentityLookupError::Database)?;
    sqlx::query(
        "SELECT set_config('pos.verified_issuer', $1, true),
                set_config('pos.verified_subject', $2, true)",
    )
    .bind(issuer)
    .bind(subject)
    .execute(&mut *transaction)
    .await
    .map_err(|_| IdentityLookupError::Database)?;
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT id::text, status, NULLIF(btrim(display_name), '') FROM person_identities WHERE issuer = $1 AND subject = $2",
    )
    .bind(issuer)
    .bind(subject)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| IdentityLookupError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| IdentityLookupError::Database)?;

    row.map(|(identity_id, status, display_name)| {
        let status = match status.as_str() {
            "active" => IdentityStatus::Active,
            "suspended" => IdentityStatus::Suspended,
            "removed" => IdentityStatus::Removed,
            _ => return Err(IdentityLookupError::InvalidStatus),
        };
        Ok(InternalIdentity {
            identity_id,
            status,
            display_name,
        })
    })
    .transpose()
}

type TenantAccessRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Lists only the active tenant memberships resolved from an already verified identity.
/// RLS context is transaction-local so pooled connections cannot retain an identity.
pub async fn list_tenant_memberships(
    pool: &PgPool,
    identity_id: &str,
) -> Result<Vec<TenantMembership>, IdentityLookupError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| IdentityLookupError::Database)?;
    sqlx::query("SELECT set_config('pos.identity_id', $1, true)")
        .bind(identity_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| IdentityLookupError::Database)?;

    let rows = sqlx::query_as::<_, TenantAccessRow>(
        "SELECT t.id::text, t.display_name, m.id::text,
                g.role_key, g.branch_id::text, g.station_id::text,
                b.id::text, b.code, b.name, b.timezone, b.currency_code::text,
                s.id::text, s.code, s.name
         FROM memberships m
         JOIN tenants t ON t.id = m.tenant_id
         LEFT JOIN membership_role_grants g
           ON g.membership_id = m.id AND g.tenant_id = m.tenant_id
         LEFT JOIN branches b
           ON b.tenant_id = m.tenant_id AND b.status = 'active'
          AND (g.branch_id IS NULL OR g.branch_id = b.id)
         LEFT JOIN stations s
           ON s.tenant_id = b.tenant_id AND s.branch_id = b.id AND s.status = 'active'
          AND (g.station_id IS NULL OR g.station_id = s.id)
         WHERE m.identity_id = $1::uuid AND m.status = 'active' AND t.status = 'active'
         ORDER BY t.display_name, g.role_key NULLS LAST, b.name, s.name",
    )
    .bind(identity_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| IdentityLookupError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| IdentityLookupError::Database)?;

    let mut memberships = BTreeMap::<String, TenantMembership>::new();
    let mut branches = BTreeMap::<(String, String), BranchAccess>::new();
    for (
        tenant_id,
        display_name,
        membership_id,
        role_key,
        grant_branch_id,
        grant_station_id,
        branch_id,
        branch_code,
        branch_name,
        timezone,
        currency_code,
        station_id,
        station_code,
        station_name,
    ) in rows
    {
        let tenant = memberships
            .entry(tenant_id.clone())
            .or_insert_with(|| TenantMembership {
                tenant_id: tenant_id.clone(),
                display_name,
                membership_id,
                role_grants: Vec::new(),
                branches: Vec::new(),
            });

        if let Some(role_key) = role_key {
            let grant = MembershipRoleGrant {
                role_key,
                branch_id: grant_branch_id,
                station_id: grant_station_id,
            };
            if !tenant.role_grants.contains(&grant) {
                tenant.role_grants.push(grant);
            }
        }

        if let (Some(branch_id), Some(code), Some(name), Some(timezone), Some(currency_code)) =
            (branch_id, branch_code, branch_name, timezone, currency_code)
        {
            let branch = branches
                .entry((tenant_id, branch_id.clone()))
                .or_insert_with(|| BranchAccess {
                    branch_id,
                    code,
                    name,
                    timezone,
                    currency_code,
                    stations: Vec::new(),
                });
            if let (Some(station_id), Some(code), Some(name)) =
                (station_id, station_code, station_name)
            {
                let station = StationAccess {
                    station_id,
                    code,
                    name,
                };
                if !branch.stations.contains(&station) {
                    branch.stations.push(station);
                }
            }
        }
    }
    for ((tenant_id, _), branch) in branches {
        if let Some(tenant) = memberships.get_mut(&tenant_id) {
            tenant.branches.push(branch);
        }
    }
    for tenant in memberships.values_mut() {
        tenant.role_grants.sort_by(|a, b| {
            (&a.role_key, &a.branch_id, &a.station_id).cmp(&(
                &b.role_key,
                &b.branch_id,
                &b.station_id,
            ))
        });
    }
    Ok(memberships.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_status_serializes_without_provider_or_tenant_fields() {
        let identity = InternalIdentity {
            identity_id: "person-id".to_owned(),
            status: IdentityStatus::Active,
            display_name: None,
        };
        let json = serde_json::to_value(identity).unwrap();

        assert_eq!(json["identityId"], "person-id");
        assert_eq!(json["status"], "active");
        assert!(json.get("issuer").is_none());
        assert!(json.get("displayName").is_none());
        assert!(json.get("tenantId").is_none());
    }

    #[test]
    fn tenant_access_serializes_membership_scopes_and_assigned_stations() {
        let tenant = TenantMembership {
            tenant_id: "tenant-1".to_owned(),
            display_name: "Example Store".to_owned(),
            membership_id: "membership-1".to_owned(),
            role_grants: vec![MembershipRoleGrant {
                role_key: "cashier".to_owned(),
                branch_id: Some("branch-1".to_owned()),
                station_id: Some("station-1".to_owned()),
            }],
            branches: vec![BranchAccess {
                branch_id: "branch-1".to_owned(),
                code: "ACCRA-1".to_owned(),
                name: "Accra".to_owned(),
                timezone: "Africa/Accra".to_owned(),
                currency_code: "GHS".to_owned(),
                stations: vec![StationAccess {
                    station_id: "station-1".to_owned(),
                    code: "TILL-1".to_owned(),
                    name: "Front till".to_owned(),
                }],
            }],
        };
        let value = serde_json::to_value(tenant).unwrap();
        assert_eq!(value["tenantId"], "tenant-1");
        assert_eq!(value["roleGrants"][0]["stationId"], "station-1");
        assert_eq!(
            value["branches"][0]["stations"][0]["stationId"],
            "station-1"
        );
        assert!(value.get("identityId").is_none());
    }
}
