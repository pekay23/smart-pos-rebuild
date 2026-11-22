// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::{Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};

#[derive(Debug, Clone, Copy)]
struct AccountDefinition {
    code: &'static str,
    name: &'static str,
    kind: &'static str,
}

const ACCOUNTS: &[AccountDefinition] = &[
    AccountDefinition {
        code: "1000",
        name: "Cash on hand",
        kind: "asset",
    },
    AccountDefinition {
        code: "1010",
        name: "Card settlement clearing",
        kind: "asset",
    },
    AccountDefinition {
        code: "1020",
        name: "Mobile money settlement clearing",
        kind: "asset",
    },
    AccountDefinition {
        code: "1030",
        name: "Bank transfer clearing",
        kind: "asset",
    },
    AccountDefinition {
        code: "1090",
        name: "Unclassified tender clearing",
        kind: "asset",
    },
    AccountDefinition {
        code: "1300",
        name: "Inventory at recorded cost",
        kind: "asset",
    },
    AccountDefinition {
        code: "2000",
        name: "Accounts payable",
        kind: "liability",
    },
    AccountDefinition {
        code: "2100",
        name: "Tax collected, classification pending",
        kind: "liability",
    },
    AccountDefinition {
        code: "3000",
        name: "Opening balance equity",
        kind: "equity",
    },
    AccountDefinition {
        code: "2110",
        name: "Goods received not invoiced",
        kind: "liability",
    },
    AccountDefinition {
        code: "4000",
        name: "Sales revenue",
        kind: "revenue",
    },
    AccountDefinition {
        code: "4010",
        name: "Sales discounts",
        kind: "contra_revenue",
    },
    AccountDefinition {
        code: "4020",
        name: "Sales returns",
        kind: "contra_revenue",
    },
    AccountDefinition {
        code: "5000",
        name: "Cost of goods sold",
        kind: "expense",
    },
    AccountDefinition {
        code: "6000",
        name: "Operating expenses",
        kind: "expense",
    },
];

#[derive(Debug, Clone)]
struct Actor<'a> {
    business_id: &'a str,
    store_id: &'a str,
    user_id: Option<&'a str>,
    currency_code: &'a str,
}

#[derive(Debug, Clone)]
struct Line {
    account_code: &'static str,
    memo: String,
    debit_minor: i64,
    credit_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalanceRow {
    pub code: String,
    pub name: String,
    pub account_type: String,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub balance_minor: i64,
    pub journal_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountingCoverage {
    pub sales_count: i64,
    pub unposted_sales_count: i64,
    pub refunds_count: i64,
    pub unposted_refunds_count: i64,
    pub expenses_count: i64,
    pub unposted_expenses_count: i64,
    pub purchases_count: i64,
    pub unposted_purchases_count: i64,
    pub supplier_bills_count: i64,
    pub unposted_supplier_bills_count: i64,
    pub supplier_payments_count: i64,
    pub unposted_supplier_payments_count: i64,
    pub partial_journal_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalance {
    pub rows: Vec<TrialBalanceRow>,
    pub total_debits_minor: i64,
    pub total_credits_minor: i64,
    pub is_balanced: bool,
    pub coverage: AccountingCoverage,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalanceInput {
    pub from_date: String,
    pub to_date_inclusive: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountingPeriodCloseInput {
    pub from_date: String,
    pub to_date_inclusive: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountingPeriodClosure {
    pub id: String,
    pub business_id: String,
    pub store_id: String,
    pub from_utc: String,
    pub to_utc_exclusive: String,
    pub timezone: String,
    pub currency_code: String,
    pub snapshot_json: String,
    pub snapshot_sha256: String,
    pub closed_by: String,
    pub closed_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashOpeningBalance {
    pub amount_minor: i64,
    pub currency_code: String,
    pub effective_at: String,
    pub reason: String,
    pub recorded_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashOpeningBalanceInput {
    pub amount_minor: i64,
    pub effective_at: String,
    pub reason: String,
    pub idempotency_key: String,
}

fn cash_opening_balance_for_store(
    connection: &Connection,
    business_id: &str,
    store_id: &str,
) -> Result<Option<CashOpeningBalance>, String> {
    connection
        .query_row(
            "SELECT amount_minor,currency_code,effective_at,reason,recorded_at
         FROM cash_opening_balances WHERE business_id=?1 AND store_id=?2",
            params![business_id, store_id],
            |row| {
                Ok(CashOpeningBalance {
                    amount_minor: row.get(0)?,
                    currency_code: row.get(1)?,
                    effective_at: row.get(2)?,
                    reason: row.get(3)?,
                    recorded_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn replay_cash_opening_balance(
    connection: &Connection,
    business_id: &str,
    store_id: &str,
    idempotency_key: &str,
    amount_minor: i64,
    effective_at: &str,
    reason: &str,
) -> Result<Option<CashOpeningBalance>, String> {
    let Some(existing) = cash_opening_balance_for_store(connection, business_id, store_id)? else {
        return Ok(None);
    };
    let existing_key: String = connection
        .query_row(
            "SELECT idempotency_key FROM cash_opening_balances WHERE business_id=?1 AND store_id=?2",
            params![business_id, store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if existing_key == idempotency_key
        && existing.amount_minor == amount_minor
        && existing.effective_at == effective_at
        && existing.reason == reason
    {
        return Ok(Some(existing));
    }
    Err("An opening cash balance is already recorded for this store. It cannot be replaced; use an authorized adjustment.".to_owned())
}

fn post_cash_opening_journal(
    connection: &Connection,
    actor: &Actor<'_>,
    source_id: &str,
    amount_minor: i64,
    effective_at: &str,
) -> Result<String, String> {
    if amount_minor <= 0 {
        return Err("The opening cash journal amount must be positive.".to_owned());
    }
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        "1000",
        "Cash on hand at accounting cutover",
        amount_minor,
        0,
    )?;
    append_line(
        &mut lines,
        "3000",
        "Opening balance equity",
        0,
        amount_minor,
    )?;
    insert_journal(
        connection,
        actor,
        "adjustment",
        source_id,
        effective_at,
        "Opening cash balance at cutover",
        "complete",
        &lines,
    )?;
    connection
        .query_row(
            "SELECT id FROM accounting_journals WHERE business_id=?1 AND source_type='adjustment' AND source_id=?2",
            params![actor.business_id, source_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn ensure_cash_cutover_has_no_prior_activity(
    connection: &Connection,
    business_id: &str,
    store_id: &str,
    effective_at: &str,
) -> Result<(), String> {
    let prior_cash_activity: bool = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM accounting_journals j
                JOIN accounting_journal_lines l ON l.journal_id=j.id AND l.business_id=j.business_id
                JOIN accounting_accounts a ON a.id=l.account_id AND a.business_id=j.business_id
                WHERE j.business_id=?1 AND j.store_id=?2 AND a.code='1000' AND j.effective_at<=?3
             )",
            params![business_id, store_id, effective_at],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if prior_cash_activity {
        return Err("Cash-on-hand journal activity already exists on or before this cutoff date. Choose a cutoff before the first cash entry; existing cash activity is never offset or counted again by this setup.".to_owned());
    }
    Ok(())
}

#[tauri::command]
pub fn get_cash_opening_balance(
    state: tauri::State<'_, AppState>,
) -> Result<Option<CashOpeningBalance>, String> {
    let session = active_session(&state, "")?;
    if !session.is_owner {
        return Err("Only the business owner can view or set the opening cash balance.".to_owned());
    }
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    cash_opening_balance_for_store(&connection, &session.business_id, &session.store_id)
}

#[tauri::command]
pub fn set_cash_opening_balance(
    input: CashOpeningBalanceInput,
    state: tauri::State<'_, AppState>,
) -> Result<CashOpeningBalance, String> {
    let session = active_session(&state, "")?;
    if !session.is_owner {
        return Err("Only the business owner can set the opening cash balance.".to_owned());
    }
    if input.amount_minor <= 0 || input.amount_minor > 9_000_000_000_000 {
        return Err(
            "Enter an opening cash balance greater than zero and within the supported range."
                .to_owned(),
        );
    }
    let effective_at = crate::sales::canonical_timestamp(&input.effective_at)?;
    let reason = input.reason.trim();
    if reason.chars().count() < 3 || reason.chars().count() > 200 {
        return Err("Give a reason between 3 and 200 characters.".to_owned());
    }
    let idempotency_key = input.idempotency_key.trim();
    if idempotency_key.is_empty() || idempotency_key.len() > 100 {
        return Err("A retry key between 1 and 100 characters is required.".to_owned());
    }

    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(existing) = replay_cash_opening_balance(
        &tx,
        &session.business_id,
        &session.store_id,
        idempotency_key,
        input.amount_minor,
        &effective_at,
        reason,
    )? {
        return Ok(existing);
    }

    ensure_cash_cutover_has_no_prior_activity(
        &tx,
        &session.business_id,
        &session.store_id,
        &effective_at,
    )?;
    let id = Uuid::new_v4().to_string();
    let journal_source = format!("cash-opening-{id}");
    let actor = Actor {
        business_id: &session.business_id,
        store_id: &session.store_id,
        user_id: Some(&session.user_id),
        currency_code: &session.currency_code,
    };
    let journal_id = post_cash_opening_journal(
        &tx,
        &actor,
        &journal_source,
        input.amount_minor,
        &effective_at,
    )?;
    let recorded_at = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO cash_opening_balances(id,business_id,store_id,currency_code,amount_minor,effective_at,reason,idempotency_key,journal_id,recorded_by,recorded_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![id,session.business_id,session.store_id,session.currency_code,input.amount_minor,effective_at,reason,idempotency_key,journal_id,session.user_id,recorded_at],
    ).map_err(|error| format!("Could not save the opening cash balance: {error}"))?;
    crate::commands::add_audit_event(
        &tx,
        &session,
        "cash_opening_balance_recorded",
        "cash_opening_balance",
        Some(&id),
        serde_json::json!({"amountMinor":input.amount_minor,"currencyCode":session.currency_code,"effectiveAt":effective_at,"reason":reason,"journalId":journal_id}),
    )?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(CashOpeningBalance {
        amount_minor: input.amount_minor,
        currency_code: session.currency_code.clone(),
        effective_at,
        reason: reason.to_owned(),
        recorded_at,
    })
}

fn ensure_accounts(db: &Connection, business_id: &str) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    for account in ACCOUNTS {
        db.execute(
            "INSERT INTO accounting_accounts(id,business_id,code,name,account_type,is_system,is_active,created_at)
             VALUES(?1,?2,?3,?4,?5,1,1,?6)
             ON CONFLICT(business_id,code) DO NOTHING",
            params![Uuid::new_v4().to_string(), business_id, account.code, account.name, account.kind, now],
        )
        .map_err(|error| format!("Could not prepare the accounting chart: {error}"))?;
    }
    Ok(())
}

fn tender_account(method: &str) -> (&'static str, bool) {
    match method {
        "cash" => ("1000", false),
        "card" => ("1010", false),
        "mobile_money" => ("1020", false),
        "bank_transfer" => ("1030", false),
        _ => ("1090", true),
    }
}

fn append_line(
    lines: &mut Vec<Line>,
    account_code: &'static str,
    memo: impl Into<String>,
    debit_minor: i64,
    credit_minor: i64,
) -> Result<(), String> {
    if debit_minor < 0 || credit_minor < 0 || (debit_minor == 0) == (credit_minor == 0) {
        return Err("Accounting lines must have one positive debit or credit.".to_owned());
    }
    lines.push(Line {
        account_code,
        memo: memo.into(),
        debit_minor,
        credit_minor,
    });
    Ok(())
}

fn validate_balance(lines: &[Line]) -> Result<(), String> {
    if lines.len() < 2 {
        return Err("A journal needs at least two accounting lines.".to_owned());
    }
    let (debits, credits) = lines
        .iter()
        .try_fold((0_i64, 0_i64), |(debits, credits), line| {
            Ok::<_, String>((
                debits
                    .checked_add(line.debit_minor)
                    .ok_or_else(|| "Accounting debit total is too large.".to_owned())?,
                credits
                    .checked_add(line.credit_minor)
                    .ok_or_else(|| "Accounting credit total is too large.".to_owned())?,
            ))
        })?;
    if debits != credits {
        return Err(
            "The accounting entry is not balanced; the source transaction was stopped.".to_owned(),
        );
    }
    Ok(())
}

fn insert_journal(
    db: &Connection,
    actor: &Actor<'_>,
    source_type: &str,
    source_id: &str,
    effective_at: &str,
    description: &str,
    coverage_status: &str,
    lines: &[Line],
) -> Result<bool, String> {
    insert_journal_with_reversal(
        db,
        actor,
        source_type,
        source_id,
        effective_at,
        description,
        coverage_status,
        lines,
        None,
    )
}

fn insert_journal_with_reversal(
    db: &Connection,
    actor: &Actor<'_>,
    source_type: &str,
    source_id: &str,
    effective_at: &str,
    description: &str,
    coverage_status: &str,
    lines: &[Line],
    reversal_of: Option<&str>,
) -> Result<bool, String> {
    ensure_accounts(db, actor.business_id)?;
    validate_balance(lines)?;
    let existing: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM accounting_journals WHERE business_id=?1 AND source_type=?2 AND source_id=?3)",
            params![actor.business_id, source_type, source_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if existing {
        return Ok(false);
    }
    let journal_id = Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO accounting_journals(id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,created_by,description,coverage_status,reversal_of)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![journal_id, actor.business_id, actor.store_id, source_type, source_id, actor.currency_code, effective_at, Utc::now().to_rfc3339(), actor.user_id, description, coverage_status, reversal_of],
    )
    .map_err(|error| format!("Could not record the accounting entry: {error}"))?;
    for (index, line) in lines.iter().enumerate() {
        let account_id: String = db
            .query_row(
                "SELECT id FROM accounting_accounts WHERE business_id=?1 AND code=?2 AND is_active=1",
                params![actor.business_id, line.account_code],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "A required accounting account is unavailable.".to_owned())?;
        db.execute(
            "INSERT INTO accounting_journal_lines(id,business_id,journal_id,account_id,line_number,memo,debit_minor,credit_minor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Uuid::new_v4().to_string(), actor.business_id, journal_id, account_id, index as i64 + 1, line.memo, line.debit_minor, line.credit_minor],
        )
        .map_err(|error| format!("Could not record an accounting line: {error}"))?;
    }
    Ok(true)
}

fn post_sale(db: &Connection, actor: &Actor<'_>, sale_id: &str) -> Result<bool, String> {
    let sale: Option<(i64, i64, i64, i64, String)> = db
        .query_row(
            "SELECT subtotal_minor,discount_minor,tax_minor,total_minor,created_at FROM sales WHERE id=?1 AND store_id=?2",
            params![sale_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (subtotal, discount, tax, total, effective_at) =
        sale.ok_or_else(|| "The sale for this accounting entry is missing.".to_owned())?;
    if subtotal < 0
        || discount < 0
        || tax < 0
        || total <= 0
        || subtotal
            .checked_sub(discount)
            .and_then(|value| value.checked_add(tax))
            != Some(total)
    {
        return Err("The saved sale amounts do not reconcile for accounting.".to_owned());
    }
    let mut lines = Vec::new();
    let mut tenders = db
        .prepare("SELECT method,amount_minor FROM payments WHERE sale_id=?1 ORDER BY id")
        .map_err(|error| error.to_string())?;
    let tender_rows = tenders
        .query_map([sale_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tenders);
    let tender_total = tender_rows.iter().try_fold(0_i64, |sum, (_, amount)| {
        sum.checked_add(*amount)
            .ok_or_else(|| "The sale tender total is too large.".to_owned())
    })?;
    if tender_rows.is_empty()
        || tender_total != total
        || tender_rows.iter().any(|(_, amount)| *amount <= 0)
    {
        return Err("Saved sale tenders do not match the sale total.".to_owned());
    }
    let mut partial = false;
    for (method, amount) in tender_rows {
        let (code, is_unclassified) = tender_account(&method);
        partial |= is_unclassified;
        append_line(
            &mut lines,
            code,
            format!("Sale tender: {method}"),
            amount,
            0,
        )?;
    }
    if discount > 0 {
        append_line(&mut lines, "4010", "Discounts on sale", discount, 0)?;
    }
    if subtotal > 0 {
        append_line(&mut lines, "4000", "Gross sale revenue", 0, subtotal)?;
    }
    if tax > 0 {
        partial = true;
        append_line(
            &mut lines,
            "2100",
            "Tax collected; jurisdiction classification pending",
            0,
            tax,
        )?;
    }

    let mut costs = db
        .prepare(
            "SELECT quantity,unit_cost_minor_snapshot FROM sale_items WHERE sale_id=?1 ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let cost_rows = costs
        .query_map([sale_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(costs);
    let mut cost_total = 0_i64;
    for (quantity, unit_cost) in cost_rows {
        match unit_cost {
            Some(cost) if cost >= 0 && quantity > 0 => {
                cost_total = cost_total
                    .checked_add(
                        cost.checked_mul(quantity)
                            .ok_or_else(|| "Sale cost is too large.".to_owned())?,
                    )
                    .ok_or_else(|| "Sale cost is too large.".to_owned())?;
            }
            _ => partial = true,
        }
    }
    if cost_total > 0 {
        append_line(&mut lines, "5000", "Cost of sold inventory", cost_total, 0)?;
        append_line(
            &mut lines,
            "1300",
            "Inventory relieved at recorded cost",
            0,
            cost_total,
        )?;
    }
    insert_journal(
        db,
        actor,
        "sale",
        sale_id,
        &effective_at,
        "Completed sale",
        if partial { "partial" } else { "complete" },
        &lines,
    )
}

fn post_refund(db: &Connection, actor: &Actor<'_>, refund_id: &str) -> Result<bool, String> {
    let refund: Option<(i64, String, String)> = db
        .query_row(
            "SELECT total_minor,created_at,sale_id FROM refunds WHERE id=?1 AND store_id=?2",
            params![refund_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (total, effective_at, sale_id) =
        refund.ok_or_else(|| "The refund for this accounting entry is missing.".to_owned())?;
    let mut items = db
        .prepare(
            "SELECT ri.subtotal_minor,ri.discount_minor,ri.tax_minor,ri.amount_minor,ri.quantity,si.unit_cost_minor_snapshot
             FROM refund_items ri JOIN sale_items si ON si.id=ri.sale_item_id WHERE ri.refund_id=?1 ORDER BY ri.id",
        )
        .map_err(|error| error.to_string())?;
    let item_rows = items
        .query_map([refund_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(items);
    let (subtotal, discount, tax, item_total, known_cost, partial) = item_rows.iter().try_fold(
        (0_i64, 0_i64, 0_i64, 0_i64, 0_i64, false),
        |(sub, disc, tax_sum, amount, cost_sum, partial),
         (line_sub, line_disc, line_tax, line_amount, quantity, unit_cost)| {
            let next_cost = match unit_cost {
                Some(cost) if *cost >= 0 && *quantity > 0 => cost_sum
                    .checked_add(
                        cost.checked_mul(*quantity)
                            .ok_or_else(|| "Refund cost is too large.".to_owned())?,
                    )
                    .ok_or_else(|| "Refund cost is too large.".to_owned())?,
                _ => cost_sum,
            };
            Ok::<_, String>((
                sub.checked_add(*line_sub)
                    .ok_or_else(|| "Refund subtotal is too large.".to_owned())?,
                disc.checked_add(*line_disc)
                    .ok_or_else(|| "Refund discount is too large.".to_owned())?,
                tax_sum
                    .checked_add(*line_tax)
                    .ok_or_else(|| "Refund tax is too large.".to_owned())?,
                amount
                    .checked_add(*line_amount)
                    .ok_or_else(|| "Refund total is too large.".to_owned())?,
                next_cost,
                partial || unit_cost.is_none(),
            ))
        },
    )?;
    if total <= 0
        || total != item_total
        || subtotal
            .checked_sub(discount)
            .and_then(|value| value.checked_add(tax))
            != Some(total)
    {
        return Err("Saved refund amounts do not reconcile for accounting.".to_owned());
    }
    let mut lines = Vec::new();
    if subtotal > 0 {
        append_line(
            &mut lines,
            "4020",
            format!("Returns against sale {sale_id}"),
            subtotal,
            0,
        )?;
    }
    if discount > 0 {
        append_line(&mut lines, "4010", "Reverse refunded discount", 0, discount)?;
    }
    if tax > 0 {
        append_line(
            &mut lines,
            "2100",
            "Reverse tax collected; classification pending",
            tax,
            0,
        )?;
    }
    let mut tenders = db
        .prepare("SELECT method,amount_minor FROM refund_tenders WHERE refund_id=?1 ORDER BY id")
        .map_err(|error| error.to_string())?;
    let tender_rows = tenders
        .query_map([refund_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tenders);
    let tender_total = tender_rows.iter().try_fold(0_i64, |sum, (_, amount)| {
        sum.checked_add(*amount)
            .ok_or_else(|| "Refund tender total is too large.".to_owned())
    })?;
    if tender_rows.is_empty()
        || tender_total != total
        || tender_rows.iter().any(|(_, amount)| *amount <= 0)
    {
        return Err("Saved refund tenders do not match the refund total.".to_owned());
    }
    let mut partial = partial;
    for (method, amount) in tender_rows {
        let (code, is_unclassified) = tender_account(&method);
        partial |= is_unclassified;
        append_line(
            &mut lines,
            code,
            format!("Refund tender: {method}"),
            0,
            amount,
        )?;
    }
    if known_cost > 0 {
        append_line(
            &mut lines,
            "1300",
            "Returned inventory at recorded cost",
            known_cost,
            0,
        )?;
        append_line(
            &mut lines,
            "5000",
            "Reverse cost of returned inventory",
            0,
            known_cost,
        )?;
    }
    insert_journal(
        db,
        actor,
        "refund",
        refund_id,
        &effective_at,
        "Recorded sale refund or void",
        if partial { "partial" } else { "complete" },
        &lines,
    )
}

fn post_expense(db: &Connection, actor: &Actor<'_>, expense_id: &str) -> Result<bool, String> {
    let expense: Option<(i64, String, String, String)> = db
        .query_row(
            "SELECT amount_minor,category,payment_method,occurred_at FROM expenses WHERE id=?1 AND store_id=?2",
            params![expense_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (amount, category, method, effective_at) =
        expense.ok_or_else(|| "The expense for this accounting entry is missing.".to_owned())?;
    if amount <= 0 {
        return Err("Saved expense amount is invalid for accounting.".to_owned());
    }
    let (tender_code, partial) = tender_account(&method);
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        "6000",
        format!("Operating expense: {category}"),
        amount,
        0,
    )?;
    append_line(
        &mut lines,
        tender_code,
        format!("Expense payment: {method}"),
        0,
        amount,
    )?;
    insert_journal(
        db,
        actor,
        "expense",
        expense_id,
        &effective_at,
        &format!("Recorded expense: {category}"),
        if partial { "partial" } else { "complete" },
        &lines,
    )
}

#[cfg(test)]
fn post_expense_reversal(
    db: &Connection,
    actor: &Actor<'_>,
    reversal_id: &str,
    original_journal_id: &str,
) -> Result<bool, String> {
    let source: Option<(i64, String, String, String, String)> = db
        .query_row(
            "SELECT r.amount_minor,e.category,r.payment_method,r.returned_at,r.expense_id
             FROM expense_reversals r JOIN expenses e ON e.id=r.expense_id AND e.store_id=r.store_id
             WHERE r.id=?1 AND r.store_id=?2 AND r.business_id=?3",
            params![reversal_id, actor.store_id, actor.business_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (amount, category, method, effective_at, _) = source
        .ok_or_else(|| "The expense reimbursement was not found in this store.".to_owned())?;
    if amount <= 0 || method == "cash" {
        return Err("Only valid non-cash expense reimbursements can be posted here.".to_owned());
    }
    let (tender_code, partial) = tender_account(&method);
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        &tender_code,
        format!("Returned expense payment: {method}"),
        amount,
        0,
    )?;
    append_line(
        &mut lines,
        "6000",
        format!("Reverse operating expense: {category}"),
        0,
        amount,
    )?;
    insert_journal_with_reversal(
        db,
        actor,
        "reversal",
        reversal_id,
        &effective_at,
        &format!("Reimbursed expense: {category}"),
        if partial { "partial" } else { "complete" },
        &lines,
        Some(original_journal_id),
    )
}

fn post_purchase(db: &Connection, actor: &Actor<'_>, purchase_id: &str) -> Result<bool, String> {
    let purchase: Option<(i64, String, Option<String>)> = db
        .query_row(
            "SELECT total_minor,received_at,reference FROM purchases WHERE id=?1 AND store_id=?2",
            params![purchase_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (total, effective_at, reference) = purchase
        .ok_or_else(|| "The purchase receipt for this accounting entry is missing.".to_owned())?;
    if total < 0 {
        return Err("Saved purchase total is invalid for accounting.".to_owned());
    }

    let mut items = db
        .prepare("SELECT quantity,unit_cost_minor,line_total_minor FROM purchase_items WHERE purchase_id=?1 ORDER BY id")
        .map_err(|error| error.to_string())?;
    let rows = items
        .query_map([purchase_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(items);
    let item_total = rows
        .iter()
        .try_fold(0_i64, |sum, (quantity, unit_cost, line_total)| {
            if *quantity <= 0
                || *unit_cost < 0
                || *line_total < 0
                || unit_cost.checked_mul(*quantity) != Some(*line_total)
            {
                return Err("A saved purchase line does not reconcile for accounting.".to_owned());
            }
            sum.checked_add(*line_total)
                .ok_or_else(|| "Purchase line total is too large.".to_owned())
        })?;
    if rows.is_empty() || item_total != total {
        return Err("Saved purchase lines do not match the receipt total.".to_owned());
    }
    // A zero-value receipt changes stock but has no monetary journal entry.
    if total == 0 {
        return Ok(false);
    }

    let mut lines = Vec::new();
    append_line(
        &mut lines,
        "1300",
        "Inventory received at recorded cost",
        total,
        0,
    )?;
    append_line(
        &mut lines,
        "2110",
        "Goods received; invoice and settlement not matched",
        0,
        total,
    )?;
    let description = reference
        .as_deref()
        .map(|value| format!("Received supplier delivery · {value}"))
        .unwrap_or_else(|| "Received supplier delivery".to_owned());
    insert_journal(
        db,
        actor,
        "purchase",
        purchase_id,
        &effective_at,
        &description,
        "complete",
        &lines,
    )
}

fn post_supplier_bill(db: &Connection, actor: &Actor<'_>, bill_id: &str) -> Result<bool, String> {
    let bill: Option<(i64, String, String)> = db
        .query_row(
            "SELECT total_minor,created_at,supplier_reference FROM supplier_bills WHERE id=?1 AND business_id=?2 AND store_id=?3",
            params![bill_id, actor.business_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (total, effective_at, reference) = bill
        .ok_or_else(|| "The supplier invoice for this accounting entry is missing.".to_owned())?;
    let allocated: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM supplier_bill_allocations WHERE bill_id=?1 AND business_id=?2 AND store_id=?3",
            params![bill_id, actor.business_id, actor.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if total <= 0 || allocated != total {
        return Err("Supplier invoice allocations do not match the invoice total.".to_owned());
    }
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        "2110",
        "Match received goods against supplier invoice",
        total,
        0,
    )?;
    append_line(
        &mut lines,
        "2000",
        format!("Accounts payable: {reference}"),
        0,
        total,
    )?;
    insert_journal(
        db,
        actor,
        "supplier_bill",
        bill_id,
        &effective_at,
        &format!("Matched supplier invoice {reference}"),
        "complete",
        &lines,
    )
}

fn post_supplier_credit(
    db: &Connection,
    actor: &Actor<'_>,
    credit_note_id: &str,
) -> Result<bool, String> {
    let credit: Option<(i64, String, String)> = db
        .query_row(
            "SELECT total_minor,effective_date,credit_reference FROM supplier_bill_credit_notes WHERE id=?1 AND business_id=?2 AND store_id=?3",
            params![credit_note_id, actor.business_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (total, effective_date, reference) = credit.ok_or_else(|| {
        "The supplier credit note for this accounting entry is missing.".to_owned()
    })?;
    let allocated: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM supplier_bill_credit_items WHERE credit_note_id=?1 AND business_id=?2 AND store_id=?3",
            params![credit_note_id, actor.business_id, actor.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if total <= 0 || allocated != total {
        return Err("Supplier return lines do not match the credit note total.".to_owned());
    }
    let effective_at = format!("{effective_date}T00:00:00Z");
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        "2000",
        format!("Accounts payable reduced by supplier credit {reference}"),
        total,
        0,
    )?;
    append_line(
        &mut lines,
        "1300",
        "Inventory returned to supplier",
        0,
        total,
    )?;
    insert_journal(
        db,
        actor,
        "supplier_credit",
        credit_note_id,
        &effective_at,
        &format!("Supplier goods return credit {reference}"),
        "complete",
        &lines,
    )
}

fn post_supplier_payment(
    db: &Connection,
    actor: &Actor<'_>,
    payment_id: &str,
) -> Result<bool, String> {
    let payment: Option<(i64, String, String, Option<String>)> = db
        .query_row(
            "SELECT amount_minor,payment_method,paid_at,external_reference FROM supplier_payments WHERE id=?1 AND business_id=?2 AND store_id=?3",
            params![payment_id, actor.business_id, actor.store_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (amount, method, effective_at, reference) = payment
        .ok_or_else(|| "The supplier payment for this accounting entry is missing.".to_owned())?;
    let allocated: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM supplier_payment_allocations WHERE payment_id=?1 AND business_id=?2 AND store_id=?3",
            params![payment_id, actor.business_id, actor.store_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if amount <= 0 || allocated != amount {
        return Err("Supplier payment allocations do not match the payment amount.".to_owned());
    }
    let (tender_code, partial) = tender_account(&method);
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        "2000",
        "Settle supplier accounts payable",
        amount,
        0,
    )?;
    append_line(
        &mut lines,
        tender_code,
        format!("Supplier payment: {method}"),
        0,
        amount,
    )?;
    let description = reference
        .as_deref()
        .map(|value| format!("Recorded supplier payment: {value}"))
        .unwrap_or_else(|| "Recorded supplier payment".to_owned());
    insert_journal(
        db,
        actor,
        "supplier_payment",
        payment_id,
        &effective_at,
        &description,
        if partial { "partial" } else { "complete" },
        &lines,
    )
}

pub(crate) fn post_sale_for_session(
    db: &Connection,
    session: &SessionUser,
    sale_id: &str,
) -> Result<bool, String> {
    post_sale(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        sale_id,
    )
}

pub(crate) fn post_refund_for_session(
    db: &Connection,
    session: &SessionUser,
    refund_id: &str,
) -> Result<bool, String> {
    post_refund(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        refund_id,
    )
}

pub(crate) fn post_expense_for_session(
    db: &Connection,
    session: &SessionUser,
    expense_id: &str,
) -> Result<bool, String> {
    post_expense(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        expense_id,
    )
}

pub(crate) fn post_expense_reimbursement_for_session(
    db: &Connection,
    session: &SessionUser,
    reimbursement_id: &str,
) -> Result<bool, String> {
    let (amount, category, method, effective_at): (i64, String, String, String) = db
        .query_row(
            "SELECT r.amount_minor,e.category,r.return_method,r.returned_at
             FROM expense_reimbursements r JOIN expenses e ON e.id=r.expense_id AND e.store_id=r.store_id
             WHERE r.id=?1 AND r.store_id=?2 AND r.business_id=?3",
            params![reimbursement_id, session.store_id, session.business_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The expense reimbursement was not found in this store.".to_owned())?;
    if amount <= 0 {
        return Err("The expense reimbursement amount is invalid.".to_owned());
    }
    let (tender_code, partial) = tender_account(&method);
    let mut lines = Vec::new();
    append_line(
        &mut lines,
        tender_code,
        format!("Expense reimbursement received: {method}"),
        amount,
        0,
    )?;
    append_line(
        &mut lines,
        "6000",
        format!("Reduce operating expense: {category}"),
        0,
        amount,
    )?;
    insert_journal(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        "reversal",
        reimbursement_id,
        &effective_at,
        &format!("Expense reimbursement: {category}"),
        if partial { "partial" } else { "complete" },
        &lines,
    )
}

pub(crate) fn post_purchase_for_session(
    db: &Connection,
    session: &SessionUser,
    purchase_id: &str,
) -> Result<bool, String> {
    post_purchase(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        purchase_id,
    )
}

pub(crate) fn post_supplier_bill_for_session(
    db: &Connection,
    session: &SessionUser,
    bill_id: &str,
) -> Result<bool, String> {
    post_supplier_bill(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        bill_id,
    )
}

pub(crate) fn post_supplier_credit_for_session(
    db: &Connection,
    session: &SessionUser,
    credit_note_id: &str,
) -> Result<bool, String> {
    post_supplier_credit(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        credit_note_id,
    )
}

pub(crate) fn post_supplier_payment_for_session(
    db: &Connection,
    session: &SessionUser,
    payment_id: &str,
) -> Result<bool, String> {
    post_supplier_payment(
        db,
        &Actor {
            business_id: &session.business_id,
            store_id: &session.store_id,
            user_id: Some(&session.user_id),
            currency_code: &session.currency_code,
        },
        payment_id,
    )
}

pub(crate) fn backfill_existing(db: &mut Connection) -> Result<(), String> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let sources = [
        ("sale", "SELECT s.id,s.store_id,s.cashier_id,b.id,b.currency_code FROM sales s JOIN stores st ON st.id=s.store_id JOIN businesses b ON b.id=st.business_id ORDER BY s.created_at,s.id"),
        ("refund", "SELECT r.id,r.store_id,r.cashier_id,b.id,b.currency_code FROM refunds r JOIN stores st ON st.id=r.store_id JOIN businesses b ON b.id=st.business_id ORDER BY r.created_at,r.id"),
        ("expense", "SELECT e.id,e.store_id,e.user_id,b.id,b.currency_code FROM expenses e JOIN stores st ON st.id=e.store_id JOIN businesses b ON b.id=st.business_id ORDER BY e.occurred_at,e.id"),
    ];
    for (source_type, query) in sources {
        let mut statement = tx.prepare(query).map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(statement);
        for (source_id, store_id, user_id, business_id, currency_code) in rows {
            let actor = Actor {
                business_id: &business_id,
                store_id: &store_id,
                user_id: Some(&user_id),
                currency_code: &currency_code,
            };
            match source_type {
                "sale" => {
                    post_sale(&tx, &actor, &source_id)?;
                }
                "refund" => {
                    post_refund(&tx, &actor, &source_id)?;
                }
                "expense" => {
                    post_expense(&tx, &actor, &source_id)?;
                }
                _ => unreachable!(),
            }
        }
    }
    tx.commit().map_err(|error| error.to_string())
}

pub fn get_trial_balance(
    input: TrialBalanceInput,
    state: tauri::State<'_, AppState>,
) -> Result<TrialBalance, String> {
    let session = active_session(&state, "reports:read")?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let (from, to, _) = accounting_period_utc_bounds(
        &tx,
        &session.business_id,
        &input.from_date,
        &input.to_date_inclusive,
    )?;
    ensure_accounts(&tx, &session.business_id)?;
    let result = trial_balance_for_period(&tx, &session, &from, &to)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn accounting_period_utc_bounds(
    connection: &Connection,
    business_id: &str,
    from_date: &str,
    to_date_inclusive: &str,
) -> Result<(String, String, String), String> {
    let from_date = NaiveDate::parse_from_str(from_date, "%Y-%m-%d")
        .map_err(|_| "Choose a valid accounting report start date.".to_owned())?;
    let to_date = NaiveDate::parse_from_str(to_date_inclusive, "%Y-%m-%d")
        .map_err(|_| "Choose a valid accounting report end date.".to_owned())?;
    if from_date > to_date {
        return Err("Choose a valid accounting report period.".to_owned());
    }
    let timezone: String = connection
        .query_row(
            "SELECT timezone FROM businesses WHERE id=?1",
            [business_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let zone: Tz = timezone
        .parse()
        .map_err(|_| "The business timezone is invalid.".to_owned())?;
    let to_next_date = to_date
        .checked_add_signed(Duration::days(1))
        .ok_or_else(|| {
            "The accounting report period is outside the supported date range.".to_owned()
        })?;
    let from = resolve_accounting_midnight(zone, from_date, true)?;
    let to = resolve_accounting_midnight(zone, to_next_date, false)?;
    if from >= to {
        return Err("Choose a valid accounting report period.".to_owned());
    }
    Ok((
        from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        timezone,
    ))
}

fn resolve_accounting_midnight(
    zone: Tz,
    date: NaiveDate,
    start: bool,
) -> Result<chrono::DateTime<Utc>, String> {
    for minute in 0..=24 * 60 {
        let local = NaiveDateTime::new(date, NaiveTime::MIN)
            .checked_add_signed(Duration::minutes(minute))
            .ok_or_else(|| {
                "The accounting report period is outside the supported date range.".to_owned()
            })?;
        match zone.from_local_datetime(&local) {
            LocalResult::Single(value) => return Ok(value.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                let first = first.with_timezone(&Utc);
                let second = second.with_timezone(&Utc);
                return Ok(
                    if (start && first <= second) || (!start && first >= second) {
                        first
                    } else {
                        second
                    },
                );
            }
            LocalResult::None => {}
        }
    }
    Err(
        "The business timezone has no valid local time at the accounting period boundary."
            .to_owned(),
    )
}

fn trial_balance_for_period(
    connection: &Connection,
    session: &SessionUser,
    from: &str,
    to: &str,
) -> Result<TrialBalance, String> {
    let mut statement = connection
        .prepare(
            "SELECT a.code,a.name,a.account_type,
                    COALESCE(SUM(l.debit_minor),0),COALESCE(SUM(l.credit_minor),0),COUNT(DISTINCT j.id)
             FROM accounting_accounts a
             LEFT JOIN accounting_journals j ON j.business_id=a.business_id
                  AND j.store_id=?2 AND j.effective_at>=?3 AND j.effective_at<?4
             LEFT JOIN accounting_journal_lines l ON l.journal_id=j.id AND l.business_id=j.business_id
                  AND l.account_id=a.id
             WHERE a.business_id=?1 AND a.is_active=1
             GROUP BY a.id ORDER BY a.code",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![session.business_id, session.store_id, from, to],
            |row| {
                let debit: i64 = row.get(3)?;
                let credit: i64 = row.get(4)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    debit,
                    credit,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut entries = Vec::with_capacity(rows.len());
    let mut total_debits = 0_i64;
    let mut total_credits = 0_i64;
    for (code, name, account_type, debit, credit, journal_count) in rows {
        total_debits = total_debits
            .checked_add(debit)
            .ok_or_else(|| "Trial balance debit total is too large.".to_owned())?;
        total_credits = total_credits
            .checked_add(credit)
            .ok_or_else(|| "Trial balance credit total is too large.".to_owned())?;
        entries.push(TrialBalanceRow {
            code,
            name,
            account_type,
            debit_minor: debit,
            credit_minor: credit,
            balance_minor: debit
                .checked_sub(credit)
                .ok_or_else(|| "Account balance is too large.".to_owned())?,
            journal_count,
        });
    }
    let count_in_period = |table: &str, date_column: &str| -> Result<i64, String> {
        let query = format!("SELECT COUNT(*) FROM {table} WHERE store_id=?1 AND {date_column}>=?2 AND {date_column}<?3");
        connection
            .query_row(&query, params![session.store_id, from, to], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())
    };
    let (sales_count, refunds_count, expenses_count) = (
        count_in_period("sales", "created_at")?,
        count_in_period("refunds", "created_at")?,
        count_in_period("expenses", "occurred_at")?,
    );
    // Zero-value receipts affect stock but have no financial value to post.
    let purchases_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM purchases WHERE store_id=?1 AND received_at>=?2 AND received_at<?3 AND total_minor>0",
            params![session.store_id, from, to],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let unposted_purchases_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM purchases p WHERE p.store_id=?1 AND p.received_at>=?2 AND p.received_at<?3
             AND p.total_minor>0
             AND NOT EXISTS (SELECT 1 FROM accounting_journals j WHERE j.business_id=?4 AND j.store_id=p.store_id AND j.source_type='purchase' AND j.source_id=p.id)",
            params![session.store_id, from, to, session.business_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let (
        supplier_bills_count,
        unposted_supplier_bills_count,
        supplier_payments_count,
        unposted_supplier_payments_count,
    ) = supplier_posting_coverage(
        connection,
        &session.business_id,
        &session.store_id,
        from,
        to,
    )?;
    let posted: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
               SUM(CASE WHEN source_type='sale' THEN 1 ELSE 0 END),
               SUM(CASE WHEN source_type='refund' THEN 1 ELSE 0 END),
               SUM(CASE WHEN source_type='expense' THEN 1 ELSE 0 END),
               SUM(CASE WHEN coverage_status='partial' THEN 1 ELSE 0 END)
             FROM accounting_journals WHERE business_id=?1 AND store_id=?2 AND effective_at>=?3 AND effective_at<?4",
            params![session.business_id, session.store_id, from, to],
            |row| Ok((row.get::<_, Option<i64>>(0)?.unwrap_or(0), row.get::<_, Option<i64>>(1)?.unwrap_or(0), row.get::<_, Option<i64>>(2)?.unwrap_or(0), row.get::<_, Option<i64>>(3)?.unwrap_or(0))),
        )
        .map_err(|error| error.to_string())?;
    Ok(TrialBalance {
        rows: entries,
        total_debits_minor: total_debits,
        total_credits_minor: total_credits,
        is_balanced: total_debits == total_credits,
        coverage: AccountingCoverage {
            sales_count,
            unposted_sales_count: sales_count.saturating_sub(posted.0),
            refunds_count,
            unposted_refunds_count: refunds_count.saturating_sub(posted.1),
            expenses_count,
            unposted_expenses_count: expenses_count.saturating_sub(posted.2),
            purchases_count,
            unposted_purchases_count,
            supplier_bills_count,
            unposted_supplier_bills_count,
            supplier_payments_count,
            unposted_supplier_payments_count,
            partial_journal_count: posted.3,
        },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountingCloseSnapshot {
    format_version: u8,
    business_id: String,
    store_id: String,
    from_utc: String,
    to_utc_exclusive: String,
    timezone: String,
    currency_code: String,
    trial_balance: TrialBalance,
    cash_close_reports: Vec<serde_json::Value>,
    closed_by: String,
    closed_at: String,
}

fn accounting_period_closure_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AccountingPeriodClosure> {
    Ok(AccountingPeriodClosure {
        id: row.get(0)?,
        business_id: row.get(1)?,
        store_id: row.get(2)?,
        from_utc: row.get(3)?,
        to_utc_exclusive: row.get(4)?,
        timezone: row.get(5)?,
        currency_code: row.get(6)?,
        snapshot_json: row.get(7)?,
        snapshot_sha256: row.get(8)?,
        closed_by: row.get(9)?,
        closed_at: row.get(10)?,
    })
}

#[tauri::command]
pub fn close_accounting_period(
    input: AccountingPeriodCloseInput,
    state: tauri::State<'_, AppState>,
) -> Result<AccountingPeriodClosure, String> {
    let session = active_session(&state, "accounting:close")?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    close_accounting_period_for_session(
        &mut db,
        &session,
        &input.from_date,
        &input.to_date_inclusive,
    )
}

fn close_accounting_period_for_session(
    db: &mut Connection,
    session: &SessionUser,
    from_date: &str,
    to_date_inclusive: &str,
) -> Result<AccountingPeriodClosure, String> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let (from, to, timezone) =
        accounting_period_utc_bounds(&tx, &session.business_id, from_date, to_date_inclusive)?;

    if let Some(existing) = tx
        .query_row(
            "SELECT id,business_id,store_id,from_utc,to_utc_exclusive,timezone,currency_code,snapshot_json,snapshot_sha256,closed_by,closed_at
             FROM accounting_period_closures WHERE business_id=?1 AND store_id=?2 AND from_utc=?3 AND to_utc_exclusive=?4",
            params![session.business_id, session.store_id, from, to],
            accounting_period_closure_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?
    {
        return Ok(existing);
    }
    let overlaps: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM accounting_period_closures WHERE business_id=?1 AND store_id=?2 AND ?3<to_utc_exclusive AND ?4>from_utc)",
            params![session.business_id, session.store_id, from, to],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if overlaps {
        return Err("This range overlaps an accounting period that is already closed.".to_owned());
    }

    ensure_accounts(&tx, &session.business_id)?;
    let trial_balance = trial_balance_for_period(&tx, &session, &from, &to)?;
    let mut blockers = Vec::new();
    if !trial_balance.is_balanced {
        blockers.push("the trial balance is not balanced");
    }
    let coverage = &trial_balance.coverage;
    if coverage.unposted_sales_count > 0
        || coverage.unposted_refunds_count > 0
        || coverage.unposted_expenses_count > 0
        || coverage.unposted_purchases_count > 0
        || coverage.unposted_supplier_bills_count > 0
        || coverage.unposted_supplier_payments_count > 0
    {
        blockers.push("one or more financial sources are not posted to the journal");
    }
    if coverage.partial_journal_count > 0 {
        blockers.push("one or more journal entries have partial coverage");
    }
    let open_drawers: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM cash_shifts WHERE store_id=?1 AND status='open' AND opened_at<?3 AND (closed_at IS NULL OR closed_at>?2)",
            params![session.store_id, from, to],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if open_drawers > 0 {
        blockers.push("one or more cash drawer shifts are still open");
    }
    if !blockers.is_empty() {
        return Err(format!(
            "This accounting period cannot close yet: {}.",
            blockers.join("; ")
        ));
    }

    let mut report_query = tx
        .prepare(
            "SELECT r.id,r.shift_id,r.created_at,r.expected_cash_minor,r.counted_cash_minor,r.difference_minor,r.report_json
             FROM cash_close_reports r JOIN cash_shifts s ON s.id=r.shift_id
             WHERE r.store_id=?1 AND s.opened_at<?3 AND COALESCE(s.closed_at,r.created_at)>?2
             ORDER BY r.created_at,r.id",
        )
        .map_err(|error| error.to_string())?;
    let cash_close_reports = report_query
        .query_map(params![session.store_id, from, to], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(
            |(id, shift_id, closed_at, expected, counted, difference, json)| {
                let report: serde_json::Value = serde_json::from_str(&json).map_err(|_| {
                    "A saved cash drawer close report is invalid; accounting close was stopped."
                        .to_owned()
                })?;
                Ok(serde_json::json!({
                    "id": id,
                    "shiftId": shift_id,
                    "closedAt": closed_at,
                    "expectedCashMinor": expected,
                    "countedCashMinor": counted,
                    "differenceMinor": difference,
                    "report": report,
                }))
            },
        )
        .collect::<Result<Vec<_>, String>>()?;
    drop(report_query);

    let closed_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let snapshot = AccountingCloseSnapshot {
        format_version: 1,
        business_id: session.business_id.clone(),
        store_id: session.store_id.clone(),
        from_utc: from.to_owned(),
        to_utc_exclusive: to.to_owned(),
        timezone: timezone.clone(),
        currency_code: session.currency_code.clone(),
        trial_balance,
        cash_close_reports,
        closed_by: session.user_id.clone(),
        closed_at: closed_at.clone(),
    };
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|error| error.to_string())?;
    let snapshot_sha256 = format!("{:x}", Sha256::digest(snapshot_json.as_bytes()));
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO accounting_period_closures(id,business_id,store_id,from_utc,to_utc_exclusive,timezone,currency_code,snapshot_json,snapshot_sha256,closed_by,closed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![id, session.business_id, session.store_id, from, to, timezone, session.currency_code, snapshot_json, snapshot_sha256, session.user_id, closed_at],
    )
    .map_err(|error| {
        if error.to_string().contains("overlap") {
            "This range overlaps an accounting period that is already closed.".to_owned()
        } else {
            error.to_string()
        }
    })?;
    add_audit_event(
        &tx,
        &session,
        "accounting_period_closed",
        "accounting_period",
        Some(&id),
        serde_json::json!({"fromUtc":from,"toUtcExclusive":to,"snapshotSha256":snapshot_sha256}),
    )?;
    let closure = AccountingPeriodClosure {
        id,
        business_id: session.business_id.clone(),
        store_id: session.store_id.clone(),
        from_utc: from.to_owned(),
        to_utc_exclusive: to.to_owned(),
        timezone,
        currency_code: session.currency_code.clone(),
        snapshot_json,
        snapshot_sha256,
        closed_by: session.user_id.clone(),
        closed_at,
    };
    tx.commit().map_err(|error| error.to_string())?;
    Ok(closure)
}

#[tauri::command]
pub fn list_accounting_period_closures(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AccountingPeriodClosure>, String> {
    let session = active_session(&state, "reports:read")?;
    let db = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let mut statement = db
        .prepare(
            "SELECT id,business_id,store_id,from_utc,to_utc_exclusive,timezone,currency_code,snapshot_json,snapshot_sha256,closed_by,closed_at
             FROM accounting_period_closures WHERE business_id=?1 AND store_id=?2 ORDER BY from_utc DESC LIMIT 100",
        )
        .map_err(|error| error.to_string())?;
    let closures = statement
        .query_map(
            params![session.business_id, session.store_id],
            accounting_period_closure_from_row,
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(closures)
}

#[tauri::command]
pub fn query_trial_balance(
    input: TrialBalanceInput,
    state: tauri::State<'_, AppState>,
) -> Result<TrialBalance, String> {
    get_trial_balance(input, state)
}

fn supplier_posting_coverage(
    connection: &Connection,
    business_id: &str,
    store_id: &str,
    from: &str,
    to: &str,
) -> Result<(i64, i64, i64, i64), String> {
    let (bill_count, payment_count): (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM supplier_bills WHERE store_id=?1 AND created_at>=?2 AND created_at<?3),
                (SELECT COUNT(*) FROM supplier_payments WHERE store_id=?1 AND paid_at>=?2 AND paid_at<?3)",
            params![store_id, from, to],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let (posted_bills, posted_payments): (i64, i64) = connection
        .query_row(
            "SELECT
                SUM(CASE WHEN source_type='supplier_bill' THEN 1 ELSE 0 END),
                SUM(CASE WHEN source_type='supplier_payment' THEN 1 ELSE 0 END)
             FROM accounting_journals
             WHERE business_id=?1 AND store_id=?2 AND effective_at>=?3 AND effective_at<?4",
            params![business_id, store_id, from, to],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    Ok((
        bill_count,
        bill_count.saturating_sub(posted_bills),
        payment_count,
        payment_count.saturating_sub(posted_payments),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_lines_must_balance_and_have_one_sided_amounts() {
        let mut lines = Vec::new();
        append_line(&mut lines, "1000", "cash", 1250, 0).unwrap();
        append_line(&mut lines, "4000", "sales", 0, 1250).unwrap();
        assert!(validate_balance(&lines).is_ok());
        lines[1].credit_minor = 1249;
        assert!(validate_balance(&lines).is_err());
        assert!(append_line(&mut lines, "1000", "bad", 10, 10).is_err());
    }

    fn accounting_test_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE businesses(id TEXT PRIMARY KEY,name TEXT,currency_code TEXT,timezone TEXT); INSERT INTO businesses VALUES('business-a','Test','GHS','Africa/Accra');
            CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT); INSERT INTO stores VALUES('store-a','business-a');
            CREATE TABLE users(id TEXT PRIMARY KEY); INSERT INTO users VALUES('user-a');
            CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
            CREATE TABLE roles(role_key TEXT,permissions_json TEXT); INSERT INTO roles VALUES('owner','[]'),('admin','[]');
            CREATE TABLE audit_events(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,user_id TEXT,action TEXT,entity_type TEXT,entity_id TEXT,details_json TEXT,created_at TEXT);
            CREATE TABLE sales(id TEXT PRIMARY KEY,store_id TEXT,subtotal_minor INTEGER,discount_minor INTEGER,tax_minor INTEGER,total_minor INTEGER,created_at TEXT);
            CREATE TABLE refunds(id TEXT PRIMARY KEY,store_id TEXT,created_at TEXT);
            CREATE TABLE payments(id TEXT PRIMARY KEY,sale_id TEXT,method TEXT,amount_minor INTEGER);
            CREATE TABLE sale_items(id TEXT PRIMARY KEY,sale_id TEXT,quantity INTEGER,unit_cost_minor_snapshot INTEGER);
            CREATE TABLE purchases(id TEXT PRIMARY KEY,store_id TEXT,total_minor INTEGER,received_at TEXT,reference TEXT);
            CREATE TABLE purchase_items(id TEXT PRIMARY KEY,purchase_id TEXT,quantity INTEGER,unit_cost_minor INTEGER,line_total_minor INTEGER);
            CREATE TABLE expenses(id TEXT PRIMARY KEY,store_id TEXT,user_id TEXT,category TEXT,description TEXT,amount_minor INTEGER,payment_method TEXT,occurred_at TEXT,created_at TEXT);
            CREATE TABLE supplier_bills(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,created_at TEXT,invoice_date TEXT);
            CREATE TABLE supplier_payments(id TEXT PRIMARY KEY,store_id TEXT,paid_at TEXT);
            CREATE TABLE cash_shifts(id TEXT PRIMARY KEY,store_id TEXT,status TEXT,opened_at TEXT,closed_at TEXT);
            CREATE TABLE cash_close_reports(id TEXT PRIMARY KEY,shift_id TEXT,store_id TEXT,created_at TEXT,expected_cash_minor INTEGER,counted_cash_minor INTEGER,difference_minor INTEGER,report_json TEXT);")
            .unwrap();
        db.execute_batch(include_str!("../migrations/0020_accounting_journals.sql"))
            .unwrap();
        db.execute_batch(include_str!("../migrations/0021_accounting_purchases.sql"))
            .unwrap();
        db.execute_batch(include_str!(
            "../migrations/0039_accounting_period_closures.sql"
        ))
        .unwrap();
        db
    }

    fn close_test_session() -> SessionUser {
        SessionUser {
            user_id: "user-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Owner".into(),
            username: "owner".into(),
            role_key: "owner".into(),
            role_name: "Owner".into(),
            workspace_mode: "management".into(),
            is_owner: true,
            permissions: vec!["accounting:close".into(), "reports:read".into()],
            business_name: "Test".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        }
    }

    #[test]
    fn accounting_period_bounds_use_store_timezone_across_dst_transitions() {
        let db = accounting_test_db();
        db.execute(
            "UPDATE businesses SET timezone='America/New_York' WHERE id='business-a'",
            [],
        )
        .unwrap();
        let spring =
            accounting_period_utc_bounds(&db, "business-a", "2026-03-08", "2026-03-08").unwrap();
        assert_eq!(spring.0, "2026-03-08T05:00:00.000Z");
        assert_eq!(spring.1, "2026-03-09T04:00:00.000Z");
        let autumn =
            accounting_period_utc_bounds(&db, "business-a", "2026-11-01", "2026-11-01").unwrap();
        assert_eq!(autumn.0, "2026-11-01T04:00:00.000Z");
        assert_eq!(autumn.1, "2026-11-02T05:00:00.000Z");
    }

    #[test]
    fn accounting_period_close_is_idempotent_immutable_and_blocks_backdated_writes() {
        let mut db = accounting_test_db();
        let session = close_test_session();
        let from = "2026-10-01";
        let to = "2026-10-31";
        let first = close_accounting_period_for_session(&mut db, &session, from, to).unwrap();
        let retry = close_accounting_period_for_session(&mut db, &session, from, to).unwrap();
        assert_eq!(first.id, retry.id);
        assert_eq!(first.snapshot_json, retry.snapshot_json);
        assert_eq!(first.snapshot_sha256.len(), 64);

        assert!(db
            .execute(
                "UPDATE accounting_period_closures SET snapshot_json='{}' WHERE id=?1",
                [&first.id],
            )
            .is_err());
        assert!(db
            .execute(
                "DELETE FROM accounting_period_closures WHERE id=?1",
                [&first.id],
            )
            .is_err());

        assert!(db.execute(
            "INSERT INTO accounting_journals(id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,description,coverage_status)
             VALUES('late-journal','business-a','store-a','adjustment','late-journal','GHS','2026-10-31T12:00:00.000Z','2026-11-02T00:00:00.000Z','Backdated adjustment','complete')",
            [],
        ).is_err());
        assert!(db.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at)
             VALUES('late-expense','store-a','user-a','Utilities','Backdated entry',100,'bank_transfer','2026-10-31T12:00:00.000Z','2026-11-02T00:00:00.000Z')",
            [],
        ).is_err());
        assert!(db.execute(
            "INSERT INTO accounting_journals(id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,description,coverage_status)
             VALUES('next-journal','business-a','store-a','adjustment','next-journal','GHS','2026-11-01T00:00:00.000Z','2026-11-02T00:00:00.000Z','Next period','complete')",
            [],
        ).is_ok());
        let saved_snapshot: String = db
            .query_row(
                "SELECT snapshot_json FROM accounting_period_closures WHERE id=?1",
                [&first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved_snapshot, first.snapshot_json);
    }

    #[test]
    fn accounting_period_close_blocks_incomplete_coverage_and_open_drawers() {
        let session = close_test_session();
        let from = "2026-10-01";
        let to = "2026-10-31";
        let mut unposted = accounting_test_db();
        unposted.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at)
             VALUES('unposted-expense','store-a','user-a','Utilities','Unposted',100,'bank_transfer','2026-10-15T12:00:00.000Z','2026-10-15T12:00:00.000Z')",
            [],
        ).unwrap();
        let error =
            close_accounting_period_for_session(&mut unposted, &session, from, to).unwrap_err();
        assert!(error.contains("not posted"));

        let mut open_drawer = accounting_test_db();
        open_drawer.execute(
            "INSERT INTO cash_shifts VALUES('open-shift','store-a','open','2026-10-15T12:00:00.000Z',NULL)",
            [],
        ).unwrap();
        let error =
            close_accounting_period_for_session(&mut open_drawer, &session, from, to).unwrap_err();
        assert!(error.contains("cash drawer shifts are still open"));
        let closes: i64 = open_drawer
            .query_row(
                "SELECT COUNT(*) FROM accounting_period_closures",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(closes, 0);
    }

    #[test]
    fn accounting_period_close_prevents_moving_sources_into_or_out_of_closed_period() {
        let mut db = accounting_test_db();
        let session = close_test_session();
        db.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at)
             VALUES('closed-expense','store-a','user-a','Utilities','Posted before close',100,'bank_transfer','2026-10-15T12:00:00.000Z','2026-10-15T12:00:00.000Z')",
            [],
        )
        .unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        post_expense(&db, &actor, "closed-expense").unwrap();
        close_accounting_period_for_session(&mut db, &session, "2026-10-01", "2026-10-31").unwrap();

        assert!(db
            .execute(
                "UPDATE expenses SET occurred_at='2026-11-01T12:00:00.000Z' WHERE id='closed-expense'",
                [],
            )
            .is_err());
        db.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at)
             VALUES('next-expense','store-a','user-a','Utilities','Recorded after close',100,'bank_transfer','2026-11-01T12:00:00.000Z','2026-11-01T12:00:00.000Z')",
            [],
        )
        .unwrap();
        assert!(db
            .execute(
                "UPDATE expenses SET occurred_at='2026-10-31T12:00:00.000Z' WHERE id='next-expense'",
                [],
            )
            .is_err());
    }

    #[test]
    fn non_cash_expense_reversal_posts_a_balanced_contra_journal_and_is_immutable() {
        let db = accounting_test_db();
        db.execute_batch(include_str!("../migrations/0029_expense_reversals.sql"))
            .unwrap();
        db.execute(
            "INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at)
             VALUES('expense-a','store-a','user-a','Utilities','Duplicate invoice',500,'bank_transfer','2026-10-30T09:00:00.000Z','2026-10-30T09:00:00.000Z')",
            [],
        ).unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        post_expense(&db, &actor, "expense-a").unwrap();
        let original_journal_id: String = db.query_row(
            "SELECT id FROM accounting_journals WHERE source_type='expense' AND source_id='expense-a'",
            [],
            |row| row.get(0),
        ).unwrap();
        db.execute(
            "INSERT INTO expense_reversals(id,business_id,store_id,expense_id,amount_minor,payment_method,return_reference,reason,returned_at,idempotency_key,recorded_by,created_at)
             VALUES('return-a','business-a','store-a','expense-a',500,'bank_transfer','BANK-REF-500','Duplicate payment returned','2026-10-31T10:00:00.000Z','retry-a','user-a','2026-10-31T10:00:00.000Z')",
            [],
        ).unwrap();
        assert!(post_expense_reversal(&db, &actor, "return-a", &original_journal_id).unwrap());
        assert!(!post_expense_reversal(&db, &actor, "return-a", &original_journal_id).unwrap());
        let (reversal_of, debits, credits, expense_debit, expense_credit): (String, i64, i64, i64, i64) = db.query_row(
            "SELECT j.reversal_of,SUM(l.debit_minor),SUM(l.credit_minor),
                    SUM(CASE WHEN a.code='6000' THEN l.debit_minor ELSE 0 END),
                    SUM(CASE WHEN a.code='6000' THEN l.credit_minor ELSE 0 END)
             FROM accounting_journals j JOIN accounting_journal_lines l ON l.journal_id=j.id
             JOIN accounting_accounts a ON a.id=l.account_id WHERE j.source_type='reversal' AND j.source_id='return-a'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).unwrap();
        assert_eq!(reversal_of, original_journal_id);
        assert_eq!(
            (debits, credits, expense_debit, expense_credit),
            (500, 500, 0, 500)
        );
        assert!(db
            .execute(
                "UPDATE expense_reversals SET reason='rewritten' WHERE id='return-a'",
                []
            )
            .is_err());
        assert!(db
            .execute("DELETE FROM expense_reversals WHERE id='return-a'", [])
            .is_err());
        assert!(db.execute(
            "INSERT INTO expense_reversals(id,business_id,store_id,expense_id,amount_minor,payment_method,return_reference,reason,returned_at,idempotency_key,recorded_by,created_at)
             VALUES('return-cash','business-a','store-a','expense-a',500,'cash','CASH-REF','Invalid cash correction','2026-10-31T10:00:00.000Z','retry-cash','user-a','2026-10-31T10:00:00.000Z')",
            [],
        ).is_err());
    }

    #[test]
    fn partial_reimbursement_posts_actual_tender_once_and_reduces_expense() {
        let db = accounting_test_db();
        db.execute_batch(
            "CREATE TABLE expense_reimbursements(id TEXT PRIMARY KEY,business_id TEXT,store_id TEXT,expense_id TEXT,amount_minor INTEGER,return_method TEXT,returned_at TEXT);
             INSERT INTO expenses(id,store_id,user_id,category,description,amount_minor,payment_method,occurred_at,created_at)
             VALUES('expense-partial','store-a','user-a','Utilities','Duplicate payment',1000,'bank_transfer','2026-10-30T09:00:00.000Z','2026-10-30T09:00:00.000Z');
             INSERT INTO expense_reimbursements VALUES('return-partial','business-a','store-a','expense-partial',400,'cash','2026-10-31T09:00:00.000Z');",
        )
        .unwrap();
        let session = SessionUser {
            user_id: "user-a".into(),
            business_id: "business-a".into(),
            store_id: "store-a".into(),
            full_name: "Owner One".into(),
            username: "owner".into(),
            role_key: "owner".into(),
            role_name: "Owner".into(),
            workspace_mode: "management".into(),
            is_owner: true,
            permissions: vec!["expenses:write".into()],
            business_name: "Test".into(),
            store_name: "Store".into(),
            currency_code: "GHS".into(),
        };
        post_expense_for_session(&db, &session, "expense-partial").unwrap();
        assert!(post_expense_reimbursement_for_session(&db, &session, "return-partial").unwrap());
        assert!(!post_expense_reimbursement_for_session(&db, &session, "return-partial").unwrap());

        let totals: (i64, i64, i64, i64) = db
            .query_row(
                "SELECT SUM(l.debit_minor),SUM(l.credit_minor),
                        SUM(CASE WHEN a.code='1000' THEN l.debit_minor ELSE 0 END),
                        SUM(CASE WHEN a.code='6000' THEN l.credit_minor ELSE 0 END)
                 FROM accounting_journals j JOIN accounting_journal_lines l ON l.journal_id=j.id
                 JOIN accounting_accounts a ON a.id=l.account_id
                 WHERE j.source_type='reversal' AND j.source_id='return-partial'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(totals, (400, 400, 400, 400));
    }

    #[test]
    fn cash_opening_posts_one_balanced_owner_cutover_journal() {
        let db = accounting_test_db();
        db.execute_batch(include_str!("../migrations/0027_cash_opening_balance.sql"))
            .unwrap();
        db.execute_batch("ALTER TABLE users ADD COLUMN business_id TEXT; UPDATE users SET business_id='business-a';")
            .unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };

        let journal_id = post_cash_opening_journal(
            &db,
            &actor,
            "cash-opening-opening-a",
            125_500,
            "2026-10-30T00:00:00.000Z",
        )
        .unwrap();
        let (debits, credits, cash, equity): (i64, i64, i64, i64) = db
            .query_row(
                "SELECT SUM(l.debit_minor),SUM(l.credit_minor),
                    SUM(CASE WHEN a.code='1000' THEN l.debit_minor ELSE 0 END),
                    SUM(CASE WHEN a.code='3000' THEN l.credit_minor ELSE 0 END)
                 FROM accounting_journal_lines l
                 JOIN accounting_journals j ON j.id=l.journal_id
                 JOIN accounting_accounts a ON a.id=l.account_id
                 WHERE j.id=?1",
                [&journal_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (debits, credits, cash, equity),
            (125_500, 125_500, 125_500, 125_500)
        );
        assert!(db
            .execute(
                "INSERT INTO cash_opening_balances(id,business_id,store_id,currency_code,amount_minor,effective_at,reason,idempotency_key,journal_id,recorded_by,recorded_at)
                 VALUES('opening-a','business-a','store-a','GHS',125500,'2026-10-30T00:00:00.000Z','Cutover count','retry-a',?1,'user-a','2026-10-30T00:00:00.000Z')",
                [&journal_id],
            )
            .is_ok());
        let replay = replay_cash_opening_balance(
            &db,
            "business-a",
            "store-a",
            "retry-a",
            125_500,
            "2026-10-30T00:00:00.000Z",
            "Cutover count",
        )
        .unwrap()
        .unwrap();
        assert_eq!(replay.amount_minor, 125_500);
        assert!(replay_cash_opening_balance(
            &db,
            "business-a",
            "store-a",
            "retry-a",
            125_501,
            "2026-10-30T00:00:00.000Z",
            "Cutover count",
        )
        .is_err());
        assert!(db
            .execute(
                "UPDATE cash_opening_balances SET amount_minor=1 WHERE id='opening-a'",
                [],
            )
            .is_err());
        assert!(db
            .execute("DELETE FROM cash_opening_balances WHERE id='opening-a'", [])
            .is_err());
        assert!(db
            .execute(
                "INSERT INTO cash_opening_balances(id,business_id,store_id,currency_code,amount_minor,effective_at,reason,idempotency_key,journal_id,recorded_by,recorded_at)
                 VALUES('opening-b','business-a','store-a','GHS',125500,'2026-10-30T00:00:00.000Z','Second count','retry-b','journal-b','user-a','2026-10-30T00:00:00.000Z')",
                [],
            )
            .is_err());
    }

    #[test]
    fn cash_opening_cutover_rejects_prior_cash_journals_that_would_be_double_counted() {
        let db = accounting_test_db();
        ensure_accounts(&db, "business-a").unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        let mut lines = Vec::new();
        append_line(&mut lines, "1000", "Prior cash tender", 500, 0).unwrap();
        append_line(&mut lines, "4000", "Prior sale", 0, 500).unwrap();
        insert_journal(
            &db,
            &actor,
            "sale",
            "prior-sale",
            "2026-10-30T09:00:00.000Z",
            "Prior cash activity",
            "complete",
            &lines,
        )
        .unwrap();

        assert!(ensure_cash_cutover_has_no_prior_activity(
            &db,
            "business-a",
            "store-a",
            "2026-10-30T10:00:00.000Z",
        )
        .unwrap_err()
        .contains("already exists"));
        assert!(ensure_cash_cutover_has_no_prior_activity(
            &db,
            "business-a",
            "store-a",
            "2026-10-30T08:00:00.000Z",
        )
        .is_ok());
    }

    #[test]
    fn accounting_upgrade_preserves_journals_and_enforces_new_source_and_reversal_rules() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch("CREATE TABLE businesses(id TEXT PRIMARY KEY,name TEXT,currency_code TEXT); INSERT INTO businesses VALUES('business-a','Test','GHS');
            CREATE TABLE stores(id TEXT PRIMARY KEY,business_id TEXT); INSERT INTO stores VALUES('store-a','business-a');
            CREATE TABLE users(id TEXT PRIMARY KEY); INSERT INTO users VALUES('user-a');")
            .unwrap();
        db.execute_batch(include_str!("../migrations/0020_accounting_journals.sql"))
            .unwrap();
        ensure_accounts(&db, "business-a").unwrap();
        db.execute("INSERT INTO accounting_journals VALUES('journal-a','business-a','store-a','sale','sale-a','GHS','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z','user-a','Historical sale','complete',NULL)", []).unwrap();
        db.execute("INSERT INTO accounting_journal_lines VALUES('line-a','business-a','journal-a',(SELECT id FROM accounting_accounts WHERE business_id='business-a' AND code='1000'),1,'Cash tender',100,0)", []).unwrap();
        db.execute("INSERT INTO accounting_journal_lines VALUES('line-b','business-a','journal-a',(SELECT id FROM accounting_accounts WHERE business_id='business-a' AND code='4000'),2,'Sales',0,100)", []).unwrap();

        db.execute_batch(include_str!("../migrations/0021_accounting_purchases.sql"))
            .unwrap();
        db.execute_batch(include_str!("../migrations/0021_accounting_purchases.sql"))
            .unwrap();

        let preserved: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM accounting_journal_lines WHERE journal_id='journal-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let fk_errors: i64 = db
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(preserved, 2);
        assert_eq!(fk_errors, 0);
        assert!(db.execute("INSERT INTO accounting_journals VALUES('purchase-journal','business-a','store-a','purchase','purchase-a','GHS','2026-10-02T00:00:00Z','2026-10-02T00:00:00Z','user-a','Receipt','partial',NULL)", []).is_ok());
        assert!(db.execute("INSERT INTO accounting_journals VALUES('reversal-a','business-a','store-a','reversal','journal-a','GHS','2026-10-02T00:00:00Z','2026-10-02T00:00:00Z','user-a','Reverse sale','complete','journal-a')", []).is_ok());
        assert!(db.execute("INSERT INTO accounting_journals VALUES('reversal-b','business-a','store-a','reversal','journal-a-2','GHS','2026-10-03T00:00:00Z','2026-10-03T00:00:00Z','user-a','Duplicate reversal','complete','journal-a')", []).is_err());
    }

    #[test]
    fn supplier_billing_migration_preserves_journals_and_adds_owner_permissions() {
        let db = accounting_test_db();
        db.execute_batch(
            "CREATE TABLE suppliers(id TEXT PRIMARY KEY,store_id TEXT NOT NULL);
             DROP TABLE supplier_bills;
             DROP TABLE supplier_payments;
             DELETE FROM roles;
             INSERT INTO suppliers VALUES('supplier-a','store-a');
             INSERT INTO roles VALUES('owner','[\"purchases:read\"]');
             INSERT INTO roles VALUES('admin','[\"purchases:read\"]');
             INSERT INTO roles VALUES('manager','[\"purchases:read\"]');",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0022_supplier_billing.sql"))
            .unwrap();

        let source_types: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='accounting_journals'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(source_types.contains("supplier_payment"));
        let fk_errors: i64 = db
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fk_errors, 0);

        let owner_permissions: String = db
            .query_row(
                "SELECT permissions_json FROM roles WHERE role_key='owner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let manager_permissions: String = db
            .query_row(
                "SELECT permissions_json FROM roles WHERE role_key='manager'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(owner_permissions.contains("purchases:invoice"));
        assert!(owner_permissions.contains("suppliers:settle"));
        assert!(!manager_permissions.contains("purchases:invoice"));
        assert!(!manager_permissions.contains("suppliers:settle"));
        let migration_version: i64 = db
            .query_row(
                "SELECT version FROM local_schema_migrations WHERE version=22",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_version, 22);
    }

    #[test]
    fn supplier_coverage_counts_missing_postings_in_the_selected_period_and_store() {
        let db = accounting_test_db();
        db.execute_batch(
            "CREATE TABLE suppliers(id TEXT PRIMARY KEY,store_id TEXT NOT NULL);
             DROP TABLE supplier_bills;
             DROP TABLE supplier_payments;
             DELETE FROM roles;
             INSERT INTO suppliers VALUES('supplier-a','store-a');
             INSERT INTO roles VALUES('owner','[]'),('admin','[]'),('manager','[]');",
        )
        .unwrap();
        db.execute_batch(include_str!("../migrations/0022_supplier_billing.sql"))
            .unwrap();
        db.execute("INSERT INTO supplier_bills(id,business_id,store_id,supplier_id,supplier_reference,reference_key,invoice_date,due_date,currency_code,total_minor,idempotency_key,created_by,created_at) VALUES('bill-posted','business-a','store-a','supplier-a','INV-1','inv-1','2026-10-29',NULL,'GHS',100,'key-bill-1','user-a','2026-10-29T10:00:00Z'),('bill-unposted','business-a','store-a','supplier-a','INV-2','inv-2','2026-10-29',NULL,'GHS',200,'key-bill-2','user-a','2026-10-29T11:00:00Z')", []).unwrap();
        db.execute("INSERT INTO supplier_payments(id,business_id,store_id,supplier_id,currency_code,payment_method,external_reference,reference_key,amount_minor,idempotency_key,recorded_by,paid_at,created_at) VALUES('payment-posted','business-a','store-a','supplier-a','GHS','cash',NULL,NULL,100,'key-payment-1','user-a','2026-10-29T12:00:00Z','2026-10-29T12:00:00Z')", []).unwrap();
        for (id, source_type, source_id, effective_at) in [
            (
                "journal-bill",
                "supplier_bill",
                "bill-posted",
                "2026-10-29T10:00:00Z",
            ),
            (
                "journal-payment",
                "supplier_payment",
                "payment-posted",
                "2026-10-29T12:00:00Z",
            ),
        ] {
            db.execute(
                "INSERT INTO accounting_journals(id,business_id,store_id,source_type,source_id,currency_code,effective_at,recorded_at,created_by,description,coverage_status) VALUES(?1,'business-a','store-a',?2,?3,'GHS',?4,?4,'user-a','Coverage fixture','complete')",
                params![id, source_type, source_id, effective_at],
            )
            .unwrap();
        }

        assert_eq!(
            supplier_posting_coverage(
                &db,
                "business-a",
                "store-a",
                "2026-10-29T00:00:00Z",
                "2026-10-30T00:00:00Z",
            )
            .unwrap(),
            (2, 1, 1, 0)
        );
        assert_eq!(
            supplier_posting_coverage(
                &db,
                "business-a",
                "store-a",
                "2026-10-30T00:00:00Z",
                "2026-10-31T00:00:00Z",
            )
            .unwrap(),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn sale_posts_tender_revenue_tax_and_known_cost_atomically_and_idempotently() {
        let db = accounting_test_db();
        ensure_accounts(&db, "business-a").unwrap();
        db.execute("INSERT INTO sales VALUES('sale-a','store-a',10000,500,950,10450,'2026-10-30T10:00:00Z')", []).unwrap();
        db.execute(
            "INSERT INTO payments VALUES('pay-a','sale-a','mobile_money',10450)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sale_items VALUES('line-a','sale-a',2,3000)",
            [],
        )
        .unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        assert!(post_sale(&db, &actor, "sale-a").unwrap());
        assert!(!post_sale(&db, &actor, "sale-a").unwrap());
        let (debits, credits, status): (i64, i64, String) = db.query_row(
            "SELECT SUM(l.debit_minor),SUM(l.credit_minor),j.coverage_status FROM accounting_journal_lines l JOIN accounting_journals j ON j.id=l.journal_id WHERE j.source_type='sale' AND j.source_id='sale-a'",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(debits, credits);
        assert_eq!(debits, 16_950);
        assert_eq!(
            status, "partial",
            "tax is recorded without claiming tax classification"
        );
    }

    #[test]
    fn sale_with_missing_cost_is_marked_partial_without_guessing_current_cost() {
        let db = accounting_test_db();
        db.execute(
            "INSERT INTO sales VALUES('sale-b','store-a',1000,0,0,1000,'2026-10-30T10:00:00Z')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO payments VALUES('pay-b','sale-b','cash',1000)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sale_items VALUES('line-b','sale-b',1,NULL)",
            [],
        )
        .unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        post_sale(&db, &actor, "sale-b").unwrap();
        let status: String = db.query_row("SELECT coverage_status FROM accounting_journals WHERE source_type='sale' AND source_id='sale-b'", [], |row| row.get(0)).unwrap();
        let cost_lines: i64 = db.query_row("SELECT COUNT(*) FROM accounting_journal_lines l JOIN accounting_journals j ON j.id=l.journal_id WHERE j.source_id='sale-b' AND l.account_id=(SELECT id FROM accounting_accounts WHERE code='5000')", [], |row| row.get(0)).unwrap();
        assert_eq!(status, "partial");
        assert_eq!(cost_lines, 0);
    }

    #[test]
    fn purchase_receipt_posts_inventory_against_unmatched_grni_once() {
        let db = accounting_test_db();
        ensure_accounts(&db, "business-a").unwrap();
        db.execute("INSERT INTO purchases VALUES('purchase-a','store-a',30500,'2026-10-30T11:00:00Z','INV-42')", []).unwrap();
        db.execute("INSERT INTO purchase_items VALUES('item-a','purchase-a',2,10000,20000),('item-b','purchase-a',3,3500,10500)", []).unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        assert!(post_purchase(&db, &actor, "purchase-a").unwrap());
        assert!(!post_purchase(&db, &actor, "purchase-a").unwrap());
        let (debits, credits, status, description): (i64, i64, String, String) = db.query_row(
            "SELECT SUM(l.debit_minor),SUM(l.credit_minor),j.coverage_status,j.description FROM accounting_journal_lines l JOIN accounting_journals j ON j.id=l.journal_id WHERE j.source_type='purchase' AND j.source_id='purchase-a'",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).unwrap();
        assert_eq!((debits, credits), (30_500, 30_500));
        assert_eq!(status, "complete");
        assert!(description.contains("INV-42"));
    }

    #[test]
    fn zero_value_purchase_has_no_empty_journal_and_mismatched_purchase_is_rejected() {
        let db = accounting_test_db();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        db.execute(
            "INSERT INTO purchases VALUES('purchase-zero','store-a',0,'2026-10-30T11:00:00Z',NULL)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO purchase_items VALUES('item-zero','purchase-zero',1,0,0)",
            [],
        )
        .unwrap();
        assert!(!post_purchase(&db, &actor, "purchase-zero").unwrap());
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM accounting_journals WHERE source_type='purchase'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        db.execute("INSERT INTO purchases VALUES('purchase-bad','store-a',101,'2026-10-30T12:00:00Z',NULL)", []).unwrap();
        db.execute(
            "INSERT INTO purchase_items VALUES('item-bad','purchase-bad',1,100,100)",
            [],
        )
        .unwrap();
        assert!(post_purchase(&db, &actor, "purchase-bad")
            .unwrap_err()
            .contains("do not match"));

        db.execute("INSERT INTO stores VALUES('store-b','business-a')", [])
            .unwrap();
        db.execute("INSERT INTO purchases VALUES('purchase-other-store','store-b',100,'2026-10-30T13:00:00Z',NULL)", []).unwrap();
        db.execute("INSERT INTO purchase_items VALUES('item-other-store','purchase-other-store',1,100,100)", []).unwrap();
        assert!(post_purchase(&db, &actor, "purchase-other-store")
            .unwrap_err()
            .contains("missing"));
    }

    #[test]
    fn failed_purchase_posting_rolls_back_source_transaction() {
        let mut db = accounting_test_db();
        ensure_accounts(&db, "business-a").unwrap();
        db.execute("UPDATE accounting_accounts SET is_active=0 WHERE business_id='business-a' AND code='2110'", []).unwrap();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        tx.execute("INSERT INTO purchases VALUES('purchase-fail','store-a',100,'2026-10-30T12:00:00Z',NULL)", []).unwrap();
        tx.execute(
            "INSERT INTO purchase_items VALUES('item-fail','purchase-fail',1,100,100)",
            [],
        )
        .unwrap();
        let actor = Actor {
            business_id: "business-a",
            store_id: "store-a",
            user_id: Some("user-a"),
            currency_code: "GHS",
        };
        assert!(post_purchase(&tx, &actor, "purchase-fail").is_err());
        drop(tx);
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM purchases WHERE id='purchase-fail'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
