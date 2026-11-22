// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event, role_definitions_permissions},
    db::AppState,
    models::{
        ClaimBusinessOwnerInput, PermissionDefinition, SaveStaffRoleInput, SessionUser,
        StaffAccountRoleInput, StaffRole, StaffRolesView,
    },
};

const PERMISSIONS: &[(&str, &str, &str, &str)] = &[
    (
        "pos:use",
        "Use register",
        "Register",
        "Create and complete sales.",
    ),
    (
        "products:read",
        "View products",
        "Inventory",
        "Search the product catalog.",
    ),
    (
        "products:write",
        "Manage products",
        "Inventory",
        "Add and edit products.",
    ),
    (
        "inventory:write",
        "Adjust stock",
        "Inventory",
        "Record stock adjustments.",
    ),
    (
        "purchases:read",
        "View purchases",
        "Purchasing",
        "Review purchase orders.",
    ),
    (
        "purchases:write",
        "Manage purchases",
        "Purchasing",
        "Receive stock and manage purchases.",
    ),
    (
        "purchases:invoice",
        "Match supplier invoices",
        "Purchasing",
        "Match supplier invoices to received deliveries.",
    ),
    ("sales:read", "View sales", "Sales", "View sales history."),
    (
        "sales:refund",
        "Refund sales",
        "Sales",
        "Refund or void completed sales.",
    ),
    (
        "sales:discount:approve",
        "Approve discounts",
        "Sales",
        "Review and approve a ticket discount with a personal credential.",
    ),
    (
        "customers:read",
        "View customers",
        "Customers",
        "Search customer records.",
    ),
    (
        "customers:write",
        "Manage customers",
        "Customers",
        "Create and edit customer records.",
    ),
    (
        "suppliers:read",
        "View suppliers",
        "Suppliers",
        "View supplier records.",
    ),
    (
        "suppliers:write",
        "Manage suppliers",
        "Suppliers",
        "Create and edit supplier records.",
    ),
    (
        "suppliers:settle",
        "Record supplier payments",
        "Suppliers",
        "Record external supplier payments against open invoices.",
    ),
    (
        "expenses:read",
        "View expenses",
        "Finance",
        "Review store expenses.",
    ),
    (
        "expenses:write",
        "Record expenses",
        "Finance",
        "Record store expenses.",
    ),
    (
        "expenses:reclassify",
        "Correct expense attribution",
        "Finance",
        "Move an expense and its returns between reporting cost centers with an audit reason.",
    ),
    (
        "reports:read",
        "View reports",
        "Reports",
        "View operational and sales reports.",
    ),
    (
        "accounting:close",
        "Close accounting periods",
        "Finance",
        "Review and finalize a balanced store accounting period.",
    ),
];

fn custom_permissions() -> Vec<String> {
    PERMISSIONS.iter().map(|p| p.0.to_owned()).collect()
}

fn require_current_owner(state: &AppState, actor: &SessionUser) -> Result<(), String> {
    let current = state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.")?
        .clone()
        .ok_or_else(|| "Sign in to continue.".to_owned())?;
    if current.user_id != actor.user_id || current.business_id != actor.business_id {
        return Err("Your account changed. Sign in again before managing roles.".to_owned());
    }
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let owner: Option<String> = db
        .query_row(
            "SELECT owner_user_id FROM businesses WHERE id=?1",
            [&actor.business_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    if owner.as_deref() != Some(actor.user_id.as_str()) {
        return Err("Only the designated business owner can manage roles.".to_owned());
    }
    Ok(())
}

#[tauri::command]
pub fn get_staff_roles(state: State<'_, AppState>) -> Result<StaffRolesView, String> {
    let session = active_session(&state, "users:write")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let mut query = db.prepare("SELECT r.role_key,r.display_name,r.permissions_json,r.workspace_mode,r.is_system,r.role_revision,(SELECT COUNT(*) FROM users u WHERE u.role_id=r.id AND u.is_active=1) FROM roles r WHERE r.business_id=?1 AND r.role_key<>'owner' ORDER BY r.is_system DESC,r.display_name COLLATE NOCASE").map_err(|e|e.to_string())?;
    let roles = query
        .query_map([&session.business_id], |r| {
            Ok(StaffRole {
                role_key: r.get(0)?,
                display_name: r.get(1)?,
                permissions: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                workspace_mode: r.get(3)?,
                is_system: r.get::<_, i64>(4)? != 0,
                revision: r.get(5)?,
                active_user_count: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let owner_user_id = db
        .query_row(
            "SELECT owner_user_id FROM businesses WHERE id=?1",
            [&session.business_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    Ok(StaffRolesView {
        roles,
        permissions: PERMISSIONS
            .iter()
            .map(|(key, label, group, description)| PermissionDefinition {
                key: (*key).into(),
                label: (*label).into(),
                group: (*group).into(),
                description: (*description).into(),
            })
            .collect(),
        owner_user_id,
    })
}

#[tauri::command]
pub fn assign_staff_role(
    input: StaffAccountRoleInput,
    state: State<'_, AppState>,
) -> Result<crate::models::StaffAccount, String> {
    let actor = active_session(&state, "users:write")?;
    if input.user_id == actor.user_id {
        return Err("You cannot change the role of the account you are using.".into());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let target: Option<(String,bool)> = tx.query_row("SELECT u.role_id,COALESCE(b.owner_user_id=u.id,0) FROM users u JOIN businesses b ON b.id=u.business_id WHERE u.id=?1 AND u.business_id=?2",params![input.user_id,actor.business_id],|r|Ok((r.get(0)?,r.get::<_,i64>(1)?!=0))).optional().map_err(|e|e.to_string())?;
    let Some((current_role_id, is_owner)) = target else {
        return Err("That staff account could not be found in this business.".into());
    };
    if is_owner {
        return Err("The business owner's role is protected.".into());
    }
    let role: Option<(String,String)> = tx.query_row("SELECT id,permissions_json FROM roles WHERE business_id=?1 AND role_key=?2 AND role_key<>'owner'",params![actor.business_id,input.role_key],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    let Some((role_id, permissions_json)) = role else {
        return Err("Choose a role configured for this business.".into());
    };
    let permissions: Vec<String> = serde_json::from_str(&permissions_json).unwrap_or_default();
    if permissions
        .iter()
        .any(|p| !actor.permissions.iter().any(|own| own == p))
    {
        return Err("You cannot assign a role with permissions you do not have.".into());
    }
    if current_role_id == role_id {
        return Err("That staff member already has this role.".into());
    }
    let next_key: String = tx
        .query_row("SELECT role_key FROM roles WHERE id=?1", [&role_id], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    if next_key != "admin" {
        let current_key: String = tx
            .query_row(
                "SELECT role_key FROM roles WHERE id=?1",
                [&current_role_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current_key == "admin" {
            let count:i64=tx.query_row("SELECT COUNT(*) FROM users u JOIN roles r ON r.id=u.role_id JOIN businesses b ON b.id=u.business_id WHERE u.business_id=?1 AND u.is_active=1 AND (r.role_key='admin' OR b.owner_user_id=u.id)", [&actor.business_id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if count <= 1 {
                return Err("Keep at least one active administrator in the business.".into());
            }
        }
    }
    tx.execute(
        "UPDATE users SET role_id=?1 WHERE id=?2 AND business_id=?3",
        params![role_id, input.user_id, actor.business_id],
    )
    .map_err(|e| e.to_string())?;
    add_audit_event(
        &tx,
        &actor,
        "staff_role_assigned",
        "user",
        Some(&input.user_id),
        serde_json::json!({"roleKey":input.role_key}),
    )?;
    let account=tx.query_row("SELECT u.id,u.full_name,u.username,r.role_key,r.display_name,u.pin_hash IS NOT NULL,u.is_active,COALESCE(b.owner_user_id=u.id,0) FROM users u JOIN roles r ON r.id=u.role_id JOIN businesses b ON b.id=u.business_id WHERE u.id=?1 AND u.business_id=?2",params![input.user_id,actor.business_id],|r|Ok(crate::models::StaffAccount{user_id:r.get(0)?,full_name:r.get(1)?,username:r.get(2)?,role_key:r.get(3)?,role_name:r.get(4)?,has_pin:r.get::<_,i64>(5)?!=0,is_active:r.get::<_,i64>(6)?!=0,is_owner:r.get::<_,i64>(7)?!=0})).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(account)
}

#[tauri::command]
pub fn save_staff_role(
    input: SaveStaffRoleInput,
    state: State<'_, AppState>,
) -> Result<StaffRole, String> {
    let actor = { active_session(&state, "roles:manage")?.user.clone() };
    let _gate = state
        .session_gate
        .write()
        .map_err(|_| "The local session is unavailable.")?;
    require_current_owner(&state, &actor)?;
    if input.password.is_empty() {
        return Err("Confirm your password to save a role.".into());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    crate::passkeys::verify_account_password(&db, &actor.user_id, &input.password)?;
    let name = input.display_name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Role names must be between 1 and 40 characters.".into());
    }
    if !matches!(input.workspace_mode.as_str(), "management" | "cashier") {
        return Err("Choose a valid workspace.".into());
    }
    let allowed = custom_permissions();
    let unique: std::collections::HashSet<_> = input.permissions.iter().collect();
    if unique.len() != input.permissions.len()
        || input.permissions.iter().any(|p| !allowed.contains(p))
    {
        return Err("A custom role contains an unsupported permission.".into());
    }
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let role_key = input
        .role_key
        .unwrap_or_else(|| format!("custom:{}", Uuid::new_v4()));
    if !role_key.starts_with("custom:") {
        return Err("Built-in roles cannot be edited.".into());
    }
    let template_key = match input.template_key.as_str() {
        "cashier" => "cashier",
        "manager" => "manager",
        "admin" => "admin",
        _ => return Err("Choose a built-in role template.".into()),
    };
    let revision: Option<i64> = tx
        .query_row(
            "SELECT role_revision FROM roles WHERE business_id=?1 AND role_key=?2 AND is_system=0",
            params![actor.business_id, role_key],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let is_update = revision.is_some();
    if is_update && input.expected_revision != revision {
        return Err("This role changed in another session. Refresh and try again.".into());
    }
    if !is_update && input.expected_revision.is_some() {
        return Err("That role no longer exists. Refresh and try again.".into());
    }
    let before = if is_update {
        let (old_name, old_permissions, old_workspace): (String, String, String) = tx
            .query_row(
                "SELECT display_name,permissions_json,workspace_mode FROM roles WHERE business_id=?1 AND role_key=?2 AND is_system=0",
                params![actor.business_id, role_key],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        serde_json::json!({"displayName":old_name,"permissions":serde_json::from_str::<Vec<String>>(&old_permissions).unwrap_or_default(),"workspaceMode":old_workspace,"revision":revision})
    } else {
        serde_json::Value::Null
    };
    let role_id = if is_update {
        tx.query_row(
            "SELECT id FROM roles WHERE business_id=?1 AND role_key=?2",
            params![actor.business_id, role_key],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?
    } else {
        Uuid::new_v4().to_string()
    };
    let permissions_json = serde_json::to_string(&input.permissions).map_err(|e| e.to_string())?;
    let next_revision = revision.unwrap_or(0) + 1;
    if is_update {
        tx.execute("UPDATE roles SET display_name=?1,permissions_json=?2,workspace_mode=?3,role_revision=?4 WHERE id=?5",params![name,permissions_json,input.workspace_mode,next_revision,role_id]).map_err(|e|e.to_string())?;
    } else {
        tx.execute("INSERT INTO roles(id,business_id,role_key,display_name,permissions_json,is_system,workspace_mode,role_revision) VALUES(?1,?2,?3,?4,?5,0,?6,1)",params![role_id,actor.business_id,role_key,name,permissions_json,input.workspace_mode]).map_err(|e|e.to_string())?;
    }
    add_audit_event(
        &tx,
        &actor,
        if is_update {
            "staff_role_updated"
        } else {
            "staff_role_created"
        },
        "role",
        Some(&role_id),
        serde_json::json!({"roleKey":role_key,"before":before,"after":{"displayName":name,"template":template_key,"workspaceMode":input.workspace_mode.clone(),"permissions":input.permissions.clone(),"revision":next_revision}}),
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(StaffRole {
        role_key,
        display_name: name.into(),
        permissions: input.permissions,
        workspace_mode: input.workspace_mode,
        is_system: false,
        revision: next_revision,
        active_user_count: 0,
    })
}

#[tauri::command]
pub fn claim_business_owner(
    input: ClaimBusinessOwnerInput,
    state: State<'_, AppState>,
) -> Result<SessionUser, String> {
    let actor = { active_session(&state, "users:write")?.user.clone() };
    let _gate = state
        .session_gate
        .write()
        .map_err(|_| "The local session is unavailable.")?;
    let current = state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.")?
        .clone()
        .ok_or_else(|| "Sign in to continue.".to_owned())?;
    if current.user_id != actor.user_id {
        return Err("Your account changed. Sign in again.".into());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    crate::passkeys::verify_account_password(&db, &actor.user_id, &input.password)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let available:bool=tx.query_row("SELECT owner_user_id IS NULL AND EXISTS(SELECT 1 FROM users u JOIN roles r ON r.id=u.role_id WHERE u.id=?1 AND u.business_id=?2 AND u.is_active=1 AND r.role_key='admin') FROM businesses WHERE id=?2",params![actor.user_id,actor.business_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !available {
        return Err(
            "Ownership is already assigned, or this account is not an active administrator.".into(),
        );
    }
    let owner_role = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO roles(id,business_id,role_key,display_name,permissions_json,is_system,workspace_mode,role_revision) VALUES(?1,?2,'owner','Owner',?3,1,'management',1)",params![owner_role,actor.business_id,serde_json::to_string(&role_definitions_permissions("owner")).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    tx.execute(
        "UPDATE businesses SET owner_user_id=?1 WHERE id=?2 AND owner_user_id IS NULL",
        params![actor.user_id, actor.business_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE users SET role_id=?1 WHERE id=?2 AND business_id=?3",
        params![owner_role, actor.user_id, actor.business_id],
    )
    .map_err(|e| e.to_string())?;
    add_audit_event(
        &tx,
        &actor,
        "business_owner_designated",
        "business",
        Some(&actor.business_id),
        serde_json::json!({}),
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    let mut next = actor;
    next.role_key = "owner".into();
    next.role_name = "Owner".into();
    next.workspace_mode = "management".into();
    next.is_owner = true;
    next.permissions = role_definitions_permissions("owner");
    *state
        .active_session
        .lock()
        .map_err(|_| "The local session is unavailable.")? = Some(next.clone());
    Ok(next)
}
