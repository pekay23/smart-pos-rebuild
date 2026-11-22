// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::identity::MembershipRoleGrant;

pub(crate) const API_PERMISSIONS: &[&str] = &[
    "pos:use",
    "branches:manage",
    "products:read",
    "products:write",
    "inventory:write",
    "inventory:availability:read",
    "inventory:transfer:read",
    "inventory:transfer:request",
    "inventory:transfer:manage",
    "inventory:pickup:read",
    "inventory:pickup:request",
    "inventory:pickup:prepare",
    "inventory:pickup:manage",
    "purchases:read",
    "purchases:write",
    "purchases:invoice",
    "sales:read",
    "sales:checkout",
    "sales:refund",
    "sales:discount:approve",
    "payments:read",
    "payments:write",
    "support:write",
    "customers:read",
    "customers:write",
    "customers:manage",
    "suppliers:read",
    "suppliers:write",
    "suppliers:settle",
    "expenses:read",
    "expenses:write",
    "expenses:cash:write",
    "expenses:reclassify",
    "reports:read",
    "accounting:close",
    "cash:locations:read",
    "cash:locations:write",
    "settings:write",
    "users:write",
    "license:manage",
    "backups:manage",
    "roles:manage",
    "attendance:self",
    "attendance:read",
    "attendance:manage",
];

/// The target of an API permission check. Station actions also carry their parent
/// branch so a station identifier cannot be reused outside its owning branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceScope<'a> {
    Tenant,
    Branch {
        branch_id: &'a str,
    },
    Station {
        branch_id: &'a str,
        station_id: &'a str,
    },
}

/// Server-owned baseline permissions for the shipped roles. Unknown and
/// tenant-defined roles receive no permissions until persisted role permissions
/// are implemented; caller-provided or identity-provider role claims are never
/// consulted here.
fn baseline_permissions(role_key: &str) -> &'static [&'static str] {
    match role_key {
        "owner" => &[
            "pos:use",
            "branches:manage",
            "products:read",
            "products:write",
            "inventory:write",
            "inventory:availability:read",
            "inventory:transfer:read",
            "inventory:transfer:request",
            "inventory:transfer:manage",
            "inventory:pickup:read",
            "inventory:pickup:request",
            "inventory:pickup:prepare",
            "inventory:pickup:manage",
            "payments:read",
            "payments:write",
            "support:write",
            "purchases:read",
            "purchases:write",
            "purchases:invoice",
            "sales:read",
            "sales:refund",
            "sales:discount:approve",
            "customers:read",
            "customers:write",
            "customers:manage",
            "suppliers:read",
            "suppliers:write",
            "suppliers:settle",
            "expenses:read",
            "expenses:write",
            "expenses:reclassify",
            "reports:read",
            "accounting:close",
            "cash:locations:read",
            "cash:locations:write",
            "settings:write",
            "users:write",
            "license:manage",
            "backups:manage",
            "roles:manage",
            "attendance:self",
            "attendance:read",
            "attendance:manage",
        ],
        "admin" => &[
            "pos:use",
            "branches:manage",
            "products:read",
            "products:write",
            "inventory:write",
            "inventory:availability:read",
            "inventory:transfer:read",
            "inventory:transfer:request",
            "inventory:transfer:manage",
            "inventory:pickup:read",
            "inventory:pickup:request",
            "inventory:pickup:prepare",
            "inventory:pickup:manage",
            "payments:read",
            "payments:write",
            "support:write",
            "purchases:read",
            "purchases:write",
            "purchases:invoice",
            "sales:read",
            "sales:refund",
            "sales:discount:approve",
            "customers:read",
            "customers:write",
            "customers:manage",
            "suppliers:read",
            "suppliers:write",
            "suppliers:settle",
            "expenses:read",
            "expenses:write",
            "expenses:reclassify",
            "reports:read",
            "accounting:close",
            "cash:locations:read",
            "cash:locations:write",
            "settings:write",
            "users:write",
            "attendance:self",
            "attendance:read",
            "attendance:manage",
        ],
        "manager" => &[
            "pos:use",
            "branches:manage",
            "products:read",
            "products:write",
            "inventory:write",
            "inventory:availability:read",
            "inventory:transfer:read",
            "inventory:transfer:request",
            "inventory:transfer:manage",
            "inventory:pickup:read",
            "inventory:pickup:request",
            "inventory:pickup:prepare",
            "inventory:pickup:manage",
            "payments:read",
            "payments:write",
            "support:write",
            "purchases:read",
            "purchases:write",
            "purchases:invoice",
            "sales:read",
            "sales:refund",
            "sales:discount:approve",
            "customers:read",
            "customers:write",
            "customers:manage",
            "suppliers:read",
            "suppliers:write",
            "expenses:read",
            "expenses:write",
            "expenses:reclassify",
            "reports:read",
            "cash:locations:read",
            "cash:locations:write",
            "attendance:self",
            "attendance:read",
            "attendance:manage",
        ],
        "cashier" => &[
            "pos:use",
            "sales:checkout",
            "products:read",
            "inventory:availability:read",
            "inventory:transfer:read",
            "inventory:transfer:request",
            "inventory:pickup:read",
            "inventory:pickup:request",
            "inventory:pickup:prepare",
            "payments:read",
            "customers:read",
            "customers:write",
            "expenses:cash:write",
            "attendance:self",
        ],
        _ => &[],
    }
}

pub(crate) fn has_permission(
    grants: &[MembershipRoleGrant],
    permission: &str,
    requested_scope: ResourceScope<'_>,
) -> bool {
    grants.iter().any(|grant| {
        baseline_permissions(&grant.role_key).contains(&permission)
            && grant_covers_scope(grant, requested_scope)
    })
}

pub(crate) fn permissions_for<'a>(
    grants: &[MembershipRoleGrant],
    requested_scope: ResourceScope<'a>,
) -> Vec<&'static str> {
    API_PERMISSIONS
        .iter()
        .copied()
        .filter(|permission| has_permission(grants, permission, requested_scope))
        .collect()
}

fn grant_covers_scope(grant: &MembershipRoleGrant, requested_scope: ResourceScope<'_>) -> bool {
    match requested_scope {
        ResourceScope::Tenant => grant.branch_id.is_none() && grant.station_id.is_none(),
        ResourceScope::Branch { branch_id } => {
            grant.station_id.is_none()
                && grant
                    .branch_id
                    .as_deref()
                    .is_none_or(|granted_branch| granted_branch == branch_id)
        }
        ResourceScope::Station {
            branch_id,
            station_id,
        } => {
            grant
                .branch_id
                .as_deref()
                .is_none_or(|granted_branch| granted_branch == branch_id)
                && grant
                    .station_id
                    .as_deref()
                    .is_none_or(|granted_station| granted_station == station_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{has_permission, permissions_for, ResourceScope};
    use crate::identity::MembershipRoleGrant;

    fn grant(
        role_key: &str,
        branch_id: Option<&str>,
        station_id: Option<&str>,
    ) -> MembershipRoleGrant {
        MembershipRoleGrant {
            role_key: role_key.to_owned(),
            branch_id: branch_id.map(str::to_owned),
            station_id: station_id.map(str::to_owned),
        }
    }

    #[test]
    fn cashier_has_register_permissions_but_not_stock_or_settings_access() {
        let grants = [grant("cashier", Some("branch-a"), Some("station-a"))];

        assert!(has_permission(
            &grants,
            "pos:use",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(has_permission(
            &grants,
            "sales:checkout",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(has_permission(
            &grants,
            "expenses:cash:write",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(!has_permission(
            &grants,
            "expenses:cash:write",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &grants,
            "inventory:write",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(!has_permission(
            &grants,
            "settings:write",
            ResourceScope::Tenant
        ));
        assert!(!has_permission(
            &grants,
            "branches:manage",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &grants,
            "customers:manage",
            ResourceScope::Branch {
                branch_id: "branch-a"
            },
        ));
    }

    #[test]
    fn customer_and_supplier_management_is_available_to_managers_but_not_cashiers() {
        let manager = [grant("manager", Some("branch-a"), None)];
        let cashier = [grant("cashier", Some("branch-a"), Some("station-a"))];
        assert!(has_permission(
            &manager,
            "branches:manage",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(has_permission(
            &manager,
            "cash:locations:read",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(has_permission(
            &manager,
            "cash:locations:write",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &cashier,
            "cash:locations:read",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &cashier,
            "cash:locations:write",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(has_permission(
            &manager,
            "customers:manage",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &cashier,
            "customers:manage",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(has_permission(
            &manager,
            "suppliers:write",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &cashier,
            "suppliers:read",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(has_permission(
            &manager,
            "purchases:invoice",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &cashier,
            "purchases:invoice",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
    }

    #[test]
    fn owners_and_managers_can_view_pos_without_checkout_permission() {
        let station = ResourceScope::Station {
            branch_id: "branch-a",
            station_id: "station-a",
        };
        for role in ["owner", "admin", "manager"] {
            let grants = [grant(role, Some("branch-a"), None)];
            assert!(has_permission(&grants, "pos:use", station));
            assert!(has_permission(&grants, "sales:read", station));
            assert!(!has_permission(&grants, "sales:checkout", station));
            assert!(!has_permission(&grants, "expenses:cash:write", station));
        }

        // A separately assigned cashier profile grants checkout only at its scope.
        let temporary_cashier = [
            grant("manager", Some("branch-a"), None),
            grant("cashier", Some("branch-a"), Some("station-a")),
        ];
        assert!(has_permission(
            &temporary_cashier,
            "sales:checkout",
            station
        ));
        assert!(!has_permission(
            &temporary_cashier,
            "sales:checkout",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-b"
            }
        ));
    }

    #[test]
    fn cashier_can_view_payment_options_but_only_managers_can_configure_them() {
        let cashier = [grant("cashier", Some("branch-a"), Some("station-a"))];
        let station = ResourceScope::Station {
            branch_id: "branch-a",
            station_id: "station-a",
        };
        assert!(has_permission(&cashier, "payments:read", station));
        assert!(!has_permission(&cashier, "payments:write", station));

        let manager = [grant("manager", Some("branch-a"), None)];
        assert!(has_permission(
            &manager,
            "payments:write",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &manager,
            "payments:write",
            ResourceScope::Branch {
                branch_id: "branch-b"
            }
        ));
    }

    #[test]
    fn cashiers_can_request_and_view_transfers_but_only_managers_can_manage_them() {
        let cashier_station = ResourceScope::Station {
            branch_id: "branch-a",
            station_id: "station-a",
        };
        let cashier = [grant("cashier", Some("branch-a"), Some("station-a"))];
        assert!(has_permission(
            &cashier,
            "inventory:transfer:read",
            cashier_station
        ));
        assert!(has_permission(
            &cashier,
            "inventory:transfer:request",
            cashier_station
        ));
        assert!(!has_permission(
            &cashier,
            "inventory:transfer:manage",
            cashier_station
        ));
        assert!(has_permission(
            &cashier,
            "inventory:pickup:read",
            cashier_station
        ));
        assert!(has_permission(
            &cashier,
            "inventory:pickup:request",
            cashier_station
        ));
        assert!(has_permission(
            &cashier,
            "inventory:pickup:prepare",
            cashier_station
        ));
        assert!(!has_permission(
            &cashier,
            "inventory:pickup:manage",
            cashier_station
        ));

        let manager = [grant("manager", Some("branch-a"), None)];
        let branch = ResourceScope::Branch {
            branch_id: "branch-a",
        };
        assert!(has_permission(
            &manager,
            "inventory:transfer:manage",
            branch
        ));
        assert!(has_permission(&manager, "inventory:pickup:manage", branch));
        assert!(has_permission(&manager, "inventory:pickup:prepare", branch));
        assert!(!has_permission(
            &manager,
            "inventory:transfer:manage",
            ResourceScope::Branch {
                branch_id: "branch-b"
            }
        ));
    }

    #[test]
    fn branch_and_station_grants_cannot_escape_their_assigned_scope() {
        let branch_grant = [grant("manager", Some("branch-a"), None)];
        assert!(has_permission(
            &branch_grant,
            "reports:read",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &branch_grant,
            "reports:read",
            ResourceScope::Branch {
                branch_id: "branch-b"
            }
        ));
        assert!(has_permission(
            &branch_grant,
            "reports:read",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-x"
            }
        ));

        let station_grant = [grant("cashier", Some("branch-a"), Some("station-a"))];
        assert!(has_permission(
            &station_grant,
            "pos:use",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(!has_permission(
            &station_grant,
            "pos:use",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-b"
            }
        ));
        assert!(!has_permission(
            &station_grant,
            "pos:use",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
    }

    #[test]
    fn tenant_grants_cover_children_but_branch_grants_do_not_authorize_tenant_actions() {
        let tenant_grant = [grant("owner", None, None)];
        assert!(has_permission(
            &tenant_grant,
            "settings:write",
            ResourceScope::Tenant
        ));
        assert!(has_permission(
            &tenant_grant,
            "settings:write",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));

        let branch_grant = [grant("admin", Some("branch-a"), None)];
        assert!(!has_permission(
            &branch_grant,
            "settings:write",
            ResourceScope::Tenant
        ));
        let branch_manager = [grant("manager", Some("branch-a"), None)];
        assert!(has_permission(
            &branch_manager,
            "products:write",
            ResourceScope::Branch {
                branch_id: "branch-a"
            }
        ));
        assert!(!has_permission(
            &branch_manager,
            "products:write",
            ResourceScope::Tenant
        ));
    }

    #[test]
    fn unknown_custom_or_malformed_role_keys_fail_closed() {
        let grants = [
            grant("custom-manager", None, None),
            grant("Owner", None, None),
            grant("owner ", None, None),
        ];
        assert!(!has_permission(
            &grants,
            "roles:manage",
            ResourceScope::Tenant
        ));
    }

    #[test]
    fn permissions_from_multiple_scoped_roles_are_unioned_only_at_the_requested_resource() {
        let grants = [
            grant("cashier", Some("branch-a"), Some("station-a")),
            grant("manager", Some("branch-b"), None),
        ];
        assert!(has_permission(
            &grants,
            "pos:use",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(!has_permission(
            &grants,
            "inventory:write",
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a"
            }
        ));
        assert!(has_permission(
            &grants,
            "inventory:write",
            ResourceScope::Branch {
                branch_id: "branch-b"
            }
        ));
    }

    #[test]
    fn station_permission_snapshot_contains_only_effective_server_owned_permissions() {
        let grants = [grant("cashier", Some("branch-a"), Some("station-a"))];
        let permissions = permissions_for(
            &grants,
            ResourceScope::Station {
                branch_id: "branch-a",
                station_id: "station-a",
            },
        );

        assert_eq!(
            permissions,
            [
                "pos:use",
                "products:read",
                "inventory:availability:read",
                "inventory:transfer:read",
                "inventory:transfer:request",
                "inventory:pickup:read",
                "inventory:pickup:request",
                "inventory:pickup:prepare",
                "sales:checkout",
                "payments:read",
                "customers:read",
                "customers:write",
                "expenses:cash:write",
                "attendance:self"
            ]
        );
        assert!(!permissions.contains(&"inventory:write"));
        assert!(!permissions.contains(&"settings:write"));
    }

    #[test]
    fn attendance_permissions_keep_self_service_separate_from_management() {
        let branch = ResourceScope::Branch {
            branch_id: "branch-a",
        };
        let cashier = [grant("cashier", Some("branch-a"), None)];
        assert!(has_permission(&cashier, "attendance:self", branch));
        assert!(!has_permission(&cashier, "attendance:read", branch));
        assert!(!has_permission(&cashier, "attendance:manage", branch));

        for role in ["owner", "admin", "manager"] {
            let manager = [grant(role, Some("branch-a"), None)];
            assert!(has_permission(&manager, "attendance:self", branch));
            assert!(has_permission(&manager, "attendance:read", branch));
            assert!(has_permission(&manager, "attendance:manage", branch));
        }

        let other_branch_manager = [grant("manager", Some("branch-b"), None)];
        assert!(!has_permission(
            &other_branch_manager,
            "attendance:manage",
            branch
        ));
    }
}
