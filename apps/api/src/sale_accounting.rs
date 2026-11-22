use sqlx::{Postgres, Transaction};
use uuid::Uuid;

struct JournalLine {
    account_code: &'static str,
    description: &'static str,
    debit_minor: i64,
    credit_minor: i64,
}

pub(crate) async fn post_sale(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    sale_id: Uuid,
) -> Result<(), String> {
    let (currency, subtotal, tax, total, on_account, effective_at) =
        sqlx::query_as::<_, (String, i64, i64, i64, i64, String)>(
            "SELECT currency_code::text,subtotal_minor,tax_minor,total_minor,on_account_minor,created_at::text
         FROM hosted_sales WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
        )
        .bind(tenant)
        .bind(branch)
        .bind(sale_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| error.to_string())?;
    if subtotal.checked_add(tax) != Some(total) {
        return Err("Hosted sale amounts do not balance.".to_owned());
    }

    let tenders = sqlx::query_as::<_, (String, i64)>(
        "SELECT tender_kind,amount_minor FROM hosted_sale_tenders
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND sale_id=$3 ORDER BY id",
    )
    .bind(tenant)
    .bind(branch)
    .bind(sale_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| error.to_string())?;
    let tender_total = tenders
        .iter()
        .try_fold(0_i64, |sum, (_, amount)| sum.checked_add(*amount));
    if tender_total.and_then(|amount| amount.checked_add(on_account)) != Some(total) {
        return Err("Hosted sale tenders and receivable do not balance.".to_owned());
    }

    let sale_lines = sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT quantity,unit_cost_minor_snapshot FROM hosted_sale_lines
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND sale_id=$3 ORDER BY line_number",
    )
    .bind(tenant)
    .bind(branch)
    .bind(sale_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| error.to_string())?;
    if sale_lines.is_empty() {
        return Err("Hosted sale has no accounting lines.".to_owned());
    }
    let mut coverage_partial = tax > 0;
    let mut cost_total = 0_i64;
    for (quantity, unit_cost) in sale_lines {
        match unit_cost {
            Some(cost) if quantity > 0 => {
                cost_total = cost_total
                    .checked_add(cost.checked_mul(quantity).ok_or("Sale cost overflow.")?)
                    .ok_or("Sale cost overflow.")?;
            }
            _ => coverage_partial = true,
        }
    }

    let mut lines = Vec::new();
    for (kind, amount) in tenders {
        let account = tender_account(&kind);
        lines.push(JournalLine {
            account_code: account,
            description: "Sale tender clearing",
            debit_minor: amount,
            credit_minor: 0,
        });
    }
    push_line(&mut lines, "1100", "Accounts receivable", on_account, 0)?;
    push_line(&mut lines, "4000", "Sales revenue", 0, subtotal)?;
    push_line(
        &mut lines,
        "2100",
        "Tax collected; classification pending",
        0,
        tax,
    )?;
    push_line(&mut lines, "5000", "Cost of goods sold", cost_total, 0)?;
    push_line(
        &mut lines,
        "1300",
        "Inventory relieved at recorded cost",
        0,
        cost_total,
    )?;
    insert_journal(
        tx,
        tenant,
        branch,
        membership,
        "sale",
        sale_id,
        &currency,
        &effective_at,
        "Completed hosted sale",
        coverage_partial,
        lines,
    )
    .await
}

pub(crate) async fn post_refund(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    refund_id: Uuid,
) -> Result<(), String> {
    let (currency, total, effective_at) = sqlx::query_as::<_, (String, i64, String)>(
        "SELECT s.currency_code::text,r.total_minor,r.created_at::text
         FROM hosted_sale_refunds r JOIN hosted_sales s
           ON s.tenant_id=r.tenant_id AND s.branch_id=r.branch_id AND s.id=r.sale_id
         WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid AND r.id=$3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(refund_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| error.to_string())?;
    let tenders = sqlx::query_as::<_, (String, i64)>(
        "SELECT method,amount_minor FROM hosted_sale_refund_tenders
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND refund_id=$3 ORDER BY id",
    )
    .bind(tenant)
    .bind(branch)
    .bind(refund_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| error.to_string())?;
    let tender_total = tenders
        .iter()
        .try_fold(0_i64, |sum, (_, amount)| sum.checked_add(*amount));
    if tenders.is_empty() || tender_total != Some(total) {
        return Err("Hosted refund tenders do not balance.".to_owned());
    }

    let refund_lines = sqlx::query_as::<_, (i64, i64, i64, bool, Option<i64>)>(
        "SELECT rl.subtotal_minor,rl.tax_minor,rl.quantity,rl.restock,sl.unit_cost_minor_snapshot
         FROM hosted_sale_refund_lines rl JOIN hosted_sale_lines sl
           ON sl.tenant_id=rl.tenant_id AND sl.branch_id=rl.branch_id AND sl.sale_id=rl.sale_id AND sl.id=rl.sale_line_id
         WHERE rl.tenant_id=$1::uuid AND rl.branch_id=$2::uuid AND rl.refund_id=$3 ORDER BY rl.id",
    )
    .bind(tenant)
    .bind(branch)
    .bind(refund_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| error.to_string())?;
    if refund_lines.is_empty() {
        return Err("Hosted refund has no accounting lines.".to_owned());
    }
    let mut subtotal = 0_i64;
    let mut tax = 0_i64;
    let mut restored_cost = 0_i64;
    let mut coverage_partial = false;
    for (line_subtotal, line_tax, quantity, restock, unit_cost) in refund_lines {
        subtotal = subtotal
            .checked_add(line_subtotal)
            .ok_or("Refund subtotal overflow.")?;
        tax = tax.checked_add(line_tax).ok_or("Refund tax overflow.")?;
        match unit_cost {
            Some(cost) if quantity > 0 => {
                if restock {
                    restored_cost = restored_cost
                        .checked_add(cost.checked_mul(quantity).ok_or("Refund cost overflow.")?)
                        .ok_or("Refund cost overflow.")?;
                }
            }
            _ => coverage_partial = true,
        }
    }
    if subtotal.checked_add(tax) != Some(total) {
        return Err("Hosted refund amounts do not balance.".to_owned());
    }
    coverage_partial |= tax > 0;

    let mut lines = Vec::new();
    push_line(&mut lines, "4020", "Sales returns", subtotal, 0)?;
    push_line(&mut lines, "2100", "Reverse tax collected", tax, 0)?;
    for (kind, amount) in tenders {
        lines.push(JournalLine {
            account_code: tender_account(&kind),
            description: "Refund tender clearing",
            debit_minor: 0,
            credit_minor: amount,
        });
    }
    push_line(
        &mut lines,
        "1300",
        "Resalable inventory returned",
        restored_cost,
        0,
    )?;
    push_line(
        &mut lines,
        "5000",
        "Reverse cost of resalable return",
        0,
        restored_cost,
    )?;
    insert_journal(
        tx,
        tenant,
        branch,
        membership,
        "sale_refund",
        refund_id,
        &currency,
        &effective_at,
        "Hosted sale refund",
        coverage_partial,
        lines,
    )
    .await?;
    Ok(())
}

fn tender_account(kind: &str) -> &'static str {
    match kind {
        "cash" => "1000",
        "card" => "1010",
        "mobile_money" => "1020",
        "bank_transfer" => "1030",
        _ => "1090",
    }
}

fn push_line(
    lines: &mut Vec<JournalLine>,
    account_code: &'static str,
    description: &'static str,
    debit_minor: i64,
    credit_minor: i64,
) -> Result<(), String> {
    if debit_minor < 0 || credit_minor < 0 || (debit_minor > 0 && credit_minor > 0) {
        return Err("Hosted journal line amount is invalid.".to_owned());
    }
    if debit_minor > 0 || credit_minor > 0 {
        lines.push(JournalLine {
            account_code,
            description,
            debit_minor,
            credit_minor,
        });
    }
    Ok(())
}

async fn insert_journal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    source_type: &str,
    source_id: Uuid,
    currency: &str,
    effective_at: &str,
    description: &str,
    coverage_partial: bool,
    lines: Vec<JournalLine>,
) -> Result<(), String> {
    let journal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description,coverage_status) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7::timestamptz,$8::uuid,$9,$10)")
        .bind(journal_id).bind(tenant).bind(branch).bind(source_type).bind(source_id).bind(currency).bind(effective_at).bind(membership).bind(description).bind(if coverage_partial {"partial"} else {"complete"})
        .execute(&mut **tx).await.map_err(|error| error.to_string())?;
    for (index, line) in lines.into_iter().enumerate() {
        sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(branch).bind(journal_id).bind((index + 1) as i16).bind(line.account_code).bind(line.description).bind(line.debit_minor).bind(line.credit_minor)
            .execute(&mut **tx).await.map_err(|error| error.to_string())?;
    }
    Ok(())
}
