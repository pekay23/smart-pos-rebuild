// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    attendance,
    authorization::{self, ResourceScope},
    catalog, identity, sales,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchOverview {
    pub branch_id: String,
    pub branch_name: String,
    pub timezone: String,
    pub currency_code: String,
    pub sales: Option<sales::RecentBranchSalesSummary>,
    pub inventory: Option<catalog::BranchLowStockSummary>,
    pub attendance: Option<attendance::BranchAttendanceSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TenantOverview {
    pub branches: Vec<BranchOverview>,
    pub offset: usize,
    pub page_size: usize,
    pub total_branch_count: usize,
    pub sales_window_hours: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverviewError {
    InvalidPage,
    PermissionDenied,
    Database,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OverviewQuery {
    pub offset: Option<usize>,
    pub page_size: Option<usize>,
}

impl OverviewQuery {
    pub(crate) fn page(self) -> Result<(usize, usize), OverviewError> {
        let offset = self.offset.unwrap_or(0);
        let page_size = self.page_size.unwrap_or(20);
        if offset > 10_000 || !(1..=50).contains(&page_size) {
            return Err(OverviewError::InvalidPage);
        }
        Ok((offset, page_size))
    }
}

pub(crate) async fn load(
    pool: &PgPool,
    identity_id: &str,
    membership: &identity::TenantMembership,
    offset: usize,
    page_size: usize,
) -> Result<TenantOverview, OverviewError> {
    if offset > 10_000 || !(1..=50).contains(&page_size) {
        return Err(OverviewError::InvalidPage);
    }

    let mut accessible = accessible_branches(membership);
    accessible.sort_by(|left, right| {
        left.0
            .name
            .to_lowercase()
            .cmp(&right.0.name.to_lowercase())
            .then_with(|| left.0.branch_id.cmp(&right.0.branch_id))
    });
    let total_branch_count = accessible.len();
    if total_branch_count == 0 {
        return Err(OverviewError::PermissionDenied);
    }
    let branches = accessible.into_iter().skip(offset).take(page_size).map(
        |(branch, can_read_sales, can_read_inventory, can_manage_attendance)| async move {
            let sales = if can_read_sales {
                Some(
                    sales::recent_branch_summary(
                        pool,
                        identity_id,
                        &membership.tenant_id,
                        &membership.membership_id,
                        &branch.branch_id,
                    )
                    .await
                    .map_err(|_| OverviewError::Database)?,
                )
            } else {
                None
            };
            let inventory = if can_read_inventory {
                Some(
                    catalog::branch_low_stock_summary(
                        pool,
                        identity_id,
                        &membership.tenant_id,
                        &membership.membership_id,
                        &branch.branch_id,
                    )
                    .await
                    .map_err(|_| OverviewError::Database)?,
                )
            } else {
                None
            };
            let attendance = if can_manage_attendance {
                Some(
                    attendance::branch_summary(
                        pool,
                        identity_id,
                        &membership.tenant_id,
                        &membership.membership_id,
                        &branch.branch_id,
                    )
                    .await
                    .map_err(|_| OverviewError::Database)?,
                )
            } else {
                None
            };
            Ok(BranchOverview {
                branch_id: branch.branch_id.clone(),
                branch_name: branch.name.clone(),
                timezone: branch.timezone.clone(),
                currency_code: branch.currency_code.clone(),
                sales,
                inventory,
                attendance,
            })
        },
    );
    let mut summaries = Vec::new();
    for branch in branches {
        summaries.push(branch.await?);
    }
    Ok(TenantOverview {
        branches: summaries,
        offset,
        page_size,
        total_branch_count,
        sales_window_hours: 168,
    })
}

fn accessible_branches(
    membership: &identity::TenantMembership,
) -> Vec<(&identity::BranchAccess, bool, bool, bool)> {
    membership
        .branches
        .iter()
        .filter_map(|branch| {
            let scope = ResourceScope::Branch {
                branch_id: &branch.branch_id,
            };
            let can_read_sales =
                authorization::has_permission(&membership.role_grants, "sales:read", scope);
            let can_read_inventory =
                authorization::has_permission(&membership.role_grants, "products:read", scope);
            let can_manage_attendance =
                authorization::has_permission(&membership.role_grants, "attendance:manage", scope);
            can_read_sales.then_some((
                branch,
                can_read_sales,
                can_read_inventory,
                can_manage_attendance,
            ))
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::accessible_branches;
    use crate::identity::{BranchAccess, MembershipRoleGrant, TenantMembership};

    fn membership(grants: Vec<(&str, Option<&str>, Option<&str>)>) -> TenantMembership {
        TenantMembership {
            tenant_id: "tenant-1".into(),
            display_name: "Example business".into(),
            membership_id: "member-1".into(),
            role_grants: grants
                .into_iter()
                .map(|(role_key, branch_id, station_id)| MembershipRoleGrant {
                    role_key: role_key.into(),
                    branch_id: branch_id.map(str::to_owned),
                    station_id: station_id.map(str::to_owned),
                })
                .collect(),
            branches: ["branch-a", "branch-b"]
                .into_iter()
                .map(|branch_id| BranchAccess {
                    branch_id: branch_id.into(),
                    code: branch_id.into(),
                    name: branch_id.into(),
                    timezone: "Africa/Accra".into(),
                    currency_code: "GHS".into(),
                    stations: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn overview_includes_all_owner_branches_and_only_manager_granted_branches() {
        let owner = membership(vec![("owner", None, None)]);
        let owner_branches = accessible_branches(&owner);
        assert_eq!(owner_branches.len(), 2);
        assert!(owner_branches.iter().all(|branch| branch.3));

        let manager = membership(vec![("manager", Some("branch-b"), None)]);
        let visible = accessible_branches(&manager);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].0.branch_id, "branch-b");
        assert!(visible[0].3);
    }

    #[test]
    fn station_only_cashier_grant_does_not_expose_a_branch_overview() {
        let cashier = membership(vec![("cashier", Some("branch-a"), Some("station-a"))]);
        assert!(accessible_branches(&cashier).is_empty());
    }

    #[test]
    fn branch_catalog_access_without_report_permission_does_not_expose_management_metrics() {
        let cashier = membership(vec![("cashier", Some("branch-a"), None)]);
        assert!(accessible_branches(&cashier).is_empty());
    }
}
