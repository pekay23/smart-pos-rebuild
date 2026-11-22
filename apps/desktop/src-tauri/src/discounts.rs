// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use pos_core::{calculate_sale_totals, PricedLine};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{
        active_session, add_audit_event, load_register_workspace, persist_register_workspace,
        verify_password,
    },
    db::AppState,
    models::{
        ApproveTicketDiscountInput, DiscountApprover, DiscountPreview, DiscountPreviewInput,
        DiscountQuoteLine, RegisterTicket, SessionUser,
    },
};

const APPROVAL_MINUTES: i64 = 15;

#[derive(Serialize)]
struct QuoteLine<'a> {
    product_id: &'a str,
    quantity: i64,
    unit_price_minor: i64,
}

#[derive(Serialize)]
struct DiscountQuote<'a> {
    schema_version: u8,
    ticket_id: &'a str,
    business_id: &'a str,
    store_id: &'a str,
    customer_id: &'a Option<String>,
    requester_user_id: &'a Option<String>,
    currency_code: &'a str,
    reason: &'a Option<String>,
    subtotal_minor: i64,
    discount_minor: i64,
    tax_rate_bps: i64,
    tax_minor: i64,
    total_minor: i64,
    lines: Vec<QuoteLine<'a>>,
}

#[derive(Debug)]
pub(crate) struct DiscountApprovalProof {
    pub id: String,
    pub requester_user_id: String,
    pub approver_user_id: String,
}

pub(crate) fn tax_rate_bps(db: &Connection, store_id: &str) -> Result<i64, String> {
    db.execute(
        "INSERT OR IGNORE INTO store_preferences(store_id) VALUES(?1)",
        [store_id],
    )
    .map_err(|error| error.to_string())?;
    db.query_row(
        "SELECT tax_rate_bps FROM store_preferences WHERE store_id=?1",
        [store_id],
        |row| row.get(0),
    )
    .map_err(|error| error.to_string())
}

fn quote_json(
    db: &Connection,
    session: &SessionUser,
    ticket: &RegisterTicket,
) -> Result<String, String> {
    if ticket.discount_minor <= 0 {
        return Err("Enter a discount greater than zero before requesting approval.".into());
    }
    if ticket
        .discount_reason
        .as_deref()
        .is_none_or(|reason| reason.trim().len() < 3)
    {
        return Err("Enter a short reason for the discount before approval.".into());
    }
    let tax_rate = tax_rate_bps(db, &session.store_id)?;
    let mut lines = Vec::with_capacity(ticket.lines.len());
    let mut priced = Vec::with_capacity(ticket.lines.len());
    for line in &ticket.lines {
        if line.quantity <= 0 {
            return Err("The ticket contains an invalid quantity.".into());
        }
        let current: Option<(i64, i64)> = db
            .query_row(
                "SELECT price_minor,is_active FROM products WHERE id=?1 AND store_id=?2",
                params![line.product_id, session.store_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((price, is_active)) = current else {
            return Err("A product on this ticket is no longer available.".into());
        };
        if is_active == 0 || price != line.accepted_unit_price_minor {
            return Err(
                "Review current product prices before asking for discount approval.".into(),
            );
        }
        lines.push(QuoteLine {
            product_id: &line.product_id,
            quantity: line.quantity,
            unit_price_minor: line.accepted_unit_price_minor,
        });
        priced.push(PricedLine {
            quantity: line.quantity,
            unit_price_minor: line.accepted_unit_price_minor,
        });
    }
    lines.sort_by(|a, b| a.product_id.cmp(b.product_id));
    let totals = calculate_sale_totals(&priced, ticket.discount_minor, tax_rate)
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&DiscountQuote {
        schema_version: 1,
        ticket_id: &ticket.id,
        business_id: &session.business_id,
        store_id: &session.store_id,
        customer_id: &ticket.customer_id,
        requester_user_id: &ticket.discount_requested_by_user_id,
        currency_code: &session.currency_code,
        reason: &ticket.discount_reason,
        subtotal_minor: totals.subtotal_minor,
        discount_minor: totals.discount_minor,
        tax_rate_bps: tax_rate,
        tax_minor: totals.tax_minor,
        total_minor: totals.total_minor,
        lines,
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn invalidate_ticket_approval(
    db: &Connection,
    session: &SessionUser,
    ticket: &mut RegisterTicket,
    reason: &str,
) -> Result<(), String> {
    if let Some(id) = ticket.discount_approval_id.as_deref() {
        let changed = db
            .execute(
                "UPDATE discount_approvals SET invalidated_at=?1,invalidation_reason=?2 WHERE id=?3 AND store_id=?4 AND invalidated_at IS NULL AND consumed_sale_id IS NULL",
                params![Utc::now().to_rfc3339_opts(SecondsFormat::Millis,true),reason,id,session.store_id],
            )
            .map_err(|e| e.to_string())?;
        if changed > 0 {
            add_audit_event(
                db,
                session,
                "discount_approval_invalidated",
                "register_ticket",
                Some(&ticket.id),
                serde_json::json!({"approvalId":id,"reason":reason}),
            )?;
        }
    }
    ticket.discount_approval_id = None;
    ticket.discount_approver_name = None;
    ticket.discount_approval_expires_at = None;
    Ok(())
}

pub(crate) fn invalidate_approval_for_ticket(
    db: &Connection,
    session: &SessionUser,
    ticket_id: &str,
    reason: &str,
) -> Result<(), String> {
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut query=db.prepare("SELECT id FROM discount_approvals WHERE store_id=?1 AND ticket_id=?2 AND invalidated_at IS NULL AND consumed_sale_id IS NULL").map_err(|e|e.to_string())?;
    let rows = query
        .query_map(params![session.store_id, ticket_id], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(query);
    if rows.is_empty() {
        return Ok(());
    }
    db.execute("UPDATE discount_approvals SET invalidated_at=?1,invalidation_reason=?2 WHERE store_id=?3 AND ticket_id=?4 AND invalidated_at IS NULL AND consumed_sale_id IS NULL",params![timestamp,reason,session.store_id,ticket_id]).map_err(|e|e.to_string())?;
    for id in rows {
        add_audit_event(
            db,
            session,
            "discount_approval_invalidated",
            "register_ticket",
            Some(ticket_id),
            serde_json::json!({"approvalId":id,"reason":reason}),
        )?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_discount_approvers(
    state: State<'_, AppState>,
) -> Result<Vec<DiscountApprover>, String> {
    let session = active_session(&state, "pos:use")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let mut query = db
        .prepare("SELECT DISTINCT u.id,u.full_name,r.display_name FROM users u JOIN roles r ON r.id=u.role_id JOIN json_each(r.permissions_json) p WHERE u.business_id=?1 AND u.is_active=1 AND p.value='sales:discount:approve' ORDER BY u.full_name COLLATE NOCASE")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([&session.business_id], |r| {
            Ok(DiscountApprover {
                user_id: r.get(0)?,
                full_name: r.get(1)?,
                role_name: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

#[tauri::command]
pub fn get_ticket_discount_preview(
    input: DiscountPreviewInput,
    state: State<'_, AppState>,
) -> Result<DiscountPreview, String> {
    let session = active_session(&state, "pos:use")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let workspace = load_register_workspace(&db, &session)?;
    if workspace.revision != input.expected_workspace_revision
        || workspace.active.id != input.ticket_id
    {
        return Err("The register changed. Review the current ticket again.".into());
    }
    let ticket = &workspace.active;
    let quote_json = quote_json(&db, &session, ticket)?;
    let _ = quote_json;
    let mut priced = Vec::with_capacity(ticket.lines.len());
    let mut lines = Vec::with_capacity(ticket.lines.len());
    for line in &ticket.lines {
        priced.push(PricedLine {
            quantity: line.quantity,
            unit_price_minor: line.accepted_unit_price_minor,
        });
        lines.push(DiscountQuoteLine {
            name: line.accepted_name.clone(),
            quantity: line.quantity,
            unit_price_minor: line.accepted_unit_price_minor,
            line_total_minor: i64::try_from(
                i128::from(line.quantity) * i128::from(line.accepted_unit_price_minor),
            )
            .map_err(|_| "The ticket total is too large.")?,
        });
    }
    let subtotal_minor = lines.iter().try_fold(0_i64, |sum, line| {
        sum.checked_add(line.line_total_minor)
            .ok_or_else(|| "The ticket total is too large.".to_owned())
    })?;
    let totals = calculate_sale_totals(
        &priced,
        ticket.discount_minor,
        tax_rate_bps(&db, &session.store_id)?,
    )
    .map_err(|e| e.to_string())?;
    Ok(DiscountPreview {
        ticket_id: ticket.id.clone(),
        workspace_revision: workspace.revision,
        currency_code: session.currency_code.clone(),
        reason: ticket.discount_reason.clone().unwrap_or_default(),
        lines,
        subtotal_minor,
        discount_minor: ticket.discount_minor,
        tax_minor: totals.tax_minor,
        total_minor: totals.total_minor,
    })
}

#[tauri::command]
pub fn approve_ticket_discount(
    input: ApproveTicketDiscountInput,
    state: State<'_, AppState>,
) -> Result<crate::models::RegisterWorkspaceView, String> {
    let session = active_session(&state, "pos:use")?;
    if input.credential.is_empty() || input.credential.chars().count() > 128 {
        return Err("Enter the approver's personal PIN or password.".into());
    }
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let mut workspace = load_register_workspace(&tx, &session)?;
    if workspace.revision != input.expected_workspace_revision
        || workspace.active.id != input.ticket_id
    {
        return Err("The register changed. Review the ticket again before approving it.".into());
    }
    let ticket = workspace.active.clone();
    if ticket.discount_minor != input.expected_discount_minor || ticket.discount_minor <= 0 {
        return Err(
            "The discount request changed. Review the current amount before approving it.".into(),
        );
    }
    let Some(requester_user_id) = ticket.discount_requested_by_user_id.clone() else {
        return Err(
            "This discount request has no recorded cashier. Clear it and request approval again."
                .into(),
        );
    };
    let approval_reason = ticket
        .discount_reason
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_owned();
    if approval_reason.len() < 3 || approval_reason.len() > 200 {
        return Err("The cashier must provide a reason between 3 and 200 characters.".into());
    }

    let manager: Option<(String, Option<String>, String, String)> = tx
        .query_row(
            "SELECT u.password_hash,u.pin_hash,u.full_name,r.permissions_json FROM users u JOIN roles r ON r.id=u.role_id AND r.business_id=u.business_id WHERE u.id=?1 AND u.business_id=?2 AND u.is_active=1",
            params![input.approver_user_id,session.business_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((password_hash, pin_hash, approver_name, permissions_json)) = manager else {
        return Err("Choose an active discount approver in this business.".into());
    };
    let permissions: Vec<String> = serde_json::from_str(&permissions_json).unwrap_or_default();
    if !permissions.iter().any(|p| p == "sales:discount:approve") {
        return Err("That account no longer has discount approval access.".into());
    }

    let locked: Option<String> = tx
        .query_row("SELECT locked_until FROM discount_approval_attempts WHERE store_id=?1 AND requester_user_id=?2 AND approver_user_id=?3",params![session.store_id,requester_user_id,input.approver_user_id],|r|r.get(0))
        .optional().map_err(|e|e.to_string())?;
    if let Some(remaining_seconds) = locked
        .as_deref()
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|until| {
            let milliseconds = until
                .with_timezone(&Utc)
                .signed_duration_since(Utc::now())
                .num_milliseconds()
                .max(0);
            (milliseconds + 999) / 1000
        })
        .filter(|seconds| *seconds > 0)
    {
        return Err(format!(
            "Too many attempts. Wait {remaining_seconds} seconds and try again."
        ));
    }
    let credential_ok = verify_password(&input.credential, &password_hash)
        || pin_hash
            .as_deref()
            .is_some_and(|hash| verify_password(&input.credential, hash));
    if !credential_ok {
        let failures:i64=tx.query_row("SELECT failed_attempts FROM discount_approval_attempts WHERE store_id=?1 AND requester_user_id=?2 AND approver_user_id=?3",params![session.store_id,requester_user_id,input.approver_user_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?.unwrap_or(0)+1;
        let lock_until = if failures >= 5 {
            Some((Utc::now() + Duration::seconds(30)).to_rfc3339_opts(SecondsFormat::Millis, true))
        } else {
            None
        };
        tx.execute("INSERT INTO discount_approval_attempts(store_id,requester_user_id,approver_user_id,failed_attempts,locked_until) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(store_id,requester_user_id,approver_user_id) DO UPDATE SET failed_attempts=excluded.failed_attempts,locked_until=excluded.locked_until",params![session.store_id,requester_user_id,input.approver_user_id,failures,lock_until]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        return Err("The approver PIN or password was not recognized.".into());
    }
    tx.execute("DELETE FROM discount_approval_attempts WHERE store_id=?1 AND requester_user_id=?2 AND approver_user_id=?3",params![session.store_id,requester_user_id,input.approver_user_id]).map_err(|e|e.to_string())?;
    let quote = quote_json(&tx, &session, &ticket)?;
    let approval_id = Uuid::new_v4().to_string();
    let approved_at = Utc::now();
    let expires_at = (approved_at + Duration::minutes(APPROVAL_MINUTES))
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    invalidate_ticket_approval(&tx, &session, &mut workspace.active, "discount_reapproved")?;
    tx.execute("INSERT INTO discount_approvals(id,business_id,store_id,ticket_id,requester_user_id,approver_user_id,discount_minor,reason,quote_json,approved_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![approval_id,session.business_id,session.store_id,input.ticket_id,requester_user_id,input.approver_user_id,input.expected_discount_minor,approval_reason,quote,approved_at.to_rfc3339_opts(SecondsFormat::Millis,true),expires_at]).map_err(|e|e.to_string())?;
    workspace.active.discount_approval_id = Some(approval_id.clone());
    workspace.active.discount_approver_name = Some(approver_name.clone());
    workspace.active.discount_approval_expires_at = Some(expires_at.clone());
    persist_register_workspace(
        &tx,
        &session,
        input.expected_workspace_revision,
        &mut workspace,
    )?;
    let mut actor = (*session).clone();
    actor.user_id = input.approver_user_id.clone();
    actor.full_name = approver_name;
    actor.permissions = permissions;
    add_audit_event(
        &tx,
        &actor,
        "ticket_discount_approved",
        "register_ticket",
        Some(&input.ticket_id),
        serde_json::json!({"requesterUserId":requester_user_id,"approverUserId":input.approver_user_id,"amountMinor":input.expected_discount_minor,"reason":approval_reason,"approvalId":approval_id,"expiresAt":expires_at}),
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(workspace)
}

pub(crate) fn validate_checkout_discount(
    db: &Connection,
    session: &SessionUser,
    ticket: &RegisterTicket,
) -> Result<DiscountApprovalProof, String> {
    let Some(approval_id) = ticket.discount_approval_id.as_deref() else {
        return Err("This discount needs manager approval before checkout.".into());
    };
    let requester = ticket
        .discount_requested_by_user_id
        .as_deref()
        .ok_or_else(|| "This discount has no recorded requester.".to_owned())?;
    let expected_quote = quote_json(db, session, ticket)?;
    let row:Option<(String,String,String,i64,String,String,Option<String>,Option<String>,String)>=db.query_row("SELECT requester_user_id,approver_user_id,business_id,discount_minor,quote_json,expires_at,invalidated_at,consumed_sale_id,id FROM discount_approvals WHERE id=?1 AND store_id=?2 AND ticket_id=?3",params![approval_id,session.store_id,ticket.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional().map_err(|e|e.to_string())?;
    let Some((
        saved_requester,
        approver,
        business_id,
        amount,
        quote,
        expires_at,
        invalidated,
        consumed,
        _,
    )) = row
    else {
        return Err("This discount approval could not be found. Ask for approval again.".into());
    };
    if business_id != session.business_id
        || saved_requester != requester
        || amount != ticket.discount_minor
        || quote != expected_quote
        || invalidated.is_some()
        || consumed.is_some()
    {
        return Err(
            "The approved discount or ticket details changed. Ask for approval again.".into(),
        );
    }
    if DateTime::parse_from_rfc3339(&expires_at)
        .map(|v| v.with_timezone(&Utc) <= Utc::now())
        .unwrap_or(true)
    {
        return Err("Discount approval expired. Ask a manager to approve it again.".into());
    }
    let still_allowed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM users u JOIN roles r ON r.id=u.role_id JOIN json_each(r.permissions_json) p WHERE u.id=?1 AND u.business_id=?2 AND u.is_active=1 AND p.value='sales:discount:approve')",params![approver,session.business_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !still_allowed {
        return Err(
            "The approving account is no longer active or authorized. Ask for approval again."
                .into(),
        );
    }
    Ok(DiscountApprovalProof {
        id: approval_id.into(),
        requester_user_id: requester.into(),
        approver_user_id: approver,
    })
}

pub(crate) fn consume_checkout_discount(
    tx: &Transaction<'_>,
    proof: &DiscountApprovalProof,
    store_id: &str,
    sale_id: &str,
) -> Result<(), String> {
    let changed=tx.execute("UPDATE discount_approvals SET consumed_sale_id=?1 WHERE id=?2 AND store_id=?3 AND requester_user_id=?4 AND approver_user_id=?5 AND consumed_sale_id IS NULL AND invalidated_at IS NULL AND expires_at>?6",params![sale_id,proof.id,store_id,proof.requester_user_id,proof.approver_user_id,Utc::now().to_rfc3339_opts(SecondsFormat::Millis,true)]).map_err(|e|e.to_string())?;
    if changed != 1 {
        return Err("This discount approval expired or was already used. Ask a manager to approve it again.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PaymentMethod, RegisterTicketLine};

    fn approved_fixture() -> (Connection, SessionUser, RegisterTicket) {
        let db = Connection::open_in_memory().expect("in-memory database");
        db.execute_batch(
            "CREATE TABLE store_preferences(store_id TEXT PRIMARY KEY,tax_rate_bps INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE products(id TEXT,store_id TEXT,price_minor INTEGER,is_active INTEGER);
             CREATE TABLE roles(id TEXT,business_id TEXT,permissions_json TEXT);
             CREATE TABLE users(id TEXT,business_id TEXT,role_id TEXT,is_active INTEGER);
             CREATE TABLE discount_approvals(
               id TEXT PRIMARY KEY,store_id TEXT,ticket_id TEXT,requester_user_id TEXT,
               approver_user_id TEXT,business_id TEXT,discount_minor INTEGER,quote_json TEXT,
               expires_at TEXT,invalidated_at TEXT,consumed_sale_id TEXT);
             INSERT INTO products VALUES('product-1','store-1',1000,1);
             INSERT INTO roles VALUES('role-1','business-1','[\"sales:discount:approve\"]');
             INSERT INTO users VALUES('manager-1','business-1','role-1',1);
             INSERT INTO store_preferences VALUES('store-1',0);",
        )
        .expect("fixture schema");
        let session = SessionUser {
            user_id: "cashier-1".into(),
            business_id: "business-1".into(),
            store_id: "store-1".into(),
            full_name: "Cashier One".into(),
            username: "cashier".into(),
            role_key: "cashier".into(),
            role_name: "Cashier".into(),
            workspace_mode: "cashier".into(),
            is_owner: false,
            permissions: vec!["pos:use".into()],
            business_name: "Demo".into(),
            store_name: "Main".into(),
            currency_code: "GHS".into(),
        };
        let mut ticket = RegisterTicket {
            payment_option_id: None,
            payment_reference: String::new(),
            id: "ticket-1".into(),
            ticket_number: "T-1".into(),
            creator_user_id: session.user_id.clone(),
            creator_name: session.full_name.clone(),
            last_editor_user_id: session.user_id.clone(),
            last_editor_name: session.full_name.clone(),
            customer_id: None,
            lines: vec![RegisterTicketLine {
                product_id: "product-1".into(),
                quantity: 1,
                accepted_name: "Tea".into(),
                accepted_sku: "TEA".into(),
                accepted_unit_price_minor: 1000,
            }],
            discount_minor: 100,
            discount_reason: Some("Price match".into()),
            discount_requested_by_user_id: Some(session.user_id.clone()),
            discount_approval_id: Some("approval-1".into()),
            discount_approver_name: Some("Manager One".into()),
            discount_approval_expires_at: None,
            payment_method: PaymentMethod::Cash,
            amount_tendered_minor: 0,
            created_at: "2026-10-30T00:00:00Z".into(),
            updated_at: "2026-10-30T00:00:00Z".into(),
        };
        let quote = quote_json(&db, &session, &ticket).expect("canonical quote");
        let expires_at =
            (Utc::now() + Duration::minutes(15)).to_rfc3339_opts(SecondsFormat::Millis, true);
        ticket.discount_approval_expires_at = Some(expires_at.clone());
        db.execute(
            "INSERT INTO discount_approvals VALUES('approval-1','store-1','ticket-1','cashier-1',
             'manager-1','business-1',100,?1,?2,NULL,NULL)",
            params![quote, expires_at],
        )
        .expect("saved approval");
        (db, session, ticket)
    }

    #[test]
    fn approval_matches_saved_quote_and_rejects_ticket_changes() {
        let (db, session, mut ticket) = approved_fixture();
        assert!(validate_checkout_discount(&db, &session, &ticket).is_ok());
        ticket.lines[0].quantity = 2;
        assert!(validate_checkout_discount(&db, &session, &ticket)
            .unwrap_err()
            .contains("ticket details changed"));
    }

    #[test]
    fn approval_rejects_expiry_and_approver_access_changes() {
        let (db, session, ticket) = approved_fixture();
        db.execute("UPDATE users SET is_active=0 WHERE id='manager-1'", [])
            .expect("deactivate manager");
        assert!(validate_checkout_discount(&db, &session, &ticket)
            .unwrap_err()
            .contains("no longer active or authorized"));
        db.execute("UPDATE users SET is_active=1 WHERE id='manager-1'", [])
            .expect("reactivate manager");
        db.execute(
            "UPDATE roles SET permissions_json='[]' WHERE id='role-1'",
            [],
        )
        .expect("remove approval access");
        assert!(validate_checkout_discount(&db, &session, &ticket).is_err());
        db.execute(
            "UPDATE roles SET permissions_json='[\"sales:discount:approve\"]' WHERE id='role-1'",
            [],
        )
        .expect("restore approval access");
        db.execute(
            "UPDATE discount_approvals SET expires_at=?1 WHERE id='approval-1'",
            [(Utc::now() - Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::Millis, true)],
        )
        .expect("expire approval");
        assert!(validate_checkout_discount(&db, &session, &ticket)
            .unwrap_err()
            .contains("expired"));
    }

    #[test]
    fn approval_is_consumed_only_once() {
        let (mut db, session, ticket) = approved_fixture();
        let proof = validate_checkout_discount(&db, &session, &ticket).expect("valid approval");
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("checkout transaction");
        consume_checkout_discount(&tx, &proof, &session.store_id, "sale-1")
            .expect("first consumption");
        tx.commit().expect("commit consumption");
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("second checkout transaction");
        assert!(consume_checkout_discount(&tx, &proof, &session.store_id, "sale-2").is_err());
    }
}
