// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::inventory_allocations;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use pos_core::{allocate_sale_components, calculate_sale_totals, PricedLine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const MAX_LINES: usize = 500;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommitSaleInput {
    pub items: Vec<SaleItemInput>,
    pub tenders: Vec<SaleTenderInput>,
    #[serde(default)]
    pub on_account_minor: i64,
    pub customer_id: Option<String>,
    pub pickup_reservation: Option<PickupReservationReference>,
    pub held_ticket_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PickupReservationReference {
    pub reservation_id: String,
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachSaleCustomerInput {
    pub customer_id: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachedSaleCustomer {
    pub sale_id: String,
    pub customer_id: String,
    pub customer_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachCustomerError {
    InvalidInput,
    NotFound,
    Conflict,
    Database,
}

pub(crate) async fn attach_customer(
    pool: &PgPool,
    identity_id: &str,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    sale_id: &str,
    customer_id: &str,
) -> Result<AttachedSaleCustomer, AttachCustomerError> {
    let (Ok(tenant), Ok(branch), Ok(station), Ok(sale), Ok(customer)) = (
        Uuid::parse_str(tenant_id),
        Uuid::parse_str(branch_id),
        Uuid::parse_str(station_id),
        Uuid::parse_str(sale_id),
        Uuid::parse_str(customer_id),
    ) else {
        return Err(AttachCustomerError::InvalidInput);
    };
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| AttachCustomerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await
    .map_err(|_| AttachCustomerError::Database)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{tenant}:{sale}"))
        .execute(&mut *tx)
        .await
        .map_err(|_| AttachCustomerError::Database)?;
    let sale_row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, customer_id::text FROM hosted_sales WHERE tenant_id=$1 AND branch_id=$2 AND station_id=$3 AND id=$4",
    ).bind(tenant).bind(branch).bind(station).bind(sale)
        .fetch_optional(&mut *tx).await.map_err(|_| AttachCustomerError::Database)?
        .ok_or(AttachCustomerError::NotFound)?;
    if let Some(existing) = sale_row.1.as_deref() {
        if existing != customer.to_string() {
            return Err(AttachCustomerError::Conflict);
        }
        let name = sqlx::query_scalar::<_, String>(
            "SELECT customer_name_snapshot FROM hosted_sales WHERE tenant_id=$1 AND id=$2",
        )
        .bind(tenant)
        .bind(sale)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| AttachCustomerError::Database)?;
        tx.commit()
            .await
            .map_err(|_| AttachCustomerError::Database)?;
        return Ok(AttachedSaleCustomer {
            sale_id: sale.to_string(),
            customer_id: customer.to_string(),
            customer_name: name,
        });
    }
    if sale_row.0 != "completed" {
        return Err(AttachCustomerError::Conflict);
    }
    if let Some(existing) = sqlx::query_as::<_, (String, String)>(
        "SELECT customer_id::text,customer_name_snapshot FROM hosted_sale_customer_links WHERE tenant_id=$1 AND sale_id=$2",
    ).bind(tenant).bind(sale).fetch_optional(&mut *tx).await.map_err(|_| AttachCustomerError::Database)? {
        if existing.0 != customer.to_string() { return Err(AttachCustomerError::Conflict); }
        tx.commit().await.map_err(|_| AttachCustomerError::Database)?;
        return Ok(AttachedSaleCustomer { sale_id: sale.to_string(), customer_id: customer.to_string(), customer_name: Some(existing.1) });
    }
    let customer_name = sqlx::query_scalar::<_, String>(
        "SELECT name FROM hosted_customers WHERE tenant_id=$1 AND id=$2 AND status='active'",
    )
    .bind(tenant)
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| AttachCustomerError::Database)?
    .ok_or(AttachCustomerError::NotFound)?;
    sqlx::query("INSERT INTO hosted_sale_customer_links(tenant_id,branch_id,sale_id,customer_id,linked_by_membership_id,customer_name_snapshot) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(tenant).bind(branch).bind(sale).bind(customer).bind(Uuid::parse_str(membership_id).map_err(|_| AttachCustomerError::InvalidInput)?).bind(&customer_name)
        .execute(&mut *tx).await.map_err(|e| if e.as_database_error().is_some_and(|d| d.is_unique_violation()) { AttachCustomerError::Conflict } else { AttachCustomerError::Database })?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1,$2,'sale_customer_linked','hosted_sale',$3,jsonb_build_object('branchId',$4::text,'stationId',$5::text,'customerId',$6::text))")
        .bind(tenant).bind(Uuid::parse_str(membership_id).map_err(|_| AttachCustomerError::InvalidInput)?)
        .bind(sale.to_string()).bind(branch.to_string()).bind(station.to_string()).bind(customer.to_string())
        .execute(&mut *tx).await.map_err(|_| AttachCustomerError::Database)?;
    tx.commit()
        .await
        .map_err(|_| AttachCustomerError::Database)?;
    Ok(AttachedSaleCustomer {
        sale_id: sale.to_string(),
        customer_id: customer.to_string(),
        customer_name: Some(customer_name),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaleItemInput {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaleTenderInput {
    pub payment_option_id: String,
    pub amount_minor: i64,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommittedSale {
    pub id: String,
    pub customer_id: Option<String>,
    pub receipt_number: String,
    pub cashier_name: String,
    pub station_name: String,
    pub currency_code: String,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub on_account_minor: i64,
    pub created_at: String,
    pub replayed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SalesReportQuery {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub offset: Option<u32>,
    pub page_size: Option<u16>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrialBalanceRow {
    pub currency_code: String,
    pub account_code: String,
    pub account_name: String,
    pub account_type: String,
    pub debit_minor: String,
    pub credit_minor: String,
    pub balance_minor: String,
    pub journal_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchTrialBalance {
    pub rows: Vec<TrialBalanceRow>,
    pub currencies: Vec<TrialBalanceCurrencyTotal>,
    pub coverage: BranchAccountingCoverage,
    pub sales_refund_coverage_complete: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrialBalanceCurrencyTotal {
    pub currency_code: String,
    pub total_debits_minor: String,
    pub total_credits_minor: String,
    pub is_balanced: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchIncomeStatement {
    pub currencies: Vec<IncomeStatementCurrency>,
    pub coverage: BranchAccountingCoverage,
    pub sales_refund_coverage_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IncomeStatementCurrency {
    pub currency_code: String,
    pub gross_sales_minor: String,
    pub sales_returns_minor: String,
    pub net_sales_minor: String,
    pub cost_of_goods_sold_minor: String,
    pub gross_profit_minor: String,
    pub operating_expenses_minor: String,
    pub operating_income_minor: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchBalanceSheet {
    pub as_of_date: String,
    pub currencies: Vec<BalanceSheetCurrency>,
    pub coverage: BranchAccountingCoverage,
    pub sales_refund_coverage_complete: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BalanceSheetCurrency {
    pub currency_code: String,
    pub assets_minor: String,
    pub liabilities_minor: String,
    pub contributed_equity_minor: String,
    pub retained_earnings_minor: String,
    pub total_equity_minor: String,
    pub liabilities_and_equity_minor: String,
    pub difference_minor: String,
    pub is_balanced: bool,
    pub accounts: Vec<BalanceSheetAccount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BalanceSheetAccount {
    pub account_code: String,
    pub account_name: String,
    pub account_type: String,
    pub balance_minor: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchCashFlowStatement {
    pub from_date: String,
    pub to_date: String,
    pub currencies: Vec<CashFlowCurrency>,
    pub coverage: BranchAccountingCoverage,
    pub sales_refund_coverage_complete: bool,
    pub cash_reconciliation_status: String,
    pub cash_location_variance_minor: String,
    pub unresolved_drawer_movement_count: i64,
    pub complete: bool,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashFlowCurrency {
    pub currency_code: String,
    pub opening_cash_minor: String,
    pub operating_inflows_minor: String,
    pub operating_outflows_minor: String,
    pub net_operating_cash_flow_minor: String,
    pub cutover_adjustment_minor: String,
    pub unclassified_journal_cash_minor: String,
    pub net_change_minor: String,
    pub ending_cash_minor: String,
    pub reconciliation_difference_minor: String,
    pub is_reconciled: bool,
    pub clearing_accounts: Vec<CashFlowClearingBalance>,
    pub unclassified_register_movement_count: i64,
    pub unclassified_register_movement_net_minor: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashFlowClearingBalance {
    pub account_code: String,
    pub account_name: String,
    pub opening_balance_minor: String,
    pub period_change_minor: String,
    pub ending_balance_minor: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CashFlowJournalRow {
    currency_code: String,
    source_type: String,
    cash_movement_minor: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CashBalanceRow {
    currency_code: String,
    account_code: String,
    opening_balance_minor: String,
    ending_balance_minor: String,
}

#[derive(Default)]
struct CashFlowTotals {
    opening_cash: i128,
    ending_cash: i128,
    operating_inflows: i128,
    operating_outflows: i128,
    cutover_adjustment: i128,
    unclassified_journal_cash: i128,
    clearing: std::collections::BTreeMap<String, (i128, i128)>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SalesReportRow {
    pub id: String,
    pub receipt_number: String,
    pub cashier_name: String,
    pub station_name: String,
    pub currency_code: String,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub payment_methods: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SalesReportCurrencyTotals {
    pub currency_code: String,
    pub sale_count: i64,
    pub subtotal_minor: String,
    pub tax_minor: String,
    pub total_minor: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SalesReport {
    pub rows: Vec<SalesReportRow>,
    pub offset: u32,
    pub page_size: u16,
    pub matched_count: i64,
    pub totals_by_currency: Vec<SalesReportCurrencyTotals>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountingCoverageException {
    pub source_type: String,
    pub source_id: String,
    pub label: String,
    pub occurred_at: String,
    pub issue_code: String,
    pub amount_minor: i64,
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchAccountingCoverage {
    pub sales_count: i64,
    pub unposted_sales_count: i64,
    pub refunds_count: i64,
    pub unposted_refunds_count: i64,
    pub partial_journal_count: i64,
    pub exceptions: Vec<AccountingCoverageException>,
    pub exceptions_truncated: bool,
}

pub(crate) async fn branch_accounting_coverage(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    query: SalesReportQuery,
) -> Result<BranchAccountingCoverage, SaleError> {
    if query
        .from_date
        .as_deref()
        .is_some_and(|date| !valid_iso_date(date))
        || query
            .to_date
            .as_deref()
            .is_some_and(|date| !valid_iso_date(date))
        || matches!((&query.from_date, &query.to_date), (Some(from), Some(to)) if from >= to)
    {
        return Err(SaleError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?;
    }
    let timezone = sqlx::query_scalar::<_, String>(
        "SELECT timezone FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?
    .ok_or(SaleError::NotFound)?;

    let (sales_count, unposted_sales_count) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE j.id IS NULL)::bigint
         FROM hosted_sales s
         LEFT JOIN hosted_accounting_journals j ON j.tenant_id=s.tenant_id AND j.branch_id=s.branch_id
           AND j.source_type='sale' AND j.source_id=s.id
         WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid
           AND ($3::date IS NULL OR s.created_at >= ($3::date::timestamp AT TIME ZONE $5))
           AND ($4::date IS NULL OR s.created_at < ($4::date::timestamp AT TIME ZONE $5))",
    )
    .bind(tenant_id).bind(branch_id).bind(&query.from_date).bind(&query.to_date).bind(&timezone)
    .fetch_one(&mut *tx).await.map_err(|_| SaleError::Database)?;
    let (refunds_count, unposted_refunds_count) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE j.id IS NULL)::bigint
         FROM hosted_sale_refunds r
         LEFT JOIN hosted_accounting_journals j ON j.tenant_id=r.tenant_id AND j.branch_id=r.branch_id
           AND j.source_type='sale_refund' AND j.source_id=r.id
         WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid
           AND ($3::date IS NULL OR r.created_at >= ($3::date::timestamp AT TIME ZONE $5))
           AND ($4::date IS NULL OR r.created_at < ($4::date::timestamp AT TIME ZONE $5))",
    )
    .bind(tenant_id).bind(branch_id).bind(&query.from_date).bind(&query.to_date).bind(&timezone)
    .fetch_one(&mut *tx).await.map_err(|_| SaleError::Database)?;
    let partial_journal_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM hosted_accounting_journals j
         WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.coverage_status='partial'
           AND ($3::date IS NULL OR j.effective_at >= ($3::date::timestamp AT TIME ZONE $5))
           AND ($4::date IS NULL OR j.effective_at < ($4::date::timestamp AT TIME ZONE $5))",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&query.from_date)
    .bind(&query.to_date)
    .bind(&timezone)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;

    let exceptions = sqlx::query_as::<_, AccountingCoverageException>(
        "WITH missing_sales AS (
           SELECT 'sale'::text AS source_type,s.id::text AS source_id,s.receipt_number AS label,
                  to_char(s.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS occurred_at,
                  CASE
                    WHEN COALESCE(lines.total_minor,0)<>s.total_minor OR COALESCE(lines.subtotal_minor,0)+COALESCE(lines.tax_minor,0)<>s.total_minor THEN 'sale_lines_do_not_reconcile'
                    WHEN COALESCE(tenders.total_minor,0)=0 THEN 'sale_tenders_missing'
                    WHEN tenders.total_minor<>s.total_minor THEN 'sale_tenders_do_not_reconcile'
                    ELSE 'sale_journal_missing'
                  END AS issue_code,s.total_minor AS amount_minor,s.currency_code::text AS currency_code,s.created_at AS sort_at
           FROM hosted_sales s
           LEFT JOIN LATERAL (SELECT SUM(l.total_minor)::bigint AS total_minor,SUM(l.subtotal_minor)::bigint AS subtotal_minor,SUM(l.tax_minor)::bigint AS tax_minor
             FROM hosted_sale_lines l WHERE l.tenant_id=s.tenant_id AND l.branch_id=s.branch_id AND l.sale_id=s.id) lines ON true
           LEFT JOIN LATERAL (SELECT SUM(t.amount_minor)::bigint AS total_minor FROM hosted_sale_tenders t
             WHERE t.tenant_id=s.tenant_id AND t.branch_id=s.branch_id AND t.sale_id=s.id) tenders ON true
           WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid
             AND ($3::date IS NULL OR s.created_at >= ($3::date::timestamp AT TIME ZONE $5))
             AND ($4::date IS NULL OR s.created_at < ($4::date::timestamp AT TIME ZONE $5))
             AND NOT EXISTS (SELECT 1 FROM hosted_accounting_journals j WHERE j.tenant_id=s.tenant_id AND j.branch_id=s.branch_id AND j.source_type='sale' AND j.source_id=s.id)
         ), missing_refunds AS (
           SELECT 'refund'::text AS source_type,r.id::text AS source_id,s.receipt_number AS label,
                  to_char(r.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS occurred_at,
                  CASE
                    WHEN COALESCE(lines.total_minor,0)<>r.total_minor OR COALESCE(lines.subtotal_minor,0)+COALESCE(lines.tax_minor,0)<>r.total_minor THEN 'refund_lines_do_not_reconcile'
                    WHEN COALESCE(tenders.total_minor,0)=0 THEN 'refund_tenders_missing'
                    WHEN tenders.total_minor<>r.total_minor THEN 'refund_tenders_do_not_reconcile'
                    ELSE 'refund_journal_missing'
                  END AS issue_code,r.total_minor AS amount_minor,s.currency_code::text AS currency_code,r.created_at AS sort_at
           FROM hosted_sale_refunds r
           JOIN hosted_sales s ON s.tenant_id=r.tenant_id AND s.branch_id=r.branch_id AND s.id=r.sale_id
           LEFT JOIN LATERAL (SELECT SUM(l.amount_minor)::bigint AS total_minor,SUM(l.subtotal_minor)::bigint AS subtotal_minor,SUM(l.tax_minor)::bigint AS tax_minor
             FROM hosted_sale_refund_lines l WHERE l.tenant_id=r.tenant_id AND l.branch_id=r.branch_id AND l.refund_id=r.id) lines ON true
           LEFT JOIN LATERAL (SELECT SUM(t.amount_minor)::bigint AS total_minor FROM hosted_sale_refund_tenders t
             WHERE t.tenant_id=r.tenant_id AND t.branch_id=r.branch_id AND t.refund_id=r.id) tenders ON true
           WHERE r.tenant_id=$1::uuid AND r.branch_id=$2::uuid
             AND ($3::date IS NULL OR r.created_at >= ($3::date::timestamp AT TIME ZONE $5))
             AND ($4::date IS NULL OR r.created_at < ($4::date::timestamp AT TIME ZONE $5))
             AND NOT EXISTS (SELECT 1 FROM hosted_accounting_journals j WHERE j.tenant_id=r.tenant_id AND j.branch_id=r.branch_id AND j.source_type='sale_refund' AND j.source_id=r.id)
         ), partial_journals AS (
           SELECT CASE j.source_type WHEN 'sale' THEN 'partial_sale' ELSE 'partial_refund' END AS source_type,
                  j.source_id::text AS source_id,COALESCE(s.receipt_number,rs.receipt_number,'Historical transaction') AS label,
                  to_char(j.effective_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS occurred_at,
                  'historical_cost_or_tax_classification_unavailable'::text AS issue_code,
                  COALESCE(s.total_minor,r.total_minor,0)::bigint AS amount_minor,j.currency_code::text AS currency_code,j.effective_at AS sort_at
           FROM hosted_accounting_journals j
           LEFT JOIN hosted_sales s ON j.source_type='sale' AND s.tenant_id=j.tenant_id AND s.branch_id=j.branch_id AND s.id=j.source_id
           LEFT JOIN hosted_sale_refunds r ON j.source_type='sale_refund' AND r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id AND r.id=j.source_id
           LEFT JOIN hosted_sales rs ON rs.tenant_id=r.tenant_id AND rs.branch_id=r.branch_id AND rs.id=r.sale_id
           WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.coverage_status='partial'
             AND ($3::date IS NULL OR j.effective_at >= ($3::date::timestamp AT TIME ZONE $5))
             AND ($4::date IS NULL OR j.effective_at < ($4::date::timestamp AT TIME ZONE $5))
         ), combined AS (
           SELECT * FROM missing_sales UNION ALL SELECT * FROM missing_refunds UNION ALL SELECT * FROM partial_journals
         )
           SELECT source_type,source_id,label,occurred_at,issue_code,amount_minor,currency_code FROM combined
         ORDER BY sort_at DESC,source_id DESC LIMIT 51",
    )
    .bind(tenant_id).bind(branch_id).bind(&query.from_date).bind(&query.to_date).bind(&timezone)
    .fetch_all(&mut *tx).await.map_err(|_| SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;
    let exceptions_truncated = exceptions.len() > 50;
    Ok(BranchAccountingCoverage {
        sales_count,
        unposted_sales_count,
        refunds_count,
        unposted_refunds_count,
        partial_journal_count,
        exceptions: exceptions.into_iter().take(50).collect(),
        exceptions_truncated,
    })
}

pub(crate) async fn branch_trial_balance(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    query: SalesReportQuery,
) -> Result<BranchTrialBalance, SaleError> {
    let coverage = branch_accounting_coverage(
        pool,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        query.clone(),
    )
    .await?;
    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?;
    }
    let timezone = sqlx::query_scalar::<_, String>(
        "SELECT timezone FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?
    .ok_or(SaleError::NotFound)?;
    let rows = sqlx::query_as::<_, TrialBalanceRow>(
        "SELECT j.currency_code::text AS currency_code,l.account_code,
                COALESCE(MIN(CASE l.account_code
                  WHEN '1000' THEN 'Cash'
                  WHEN '1010' THEN 'Card clearing'
                  WHEN '1020' THEN 'Mobile money clearing'
                  WHEN '1030' THEN 'Bank transfer clearing'
                  WHEN '1090' THEN 'Other tender clearing'
                  WHEN '1300' THEN 'Inventory'
                  WHEN '2000' THEN 'Accounts payable'
                  WHEN '2100' THEN 'Tax payable'
                  WHEN '3000' THEN 'Equity'
                  WHEN '4000' THEN 'Sales revenue'
                  WHEN '4020' THEN 'Sales returns'
                  WHEN '5000' THEN 'Cost of goods sold'
                  WHEN '6000' THEN 'Operating expenses'
                  ELSE l.description END),l.account_code) AS account_name,
                CASE LEFT(l.account_code,1) WHEN '1' THEN 'asset' WHEN '2' THEN 'liability'
                  WHEN '3' THEN 'equity' WHEN '4' THEN 'revenue' ELSE 'expense' END AS account_type,
                SUM(l.debit_minor)::text AS debit_minor,SUM(l.credit_minor)::text AS credit_minor,
                SUM(l.debit_minor-l.credit_minor)::text AS balance_minor,
                COUNT(DISTINCT j.id)::bigint AS journal_count
         FROM hosted_accounting_journals j
         JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
         WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid
           AND ($3::date IS NULL OR j.effective_at >= ($3::date::timestamp AT TIME ZONE $5))
           AND ($4::date IS NULL OR j.effective_at < ($4::date::timestamp AT TIME ZONE $5))
         GROUP BY j.currency_code,l.account_code ORDER BY j.currency_code,l.account_code",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&query.from_date)
    .bind(&query.to_date)
    .bind(&timezone)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;

    let mut currencies: Vec<TrialBalanceCurrencyTotal> = Vec::new();
    for row in &rows {
        let debit = row
            .debit_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let credit = row
            .credit_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let total = currencies
            .iter_mut()
            .find(|total| total.currency_code == row.currency_code);
        if let Some(total) = total {
            let next_debits = total
                .total_debits_minor
                .parse::<i128>()
                .map_err(|_| SaleError::Database)?
                + debit;
            let next_credits = total
                .total_credits_minor
                .parse::<i128>()
                .map_err(|_| SaleError::Database)?
                + credit;
            total.total_debits_minor = next_debits.to_string();
            total.total_credits_minor = next_credits.to_string();
            total.is_balanced = next_debits == next_credits;
        } else {
            currencies.push(TrialBalanceCurrencyTotal {
                currency_code: row.currency_code.clone(),
                total_debits_minor: debit.to_string(),
                total_credits_minor: credit.to_string(),
                is_balanced: debit == credit,
            });
        }
    }
    let sales_refund_coverage_complete = coverage.unposted_sales_count == 0
        && coverage.unposted_refunds_count == 0
        && coverage.partial_journal_count == 0;
    Ok(BranchTrialBalance {
        rows,
        currencies,
        coverage,
        sales_refund_coverage_complete,
    })
}

pub(crate) async fn branch_income_statement(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    query: SalesReportQuery,
) -> Result<BranchIncomeStatement, SaleError> {
    let trial_balance = branch_trial_balance(
        pool,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        query,
    )
    .await?;
    let mut totals_by_currency = std::collections::BTreeMap::<String, [i128; 4]>::new();
    for row in trial_balance.rows {
        let account_code = row
            .account_code
            .parse::<u16>()
            .map_err(|_| SaleError::Database)?;
        let balance = row
            .balance_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let (slot, amount) = match account_code {
            4000 => (0, balance.checked_neg().ok_or(SaleError::Database)?),
            4020 => (1, balance),
            5000 => (2, balance),
            5100..=6000 => (3, balance),
            _ => continue,
        };
        let value = totals_by_currency
            .entry(row.currency_code)
            .or_insert([0; 4]);
        value[slot] = value[slot].checked_add(amount).ok_or(SaleError::Database)?;
    }
    let mut currencies = Vec::with_capacity(totals_by_currency.len());
    for (currency_code, totals) in totals_by_currency {
        let [gross_sales, sales_returns, cost_of_goods_sold, operating_expenses] = totals;
        let net_sales = gross_sales
            .checked_sub(sales_returns)
            .ok_or(SaleError::Database)?;
        let gross_profit = net_sales
            .checked_sub(cost_of_goods_sold)
            .ok_or(SaleError::Database)?;
        let operating_income = gross_profit
            .checked_sub(operating_expenses)
            .ok_or(SaleError::Database)?;
        currencies.push(IncomeStatementCurrency {
            currency_code,
            gross_sales_minor: gross_sales.to_string(),
            sales_returns_minor: sales_returns.to_string(),
            net_sales_minor: net_sales.to_string(),
            cost_of_goods_sold_minor: cost_of_goods_sold.to_string(),
            gross_profit_minor: gross_profit.to_string(),
            operating_expenses_minor: operating_expenses.to_string(),
            operating_income_minor: operating_income.to_string(),
        });
    }
    Ok(BranchIncomeStatement {
        currencies,
        coverage: trial_balance.coverage,
        sales_refund_coverage_complete: trial_balance.sales_refund_coverage_complete,
    })
}

pub(crate) async fn branch_balance_sheet(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    as_of_date: String,
) -> Result<BranchBalanceSheet, SaleError> {
    let Some(to_date) = next_iso_date(&as_of_date) else {
        return Err(SaleError::InvalidInput);
    };
    let trial_balance = branch_trial_balance(
        pool,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        SalesReportQuery {
            from_date: None,
            to_date: Some(to_date),
            offset: None,
            page_size: None,
        },
    )
    .await?;

    // Assets, liabilities, posted equity, and the net result of posted temporary
    // accounts. Temporary accounts are shown as accumulated retained earnings
    // until the product supports formal period-close closing entries.
    let mut totals = std::collections::BTreeMap::<String, [i128; 4]>::new();
    let mut accounts = std::collections::BTreeMap::<String, Vec<BalanceSheetAccount>>::new();
    for row in trial_balance.rows {
        let account_code = row
            .account_code
            .parse::<u16>()
            .map_err(|_| SaleError::Database)?;
        let debit_balance = row
            .balance_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let value = totals.entry(row.currency_code.clone()).or_insert([0; 4]);
        let normal_balance = match row.account_type.as_str() {
            "asset" => {
                value[0] = value[0]
                    .checked_add(debit_balance)
                    .ok_or(SaleError::Database)?;
                Some(debit_balance)
            }
            "liability" => {
                let credit_balance = debit_balance.checked_neg().ok_or(SaleError::Database)?;
                value[1] = value[1]
                    .checked_add(credit_balance)
                    .ok_or(SaleError::Database)?;
                Some(credit_balance)
            }
            "equity" => {
                let credit_balance = debit_balance.checked_neg().ok_or(SaleError::Database)?;
                value[2] = value[2]
                    .checked_add(credit_balance)
                    .ok_or(SaleError::Database)?;
                Some(credit_balance)
            }
            "revenue" | "expense" => {
                let net_income_contribution =
                    debit_balance.checked_neg().ok_or(SaleError::Database)?;
                value[3] = value[3]
                    .checked_add(net_income_contribution)
                    .ok_or(SaleError::Database)?;
                None
            }
            _ => return Err(SaleError::Database),
        };
        if let Some(balance_minor) = normal_balance {
            accounts
                .entry(row.currency_code)
                .or_default()
                .push(BalanceSheetAccount {
                    account_code: row.account_code,
                    account_name: row.account_name,
                    account_type: row.account_type,
                    balance_minor: balance_minor.to_string(),
                });
        } else if account_code < 4000 {
            return Err(SaleError::Database);
        }
    }

    let mut currencies = Vec::with_capacity(totals.len());
    for (currency_code, values) in totals {
        let [assets, liabilities, contributed_equity, retained_earnings] = values;
        let total_equity = contributed_equity
            .checked_add(retained_earnings)
            .ok_or(SaleError::Database)?;
        let liabilities_and_equity = liabilities
            .checked_add(total_equity)
            .ok_or(SaleError::Database)?;
        let difference = assets
            .checked_sub(liabilities_and_equity)
            .ok_or(SaleError::Database)?;
        currencies.push(BalanceSheetCurrency {
            currency_code: currency_code.clone(),
            assets_minor: assets.to_string(),
            liabilities_minor: liabilities.to_string(),
            contributed_equity_minor: contributed_equity.to_string(),
            retained_earnings_minor: retained_earnings.to_string(),
            total_equity_minor: total_equity.to_string(),
            liabilities_and_equity_minor: liabilities_and_equity.to_string(),
            difference_minor: difference.to_string(),
            is_balanced: difference == 0,
            accounts: accounts.remove(&currency_code).unwrap_or_default(),
        });
    }

    Ok(BranchBalanceSheet {
        as_of_date,
        currencies,
        coverage: trial_balance.coverage,
        sales_refund_coverage_complete: trial_balance.sales_refund_coverage_complete,
        // Opening balances and non-sale/refund source coverage have not been
        // proven complete, so the hosted statement is intentionally provisional.
        complete: false,
    })
}

fn next_iso_date(value: &str) -> Option<String> {
    if !valid_iso_date(value) {
        return None;
    }
    let mut year = value[0..4].parse::<u32>().ok()?;
    let mut month = value[5..7].parse::<u32>().ok()?;
    let mut day = value[8..10].parse::<u32>().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < days_in_month {
        day += 1;
    } else if month < 12 {
        month += 1;
        day = 1;
    } else {
        year = year.checked_add(1)?;
        if year > 9999 {
            return None;
        }
        month = 1;
        day = 1;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

pub(crate) async fn branch_cash_flow_statement(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    query: SalesReportQuery,
) -> Result<BranchCashFlowStatement, SaleError> {
    let (Some(from_date), Some(to_date)) = (query.from_date.clone(), query.to_date.clone()) else {
        return Err(SaleError::InvalidInput);
    };
    if query.offset.is_some() || query.page_size.is_some() {
        return Err(SaleError::InvalidInput);
    }
    let coverage = branch_accounting_coverage(
        pool,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        query,
    )
    .await?;

    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        "",
    )
    .await
    .map_err(|_| SaleError::Database)?;
    let (timezone, branch_currency): (String, String) = sqlx::query_as(
        "SELECT timezone,currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?
    .ok_or(SaleError::NotFound)?;

    // Account 1000 is physical cash. Tender clearing accounts are shown below
    // as unsettled receivables and are deliberately excluded from cash flow.
    let journal_rows = sqlx::query_as::<_, CashFlowJournalRow>(
        "SELECT j.currency_code,j.source_type,
          SUM(CASE WHEN l.account_code='1000' THEN l.debit_minor::numeric-l.credit_minor::numeric ELSE 0 END)::text AS cash_movement_minor
         FROM hosted_accounting_journals j
         JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
         WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid
           AND j.effective_at >= ($3::date::timestamp AT TIME ZONE $5)
           AND j.effective_at < ($4::date::timestamp AT TIME ZONE $5)
         GROUP BY j.currency_code,j.id,j.source_type
         HAVING SUM(CASE WHEN l.account_code='1000' THEN l.debit_minor::numeric-l.credit_minor::numeric ELSE 0 END)<>0
         ORDER BY j.currency_code,j.id",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&from_date)
    .bind(&to_date)
    .bind(&timezone)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;

    let balance_rows = sqlx::query_as::<_, CashBalanceRow>(
        "SELECT j.currency_code,l.account_code,
          COALESCE(SUM(CASE WHEN j.effective_at < ($3::date::timestamp AT TIME ZONE $5) THEN l.debit_minor::numeric-l.credit_minor::numeric ELSE 0 END),0)::text AS opening_balance_minor,
          COALESCE(SUM(l.debit_minor::numeric-l.credit_minor::numeric),0)::text AS ending_balance_minor
         FROM hosted_accounting_journals j
         JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
         WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND l.account_code IN ('1000','1010','1020','1030','1090')
           AND j.effective_at < ($4::date::timestamp AT TIME ZONE $5)
         GROUP BY j.currency_code,l.account_code ORDER BY j.currency_code,l.account_code",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&from_date)
    .bind(&to_date)
    .bind(&timezone)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;

    let (manual_movement_count, manual_movement_net): (i64, String) = sqlx::query_as(
        "SELECT COUNT(*)::bigint,COALESCE(SUM(e.signed_amount_minor::numeric),0)::text
         FROM hosted_cash_location_events e
         WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid
           AND e.event_type IN ('register_movement_inflow','register_movement_outflow')
           AND e.created_at >= ($3::date::timestamp AT TIME ZONE $4)
           AND e.created_at < ($5::date::timestamp AT TIME ZONE $4)
           AND NOT EXISTS (SELECT 1 FROM hosted_cash_location_events d
             WHERE d.tenant_id=e.tenant_id AND d.branch_id=e.branch_id
               AND d.source_drawer_movement_id=e.source_drawer_movement_id AND d.event_type='cash_drop_inflow')",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&from_date)
    .bind(&timezone)
    .bind(&to_date)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;

    let reconciliation = crate::cash_locations::reconciliation_status(
        pool,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
    )
    .await
    .map_err(|_| SaleError::Database)?;

    let mut totals = std::collections::BTreeMap::<String, CashFlowTotals>::new();
    totals.entry(branch_currency.clone()).or_default();
    for row in balance_rows {
        let opening = row
            .opening_balance_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let ending = row
            .ending_balance_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let value = totals.entry(row.currency_code).or_default();
        if row.account_code == "1000" {
            value.opening_cash = opening;
            value.ending_cash = ending;
        } else {
            value.clearing.insert(row.account_code, (opening, ending));
        }
    }
    for row in journal_rows {
        let movement = row
            .cash_movement_minor
            .parse::<i128>()
            .map_err(|_| SaleError::Database)?;
        let value = totals.entry(row.currency_code).or_default();
        match row.source_type.as_str() {
            "sale" | "expense_reimbursement" => {
                if movement >= 0 {
                    value.operating_inflows = value
                        .operating_inflows
                        .checked_add(movement)
                        .ok_or(SaleError::Database)?;
                } else {
                    value.operating_outflows = value
                        .operating_outflows
                        .checked_add(movement.checked_neg().ok_or(SaleError::Database)?)
                        .ok_or(SaleError::Database)?;
                }
            }
            "sale_refund" | "expense" | "supplier_payment" => {
                if movement <= 0 {
                    value.operating_outflows = value
                        .operating_outflows
                        .checked_add(movement.checked_neg().ok_or(SaleError::Database)?)
                        .ok_or(SaleError::Database)?;
                } else {
                    value.operating_inflows = value
                        .operating_inflows
                        .checked_add(movement)
                        .ok_or(SaleError::Database)?;
                }
            }
            "cash_reconciliation" => {
                value.cutover_adjustment = value
                    .cutover_adjustment
                    .checked_add(movement)
                    .ok_or(SaleError::Database)?;
            }
            // Accrual bills and non-cash journals should not move account 1000.
            // Any unexpected cash line stays visible as an unclassified item.
            _ => {
                value.unclassified_journal_cash = value
                    .unclassified_journal_cash
                    .checked_add(movement)
                    .ok_or(SaleError::Database)?;
            }
        }
    }

    let mut currencies = Vec::with_capacity(totals.len());
    let mut has_unsettled_clearing = false;
    for (currency_code, mut value) in totals {
        let mut clearing_accounts = Vec::with_capacity(4);
        for (account_code, account_name) in [
            ("1010", "Card clearing"),
            ("1020", "Mobile money clearing"),
            ("1030", "Bank transfer clearing"),
            ("1090", "Other tender clearing"),
        ] {
            let (opening, ending) = value.clearing.remove(account_code).unwrap_or((0, 0));
            let movement = ending.checked_sub(opening).ok_or(SaleError::Database)?;
            has_unsettled_clearing |= ending != 0;
            clearing_accounts.push(CashFlowClearingBalance {
                account_code: account_code.to_owned(),
                account_name: account_name.to_owned(),
                opening_balance_minor: opening.to_string(),
                period_change_minor: movement.to_string(),
                ending_balance_minor: ending.to_string(),
            });
        }
        let operating_net = value
            .operating_inflows
            .checked_sub(value.operating_outflows)
            .ok_or(SaleError::Database)?;
        let net_change = value
            .ending_cash
            .checked_sub(value.opening_cash)
            .ok_or(SaleError::Database)?;
        let explained_change = operating_net
            .checked_add(value.cutover_adjustment)
            .and_then(|amount| amount.checked_add(value.unclassified_journal_cash))
            .ok_or(SaleError::Database)?;
        let difference = net_change
            .checked_sub(explained_change)
            .ok_or(SaleError::Database)?;
        currencies.push(CashFlowCurrency {
            currency_code,
            opening_cash_minor: value.opening_cash.to_string(),
            operating_inflows_minor: value.operating_inflows.to_string(),
            operating_outflows_minor: value.operating_outflows.to_string(),
            net_operating_cash_flow_minor: operating_net.to_string(),
            cutover_adjustment_minor: value.cutover_adjustment.to_string(),
            unclassified_journal_cash_minor: value.unclassified_journal_cash.to_string(),
            net_change_minor: net_change.to_string(),
            ending_cash_minor: value.ending_cash.to_string(),
            reconciliation_difference_minor: difference.to_string(),
            is_reconciled: difference == 0,
            clearing_accounts,
            unclassified_register_movement_count: manual_movement_count,
            unclassified_register_movement_net_minor: manual_movement_net.clone(),
        });
    }
    let mut limitations = vec![
        "bank and payment processor settlements are not independently verified",
        "investing and financing activity classifications are not implemented",
        "this report covers posted physical cash account 1000 only",
    ];
    if !coverage.exceptions.is_empty()
        || coverage.unposted_sales_count > 0
        || coverage.unposted_refunds_count > 0
    {
        limitations.push("sale and refund journal coverage is incomplete or has exceptions");
    }
    if manual_movement_count > 0 || reconciliation.unresolved_drawer_movement_count > 0 {
        limitations.push("manual drawer movements remain unclassified");
    }
    if has_unsettled_clearing {
        limitations.push("tender clearing balances are not verified cash or bank settlement");
    }
    if reconciliation.status != "reconciled" {
        limitations.push("physical cash locations are not fully reconciled to account 1000");
    }
    Ok(BranchCashFlowStatement {
        from_date,
        to_date,
        currencies,
        sales_refund_coverage_complete: coverage.unposted_sales_count == 0
            && coverage.unposted_refunds_count == 0
            && coverage.partial_journal_count == 0,
        coverage,
        cash_reconciliation_status: reconciliation.status.to_owned(),
        cash_location_variance_minor: reconciliation.variance_minor.to_string(),
        unresolved_drawer_movement_count: reconciliation.unresolved_drawer_movement_count,
        complete: false,
        limitations,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StationSalesHistoryQuery {
    pub search: Option<String>,
    pub offset: Option<u32>,
    pub page_size: Option<u16>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StationSalesHistoryRow {
    pub id: String,
    pub receipt_number: String,
    pub cashier_name: String,
    pub customer_name: Option<String>,
    pub currency_code: String,
    pub total_minor: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StationSalesHistory {
    pub rows: Vec<StationSalesHistoryRow>,
    pub offset: u32,
    pub page_size: u16,
    pub matched_count: i64,
}

pub(crate) async fn station_sales_history(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    query: StationSalesHistoryQuery,
) -> Result<StationSalesHistory, SaleError> {
    let search = query.search.map(|value| value.trim().to_owned());
    if search
        .as_deref()
        .is_some_and(|value| value.len() > 120 || value.chars().any(char::is_control))
        || query.offset.is_some_and(|offset| offset > 50_000)
        || query
            .page_size
            .is_some_and(|size| !(1..=100).contains(&size))
    {
        return Err(SaleError::InvalidInput);
    }
    let search = search.filter(|value| !value.is_empty());
    let offset = query.offset.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(25);
    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;

    let matched_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::bigint
         FROM hosted_sales s
         LEFT JOIN hosted_sale_customer_links link
           ON link.tenant_id=s.tenant_id AND link.branch_id=s.branch_id AND link.sale_id=s.id
         WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid AND s.station_id=$3::uuid
           AND ($4::text IS NULL
             OR strpos(lower(s.receipt_number),lower($4)) > 0
             OR strpos(lower(COALESCE(link.customer_name_snapshot,s.customer_name_snapshot,'')),lower($4)) > 0
             OR EXISTS (SELECT 1 FROM hosted_sale_lines line
                        WHERE line.tenant_id=s.tenant_id AND line.branch_id=s.branch_id AND line.sale_id=s.id
                          AND (strpos(lower(line.name_snapshot),lower($4)) > 0
                            OR strpos(lower(line.sku_snapshot),lower($4)) > 0
                            OR strpos(lower(COALESCE(line.barcode_snapshot,'')),lower($4)) > 0))
             OR EXISTS (SELECT 1 FROM hosted_sale_tenders tender
                        WHERE tender.tenant_id=s.tenant_id AND tender.branch_id=s.branch_id AND tender.sale_id=s.id
                          AND strpos(lower(tender.label_snapshot),lower($4)) > 0))",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(&search)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    let rows = sqlx::query_as::<_, StationSalesHistoryRow>(
        "SELECT s.id::text, s.receipt_number, s.cashier_name_snapshot AS cashier_name,
                COALESCE(link.customer_name_snapshot,s.customer_name_snapshot) AS customer_name,
                s.currency_code::text AS currency_code, s.total_minor,
                to_char(s.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at
         FROM hosted_sales s
         LEFT JOIN hosted_sale_customer_links link
           ON link.tenant_id=s.tenant_id AND link.branch_id=s.branch_id AND link.sale_id=s.id
         WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid AND s.station_id=$3::uuid
           AND ($4::text IS NULL
             OR strpos(lower(s.receipt_number),lower($4)) > 0
             OR strpos(lower(COALESCE(link.customer_name_snapshot,s.customer_name_snapshot,'')),lower($4)) > 0
             OR EXISTS (SELECT 1 FROM hosted_sale_lines line
                        WHERE line.tenant_id=s.tenant_id AND line.branch_id=s.branch_id AND line.sale_id=s.id
                          AND (strpos(lower(line.name_snapshot),lower($4)) > 0
                            OR strpos(lower(line.sku_snapshot),lower($4)) > 0
                            OR strpos(lower(COALESCE(line.barcode_snapshot,'')),lower($4)) > 0))
             OR EXISTS (SELECT 1 FROM hosted_sale_tenders tender
                        WHERE tender.tenant_id=s.tenant_id AND tender.branch_id=s.branch_id AND tender.sale_id=s.id
                          AND strpos(lower(tender.label_snapshot),lower($4)) > 0))
         ORDER BY s.created_at DESC,s.id DESC LIMIT $5 OFFSET $6",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id)
    .bind(&search)
    .bind(i64::from(page_size))
    .bind(i64::from(offset))
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;
    Ok(StationSalesHistory {
        rows,
        offset,
        page_size,
        matched_count,
    })
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedReceiptLine {
    pub line_number: i32,
    pub sku: String,
    pub name: String,
    pub quantity: i64,
    pub unit_price_minor: i64,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedReceiptTender {
    pub label: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedReceipt {
    pub id: String,
    pub customer_id: Option<String>,
    pub customer_name: Option<String>,
    pub receipt_number: String,
    pub cashier_name: String,
    pub station_name: String,
    pub currency_code: String,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub on_account_minor: i64,
    pub due_at: Option<String>,
    pub created_at: String,
    pub lines: Vec<HostedReceiptLine>,
    pub tenders: Vec<HostedReceiptTender>,
}

pub(crate) async fn receipt(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    sale_id: &str,
) -> Result<HostedReceipt, SaleError> {
    let sale = Uuid::parse_str(sale_id).map_err(|_| SaleError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id.unwrap_or_default(),
    )
    .await?;
    let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, String, String, String, String, i64, i64, i64, i64, Option<String>, String)>(
        "SELECT s.id::text,COALESCE(link.customer_id,s.customer_id)::text,COALESCE(link.customer_name_snapshot,s.customer_name_snapshot),s.receipt_number,s.cashier_name_snapshot,s.station_name_snapshot,s.currency_code,s.subtotal_minor,s.tax_minor,s.total_minor,s.on_account_minor,(SELECT to_char(i.due_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM hosted_receivable_invoices i WHERE i.tenant_id=s.tenant_id AND i.branch_id=s.branch_id AND i.sale_id=s.id),to_char(s.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM hosted_sales s LEFT JOIN hosted_sale_customer_links link ON link.tenant_id=s.tenant_id AND link.branch_id=s.branch_id AND link.sale_id=s.id WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid AND s.id=$3 AND ($4::uuid IS NULL OR s.station_id=$4::uuid)",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .bind(station_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?
    .ok_or(SaleError::NotFound)?;
    let lines = sqlx::query_as::<_, HostedReceiptLine>(
        "SELECT line_number,sku_snapshot AS sku,name_snapshot AS name,quantity,unit_price_minor,subtotal_minor,tax_minor,total_minor FROM hosted_sale_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND sale_id=$3 ORDER BY line_number",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    let tenders = sqlx::query_as::<_, HostedReceiptTender>(
        "SELECT label_snapshot AS label,amount_minor FROM hosted_sale_tenders WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND sale_id=$3 ORDER BY recorded_at,id",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(sale)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;
    Ok(HostedReceipt {
        id: row.0,
        customer_id: row.1,
        customer_name: row.2,
        receipt_number: row.3,
        cashier_name: row.4,
        station_name: row.5,
        currency_code: row.6,
        subtotal_minor: row.7,
        tax_minor: row.8,
        total_minor: row.9,
        on_account_minor: row.10,
        due_at: row.11,
        created_at: row.12,
        lines,
        tenders,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentBranchSalesSummary {
    pub sale_count: i64,
    pub totals_by_currency: Vec<SalesReportCurrencyTotals>,
}

pub(crate) async fn recent_branch_summary(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<RecentBranchSalesSummary, SaleError> {
    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?;
    }
    let totals_by_currency = sqlx::query_as::<_, SalesReportCurrencyTotals>(
        "SELECT currency_code::text AS currency_code, COUNT(*)::bigint AS sale_count,
                COALESCE(SUM(subtotal_minor),0)::text AS subtotal_minor,
                COALESCE(SUM(tax_minor),0)::text AS tax_minor,
                COALESCE(SUM(total_minor),0)::text AS total_minor
         FROM hosted_sales
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
           AND created_at >= now() - interval '7 days'
         GROUP BY currency_code ORDER BY currency_code",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    let sale_count = totals_by_currency
        .iter()
        .try_fold(0_i64, |count, item| count.checked_add(item.sale_count))
        .ok_or(SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;
    Ok(RecentBranchSalesSummary {
        sale_count,
        totals_by_currency,
    })
}

pub(crate) async fn branch_report(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    query: SalesReportQuery,
) -> Result<SalesReport, SaleError> {
    if query
        .from_date
        .as_deref()
        .is_some_and(|date| !valid_iso_date(date))
        || query
            .to_date
            .as_deref()
            .is_some_and(|date| !valid_iso_date(date))
        || matches!((&query.from_date, &query.to_date), (Some(from), Some(to)) if from >= to)
        || query.offset.is_some_and(|offset| offset > 50_000)
        || query
            .page_size
            .is_some_and(|size| !(1..=100).contains(&size))
    {
        return Err(SaleError::InvalidInput);
    }
    let offset = query.offset.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(50);
    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?;
    }
    let timezone = sqlx::query_scalar::<_, String>(
        "SELECT timezone FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?
    .ok_or(SaleError::NotFound)?;
    let totals_by_currency = sqlx::query_as::<_, SalesReportCurrencyTotals>(
        "SELECT currency_code::text AS currency_code, COUNT(*)::bigint AS sale_count,
                COALESCE(SUM(subtotal_minor),0)::text AS subtotal_minor,
                COALESCE(SUM(tax_minor),0)::text AS tax_minor,
                COALESCE(SUM(total_minor),0)::text AS total_minor
         FROM hosted_sales
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
           AND ($3::date IS NULL OR created_at >= ($3::date::timestamp AT TIME ZONE $5))
           AND ($4::date IS NULL OR created_at < ($4::date::timestamp AT TIME ZONE $5))
         GROUP BY currency_code ORDER BY currency_code",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&query.from_date)
    .bind(&query.to_date)
    .bind(&timezone)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    let matched_count = totals_by_currency
        .iter()
        .try_fold(0i64, |count, totals| count.checked_add(totals.sale_count))
        .ok_or(SaleError::Database)?;
    let rows = sqlx::query_as::<_, SalesReportRow>(
        "SELECT s.id::text, s.receipt_number, s.cashier_name_snapshot AS cashier_name,
                s.station_name_snapshot AS station_name, s.currency_code,
                s.subtotal_minor, s.tax_minor, s.total_minor,
                COALESCE((SELECT string_agg(DISTINCT t.label_snapshot, ', ' ORDER BY t.label_snapshot)
                          FROM hosted_sale_tenders t
                          WHERE t.tenant_id=s.tenant_id AND t.branch_id=s.branch_id AND t.sale_id=s.id), '') AS payment_methods,
                to_char(s.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at
         FROM hosted_sales s
         WHERE s.tenant_id=$1::uuid AND s.branch_id=$2::uuid
           AND ($3::date IS NULL OR s.created_at >= ($3::date::timestamp AT TIME ZONE $5))
           AND ($4::date IS NULL OR s.created_at < ($4::date::timestamp AT TIME ZONE $5))
         ORDER BY s.created_at DESC, s.id DESC LIMIT $6 OFFSET $7",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&query.from_date)
    .bind(&query.to_date)
    .bind(&timezone)
    .bind(i64::from(page_size))
    .bind(i64::from(offset))
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?;
    tx.commit().await.map_err(|_| SaleError::Database)?;
    Ok(SalesReport {
        rows,
        offset,
        page_size,
        matched_count,
        totals_by_currency,
    })
}

fn valid_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        value[0..4].parse::<u32>(),
        value[5..7].parse::<u32>(),
        value[8..10].parse::<u32>(),
    ) else {
        return false;
    };
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

#[cfg(test)]
mod report_tests {
    use super::valid_iso_date;

    #[test]
    fn report_dates_require_real_iso_calendar_dates() {
        assert!(valid_iso_date("2024-02-29"));
        assert!(valid_iso_date("2026-10-31"));
        assert!(!valid_iso_date("2025-02-29"));
        assert!(!valid_iso_date("2026-13-01"));
        assert!(!valid_iso_date("2026-04-31"));
        assert!(!valid_iso_date("0000-01-01"));
        assert!(!valid_iso_date("not-a-date"));
        assert!(!valid_iso_date("éééé-10-01"));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaleError {
    InvalidInput,
    LeaseUnavailable,
    InvalidProof,
    InsufficientStock,
    TenderUnavailable,
    PickupUnavailable,
    DrawerRequired,
    RegisterCutoverRequired,
    IdempotencyConflict,
    NotFound,
    Database,
}

struct ProductRow {
    product_id: String,
    sku: String,
    barcode: Option<String>,
    name: String,
    price_minor: i64,
    tax_rate_bps: i32,
    cost_minor: Option<i64>,
}

pub(crate) async fn commit(
    pool: &PgPool,
    identity_id: &str,
    cashier_display_name: Option<&str>,
    membership_id: &str,
    tenant_id: &str,
    branch_id: &str,
    station_id: &str,
    lease_id: &str,
    lease_token: &str,
    idempotency_key: &str,
    challenge_id: &str,
    proof_hex: &str,
    raw_body: &[u8],
    input: CommitSaleInput,
) -> Result<CommittedSale, SaleError> {
    if input.items.is_empty()
        || input.items.len() > MAX_LINES
        || input.tenders.len() > 8
        || input.on_account_minor < 0
        || input.on_account_minor > MAX_SAFE_INTEGER
        || (input.on_account_minor > 0 && input.customer_id.is_none())
        || (input.tenders.is_empty() && input.on_account_minor == 0)
        || !valid_key(idempotency_key)
    {
        return Err(SaleError::InvalidInput);
    }
    let held_ticket_id = input
        .held_ticket_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| SaleError::InvalidInput)?;
    let payload_hash = Sha256::digest(raw_body).to_vec();
    let lease_token_hash = Sha256::digest(lease_token.as_bytes()).to_vec();
    let proof = decode_hex(proof_hex, 64).ok_or(SaleError::InvalidProof)?;
    let proof: [u8; 64] = proof.try_into().map_err(|_| SaleError::InvalidProof)?;
    let item_ids = input
        .items
        .iter()
        .map(|item| {
            if item.quantity <= 0 || item.quantity > MAX_SAFE_INTEGER {
                return Err(SaleError::InvalidInput);
            }
            Uuid::parse_str(&item.product_id).map_err(|_| SaleError::InvalidInput)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let customer_id = input
        .customer_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| SaleError::InvalidInput)?;
    let pickup_reservation = input
        .pickup_reservation
        .as_ref()
        .map(|reference| {
            if reference.expected_revision <= 0 {
                return Err(SaleError::InvalidInput);
            }
            Uuid::parse_str(&reference.reservation_id).map_err(|_| SaleError::InvalidInput)
        })
        .transpose()?;
    let mut quantities = std::collections::BTreeMap::<Uuid, i64>::new();
    for (item, id) in input.items.iter().zip(item_ids) {
        let quantity = quantities.entry(id).or_default();
        *quantity = quantity
            .checked_add(item.quantity)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(SaleError::InvalidInput)?;
    }
    let option_ids = input
        .tenders
        .iter()
        .map(|tender| {
            Uuid::parse_str(&tender.payment_option_id).map_err(|_| SaleError::InvalidInput)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if input.tenders.iter().any(|t| {
        t.amount_minor <= 0
            || t.amount_minor > MAX_SAFE_INTEGER
            || t.reference
                .as_ref()
                .is_some_and(|v| v.chars().count() > 160)
    }) {
        return Err(SaleError::InvalidInput);
    }

    let mut tx = pool.begin().await.map_err(|_| SaleError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    sqlx::query("SELECT set_config('pos.lease_id',$1,true)")
        .bind(lease_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?;
    if let Some(customer_id) = customer_id {
        let customer_is_active = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2 AND status='active')")
            .bind(tenant_id).bind(customer_id).fetch_one(&mut *tx).await.map_err(|_| SaleError::Database)?;
        if !customer_is_active {
            return Err(SaleError::NotFound);
        }
    }
    let customer_name_snapshot = if let Some(customer_id) = customer_id {
        sqlx::query_scalar::<_, String>(
            "SELECT name FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2",
        )
        .bind(tenant_id)
        .bind(customer_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?
    } else {
        None
    };
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(station_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?;

    let lease = sqlx::query_as::<_, (String, Vec<u8>, String, String)>(
        "SELECT l.activation_id::text, a.device_public_key, COALESCE(NULLIF(split_part(btrim($8), ' ', 1), ''), 'Cashier'), s.name
         FROM station_actor_leases l
         JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id AND k.station_id=l.station_id AND k.lease_id=l.id
         JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id AND a.station_id=l.station_id AND a.id=l.activation_id
         JOIN memberships m ON m.id=l.membership_id AND m.tenant_id=l.tenant_id
         JOIN stations s ON s.id=l.station_id AND s.tenant_id=l.tenant_id AND s.branch_id=l.branch_id
         JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
         WHERE l.id=$1::uuid AND l.tenant_id=$2::uuid AND l.branch_id=$3::uuid AND l.station_id=$4::uuid
           AND l.membership_id=$5::uuid AND l.identity_id=$6::uuid AND l.lease_token_hash=$7
           AND l.status='issued' AND l.expires_at>now() AND a.status='active' AND m.status='active'
           AND g.role_key='cashier' AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
           AND (g.station_id IS NULL OR g.station_id=l.station_id) LIMIT 1 FOR UPDATE OF l",
    ).bind(lease_id).bind(tenant_id).bind(branch_id).bind(station_id).bind(membership_id).bind(identity_id).bind(lease_token_hash).bind(cashier_display_name.unwrap_or("Cashier"))
     .fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)?.ok_or(SaleError::LeaseUnavailable)?;

    let challenge = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        "SELECT nonce,payload_hash FROM station_financial_proof_challenges
         WHERE id=$1::uuid AND tenant_id=$2::uuid AND branch_id=$3::uuid AND station_id=$4::uuid
           AND membership_id=$5::uuid AND identity_id=$6::uuid AND activation_id=$7::uuid AND lease_id=$8::uuid
           AND operation='sale.commit' AND idempotency_key=$9 AND consumed_at IS NULL AND expires_at>now()
         FOR UPDATE",
    ).bind(challenge_id).bind(tenant_id).bind(branch_id).bind(station_id).bind(membership_id).bind(identity_id).bind(&lease.0).bind(lease_id).bind(idempotency_key)
     .fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)?.ok_or(SaleError::InvalidProof)?;
    if challenge.1 != payload_hash {
        return Err(SaleError::InvalidProof);
    }
    let key: [u8; 32] = lease
        .1
        .as_slice()
        .try_into()
        .map_err(|_| SaleError::InvalidProof)?;
    let verifying_key = VerifyingKey::from_bytes(&key).map_err(|_| SaleError::InvalidProof)?;
    let nonce = encode_hex(&challenge.0);
    let message = format!("advanced-pos/financial-operation/v1\nPOST\n/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales\nsale.commit\n{tenant_id}\n{branch_id}\n{station_id}\n{lease_id}\n{membership_id}\n{}\n{challenge_id}\n{idempotency_key}\n{}\n{nonce}", lease.0, encode_hex(&payload_hash));
    verifying_key
        .verify(message.as_bytes(), &Signature::from_bytes(&proof))
        .map_err(|_| SaleError::InvalidProof)?;

    let shared_checkout_body = if held_ticket_id.is_some() {
        Some(
            serde_json::from_slice::<serde_json::Value>(raw_body)
                .map_err(|_| SaleError::InvalidInput)?,
        )
    } else {
        None
    };
    let shared_checkout_fingerprint = shared_checkout_body
        .as_ref()
        .map(|body| serde_json::to_vec(body).map(|bytes| Sha256::digest(bytes).to_vec()))
        .transpose()
        .map_err(|_| SaleError::InvalidInput)?;
    if let (Some(ticket_id), Some(body), Some(fingerprint)) = (
        held_ticket_id,
        shared_checkout_body.as_ref(),
        shared_checkout_fingerprint.as_ref(),
    ) {
        let lease_uuid = Uuid::parse_str(lease_id).map_err(|_| SaleError::LeaseUnavailable)?;
        let ticket_key = ticket_id.to_string();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&ticket_key)
            .execute(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?;
        let ticket_snapshot = sqlx::query_as::<_, (String, serde_json::Value, String, Vec<u8>, Option<Uuid>, Option<Uuid>)>(
            "SELECT status,checkout_body,checkout_key,checkout_fingerprint,claim_lease_id,completed_sale_id
             FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(ticket_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?
        .ok_or(SaleError::NotFound)?;
        if ticket_snapshot.1 != *body
            || ticket_snapshot.2 != idempotency_key
            || ticket_snapshot.3 != *fingerprint
        {
            return Err(SaleError::IdempotencyConflict);
        }
        if ticket_snapshot.0 == "completed" {
            let sale_id = ticket_snapshot.5.ok_or(SaleError::Database)?;
            let existing = sqlx::query_as::<_, CommittedSale>(
                "SELECT id::text,customer_id::text AS customer_id,receipt_number,
                        cashier_name_snapshot AS cashier_name,station_name_snapshot AS station_name,
                        currency_code,subtotal_minor,tax_minor,total_minor,on_account_minor,created_at::text,false AS replayed
                 FROM hosted_sales WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
                   AND id=$3 AND held_ticket_id=$4",
            )
            .bind(tenant_id)
            .bind(branch_id)
            .bind(sale_id)
            .bind(ticket_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?
            .ok_or(SaleError::Database)?;
            let consumed = sqlx::query("UPDATE station_financial_proof_challenges SET consumed_at=now() WHERE id=$1::uuid AND consumed_at IS NULL AND expires_at>now() AND payload_hash=$2")
                .bind(challenge_id).bind(&payload_hash).execute(&mut *tx).await.map_err(|_| SaleError::Database)?;
            if consumed.rows_affected() != 1 {
                return Err(SaleError::InvalidProof);
            }
            tx.commit().await.map_err(|_| SaleError::Database)?;
            return Ok(CommittedSale {
                replayed: true,
                ..existing
            });
        }
        let ticket = sqlx::query_as::<_, (String, serde_json::Value, String, Vec<u8>, Option<Uuid>, Option<Uuid>)>(
            "SELECT status,checkout_body,checkout_key,checkout_fingerprint,claim_lease_id,completed_sale_id
             FROM branch_held_tickets WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(ticket_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?
        .ok_or(SaleError::NotFound)?;
        if ticket.1 != *body || ticket.2 != idempotency_key || ticket.3 != *fingerprint {
            return Err(SaleError::IdempotencyConflict);
        }
        if ticket.0 != "checkout_pending" || ticket.4.as_ref() != Some(&lease_uuid) {
            return Err(SaleError::LeaseUnavailable);
        }
    }

    if let Some(existing) = sqlx::query_as::<_, CommittedSale>(
        "SELECT id::text,customer_id::text AS customer_id,receipt_number,cashier_name_snapshot AS cashier_name,station_name_snapshot AS station_name,
         currency_code,subtotal_minor,tax_minor,total_minor,on_account_minor,created_at::text,false AS replayed
         FROM hosted_sales WHERE tenant_id=$1::uuid AND station_id=$2::uuid AND cashier_membership_id=$3::uuid AND idempotency_key=$4",
    ).bind(tenant_id).bind(station_id).bind(membership_id).bind(idempotency_key).fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)? {
        if sqlx::query_scalar::<_, Vec<u8>>("SELECT request_hash FROM hosted_sales WHERE id=$1::uuid").bind(&existing.id).fetch_one(&mut *tx).await.map_err(|_| SaleError::Database)? != payload_hash { return Err(SaleError::IdempotencyConflict); }
        let consumed = sqlx::query("UPDATE station_financial_proof_challenges SET consumed_at=now() WHERE id=$1::uuid AND consumed_at IS NULL AND expires_at>now() AND payload_hash=$2")
            .bind(challenge_id).bind(&payload_hash).execute(&mut *tx).await.map_err(|_| SaleError::Database)?;
        if consumed.rows_affected() != 1 { return Err(SaleError::InvalidProof); }
        tx.commit().await.map_err(|_| SaleError::Database)?;
        return Ok(CommittedSale { replayed:true, ..existing });
    }

    let pickup_lines = if let Some(reservation_id) = pickup_reservation {
        sqlx::query_as::<_, (Uuid, i64)>(
            "SELECT product_id,quantity FROM pickup_reservation_lines
             WHERE tenant_id=$1::uuid AND reservation_id=$2::uuid ORDER BY product_id",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?
    } else {
        Vec::new()
    };
    let lock_products = quantities
        .keys()
        .copied()
        .chain(pickup_lines.iter().map(|(product_id, _)| *product_id));
    inventory_allocations::lock_products(
        &mut tx,
        tenant_id,
        branch_id,
        lock_products.map(|product_id| product_id.to_string()),
    )
    .await
    .map_err(|_| SaleError::Database)?;

    let (pickup_revision, pickup_quantities) = if let Some(reservation_id) = pickup_reservation {
        let reference = input
            .pickup_reservation
            .as_ref()
            .expect("parsed pickup reference");
        let reservation = sqlx::query_as::<_, (String, i64, bool, Option<Uuid>)>(
            "SELECT status,revision,expires_at>clock_timestamp(),customer_id FROM pickup_reservations
             WHERE tenant_id=$1::uuid AND id=$2::uuid AND pickup_branch_id=$3::uuid FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .bind(branch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?
        .ok_or(SaleError::PickupUnavailable)?;
        if reservation.0 != "ready"
            || reservation.1 != reference.expected_revision
            || !reservation.2
            || reservation
                .3
                .is_some_and(|reserved_customer| Some(reserved_customer) != customer_id)
            || pickup_lines.is_empty()
        {
            return Err(SaleError::PickupUnavailable);
        }
        let held = pickup_lines
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        if held.iter().any(|(product_id, held_quantity)| {
            quantities.get(product_id).copied().unwrap_or(0) < *held_quantity
        }) {
            return Err(SaleError::PickupUnavailable);
        }
        (Some(reservation.1), held)
    } else {
        (None, std::collections::BTreeMap::new())
    };

    let mut products = Vec::new();
    for (product_id, quantity) in &quantities {
        let row=sqlx::query_as::<_, (String,String,Option<String>,String,i64,i32,i64,Option<i64>)>(
            "SELECT p.id::text,p.sku,p.barcode,p.name,b.price_minor,b.tax_rate_bps,b.stock_quantity,b.cost_minor
             FROM branch_catalog_items b JOIN catalog_products p ON p.id=b.product_id AND p.tenant_id=b.tenant_id
             WHERE b.tenant_id=$1::uuid AND b.branch_id=$2::uuid AND b.product_id=$3 AND b.status='active' AND p.status='active' FOR UPDATE OF b",
        ).bind(tenant_id).bind(branch_id).bind(product_id).fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)?.ok_or(SaleError::NotFound)?;
        let committed = if let Some(reservation_id) = pickup_reservation {
            inventory_allocations::allocated_quantity_excluding_pickup(
                &mut tx,
                tenant_id,
                branch_id,
                &product_id.to_string(),
                reservation_id,
            )
            .await
        } else {
            inventory_allocations::allocated_quantity(
                &mut tx,
                tenant_id,
                branch_id,
                &product_id.to_string(),
                None,
            )
            .await
        }
        .map_err(|_| SaleError::Database)?;
        if !inventory_allocations::can_consume(row.6, committed, *quantity) {
            return Err(SaleError::InsufficientStock);
        }
        products.push((
            ProductRow {
                product_id: row.0,
                sku: row.1,
                barcode: row.2,
                name: row.3,
                price_minor: row.4,
                tax_rate_bps: row.5,
                cost_minor: row.7,
            },
            *quantity,
        ));
    }
    let mut subtotal = 0i64;
    let mut tax = 0i64;
    let mut total = 0i64;
    let mut line_values = Vec::new();
    for (product, quantity) in &products {
        let totals = calculate_sale_totals(
            &[PricedLine {
                quantity: *quantity,
                unit_price_minor: product.price_minor,
            }],
            0,
            i64::from(product.tax_rate_bps),
        )
        .map_err(|_| SaleError::InvalidInput)?;
        let allocation = allocate_sale_components(
            &[PricedLine {
                quantity: *quantity,
                unit_price_minor: product.price_minor,
            }],
            totals,
        )
        .map_err(|_| SaleError::InvalidInput)?[0];
        subtotal = subtotal
            .checked_add(allocation.line_subtotal_minor)
            .ok_or(SaleError::InvalidInput)?;
        tax = tax
            .checked_add(allocation.tax_minor)
            .ok_or(SaleError::InvalidInput)?;
        total = total
            .checked_add(totals.total_minor)
            .ok_or(SaleError::InvalidInput)?;
        line_values.push((product, quantity, totals, allocation));
    }
    if [subtotal, tax, total]
        .iter()
        .any(|v| *v < 0 || *v > MAX_SAFE_INTEGER)
    {
        return Err(SaleError::InvalidInput);
    }
    let tender_total = input
        .tenders
        .iter()
        .try_fold(0i64, |sum, t| sum.checked_add(t.amount_minor))
        .filter(|sum| *sum <= MAX_SAFE_INTEGER)
        .ok_or(SaleError::InvalidInput)?;
    if tender_total.checked_add(input.on_account_minor) != Some(total) {
        return Err(SaleError::InvalidInput);
    }

    let mut tenders = Vec::new();
    for (tender, option_id) in input.tenders.iter().zip(option_ids) {
        let option=sqlx::query_as::<_, (String,String,String,Option<String>,bool,String)>(
            "SELECT method_key,label,tender_kind,provider_name,requires_reference,recording_mode FROM branch_payment_options
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3 AND enabled=true",
        ).bind(tenant_id).bind(branch_id).bind(option_id).fetch_optional(&mut *tx).await.map_err(|_|SaleError::Database)?.ok_or(SaleError::TenderUnavailable)?;
        if option.5 != "manual"
            || (option.4 && tender.reference.as_deref().is_none_or(str::is_empty))
        {
            return Err(SaleError::TenderUnavailable);
        }
        tenders.push((option_id, option, tender));
    }
    let cash_shift_id = if tenders.iter().any(|(_, option, _)| option.2 == "cash") {
        Some(sqlx::query_scalar::<_, String>(
            "SELECT id::text FROM hosted_drawer_shifts WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND station_id=$3::uuid AND status='open' FOR UPDATE",
        ).bind(tenant_id).bind(branch_id).bind(station_id).fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)?.ok_or(SaleError::DrawerRequired)?)
    } else {
        None
    };
    let cash_location_id = if cash_shift_id.is_some() {
        let configured = sqlx::query_as::<_, (Uuid, bool)>(
            "SELECT l.id,EXISTS(SELECT 1 FROM hosted_cash_location_events e WHERE e.tenant_id=l.tenant_id AND e.location_id=l.id AND e.event_type='opening')
             FROM hosted_cash_locations l WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.station_id=$3::uuid AND l.location_type='register' AND l.status='active'",
        )
        .bind(tenant_id).bind(branch_id).bind(station_id)
        .fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)?;
        match configured {
            Some((_, false)) => return Err(SaleError::RegisterCutoverRequired),
            Some((id, true)) => Some(id),
            None => None,
        }
    } else {
        None
    };
    let receipt_number = format!(
        "{}-{}",
        UtcDateStamp::today(),
        Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase()
    );
    let sale_id = Uuid::new_v4();
    let currency = sqlx::query_scalar::<_, String>(
        "SELECT currency_code::text FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| SaleError::Database)?
    .ok_or(SaleError::NotFound)?;
    let credit_terms = if input.on_account_minor > 0 {
        let customer = customer_id.ok_or(SaleError::InvalidInput)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!(
                "customer-credit-terms:{tenant_id}:{customer}:{}",
                currency.trim()
            ))
            .execute(&mut *tx)
            .await
            .map_err(|_| SaleError::Database)?;
        let position = sqlx::query_as::<_, crate::receivables::CustomerCreditPosition>(
            "SELECT currency_code,has_credit_terms,terms_revision,credit_limit_minor,due_days,outstanding_minor,available_minor FROM pos_worker.customer_credit_position($1::uuid)"
        ).bind(customer).fetch_optional(&mut *tx).await.map_err(|_| SaleError::Database)?.ok_or(SaleError::NotFound)?;
        let available = position
            .available_minor
            .parse::<i64>()
            .map_err(|_| SaleError::Database)?;
        if !position.has_credit_terms
            || position.currency_code.trim() != currency.trim()
            || input.on_account_minor > available
            || position.terms_revision.is_none()
            || position
                .credit_limit_minor
                .parse::<i64>()
                .ok()
                .filter(|limit| *limit > 0)
                .is_none()
        {
            return Err(SaleError::TenderUnavailable);
        }
        Some((
            position.terms_revision.unwrap(),
            position.due_days,
            position
                .credit_limit_minor
                .parse::<i64>()
                .map_err(|_| SaleError::Database)?,
        ))
    } else {
        None
    };
    let created=sqlx::query_scalar::<_,String>(
        "INSERT INTO hosted_sales(id,tenant_id,branch_id,station_id,cashier_membership_id,lease_id,activation_id,receipt_number,cashier_name_snapshot,station_name_snapshot,currency_code,subtotal_minor,tax_minor,total_minor,on_account_minor,request_hash,idempotency_key,cash_shift_id,customer_id,customer_name_snapshot,held_ticket_id)
         VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7::uuid,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18::uuid,$19::uuid,$20,$21::uuid)
         RETURNING created_at::text",
    ).bind(sale_id).bind(tenant_id).bind(branch_id).bind(station_id).bind(membership_id).bind(lease_id).bind(&lease.0).bind(&receipt_number).bind(&lease.2).bind(&lease.3).bind(currency.trim()).bind(subtotal).bind(tax).bind(total).bind(input.on_account_minor).bind(&payload_hash).bind(idempotency_key).bind(&cash_shift_id).bind(customer_id).bind(&customer_name_snapshot).bind(held_ticket_id)
     .fetch_one(&mut *tx).await.map_err(|e| if e.as_database_error().is_some_and(|d|d.is_unique_violation()){SaleError::IdempotencyConflict}else{SaleError::Database})?;
    if let Some((revision, due_days, credit_limit)) = credit_terms {
        sqlx::query("INSERT INTO hosted_receivable_invoices(tenant_id,branch_id,sale_id,customer_id,currency_code,original_amount_minor,credit_term_revision,due_days_snapshot,credit_limit_minor_snapshot,issued_at,due_at,issued_by_membership_id) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10::timestamptz,$10::timestamptz + ($8 * interval '1 day'),$11::uuid)")
            .bind(tenant_id).bind(branch_id).bind(sale_id).bind(customer_id.ok_or(SaleError::InvalidInput)?).bind(currency.trim()).bind(input.on_account_minor).bind(revision).bind(due_days).bind(credit_limit).bind(&created).bind(membership_id).execute(&mut *tx).await.map_err(|_| SaleError::Database)?;
    }
    // Release the committed hold before decrementing on-hand stock. The sale,
    // collection event and allocation release remain in this transaction, so
    // any later failure restores the hold along with the rest of checkout.
    if let (Some(reservation_id), Some(previous_revision)) = (pickup_reservation, pickup_revision) {
        let collected = sqlx::query(
            "UPDATE pickup_reservations SET status='collected',revision=revision+1,sale_id=$4,updated_at=clock_timestamp()
             WHERE tenant_id=$1::uuid AND id=$2::uuid AND pickup_branch_id=$3::uuid
               AND status='ready' AND revision=$5",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .bind(branch_id)
        .bind(sale_id)
        .bind(previous_revision)
        .execute(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?;
        if collected.rows_affected() != 1 {
            return Err(SaleError::PickupUnavailable);
        }
        let next_revision = previous_revision + 1;
        sqlx::query(
            "INSERT INTO pickup_reservation_events
             (tenant_id,reservation_id,acting_branch_id,acting_station_id,actor_membership_id,event_type,revision,idempotency_key,request_hash)
             VALUES($1::uuid,$2,$3::uuid,$4::uuid,$5::uuid,'collected',$6,$7,$8)",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .bind(branch_id)
        .bind(station_id)
        .bind(membership_id)
        .bind(next_revision)
        .bind(idempotency_key)
        .bind(&payload_hash)
        .execute(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?;
        let released = sqlx::query(
            "UPDATE inventory_allocations SET released_at=clock_timestamp(),release_reason='collected'
             WHERE tenant_id=$1::uuid AND pickup_reservation_id=$2::uuid AND branch_id=$3::uuid AND released_at IS NULL",
        )
        .bind(tenant_id)
        .bind(reservation_id)
        .bind(branch_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?;
        if released.rows_affected() as usize != pickup_quantities.len() {
            return Err(SaleError::PickupUnavailable);
        }
        sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'pickup_reservation_collected','pickup_reservation',$3,jsonb_build_object('saleId',$4::uuid,'revision',$5))")
            .bind(tenant_id).bind(membership_id).bind(reservation_id).bind(sale_id).bind(next_revision)
            .execute(&mut *tx).await.map_err(|_| SaleError::Database)?;
    }
    for (index, (product, quantity, totals, allocation)) in line_values.iter().enumerate() {
        sqlx::query("INSERT INTO hosted_sale_lines(tenant_id,branch_id,sale_id,line_number,product_id,sku_snapshot,barcode_snapshot,name_snapshot,quantity,unit_price_minor,tax_rate_bps,subtotal_minor,tax_minor,total_minor,unit_cost_minor_snapshot) VALUES($1::uuid,$2::uuid,$3,$4,$5::uuid,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
            .bind(tenant_id).bind(branch_id).bind(sale_id).bind((index+1) as i32).bind(&product.product_id).bind(&product.sku).bind(&product.barcode).bind(&product.name).bind(*quantity).bind(product.price_minor).bind(product.tax_rate_bps).bind(allocation.line_subtotal_minor).bind(allocation.tax_minor).bind(totals.total_minor).bind(product.cost_minor).execute(&mut *tx).await.map_err(|_|SaleError::Database)?;
        let updated_stock=sqlx::query_scalar::<_,i64>("UPDATE branch_catalog_items SET stock_quantity=stock_quantity-$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND stock_quantity >= $4 RETURNING stock_quantity")
            .bind(tenant_id).bind(branch_id).bind(&product.product_id).bind(*quantity).fetch_optional(&mut *tx).await.map_err(|_|SaleError::Database)?.ok_or(SaleError::InsufficientStock)?;
        sqlx::query("INSERT INTO stock_movements(tenant_id,branch_id,product_id,actor_membership_id,movement_kind,quantity_delta,note,source_id,resulting_stock_quantity) VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,'sale',$5,'Hosted POS sale',$6,$7)")
            .bind(tenant_id).bind(branch_id).bind(&product.product_id).bind(membership_id).bind(-*quantity).bind(sale_id).bind(updated_stock).execute(&mut *tx).await.map_err(|_|SaleError::Database)?;
    }
    for (option_id, option, tender) in tenders {
        let is_cash = option.2 == "cash";
        let tender_id = sqlx::query_scalar::<_, Uuid>("INSERT INTO hosted_sale_tenders(tenant_id,branch_id,sale_id,payment_option_id,method_key_snapshot,label_snapshot,tender_kind,provider_name_snapshot,amount_minor,reference) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id")
            .bind(tenant_id).bind(branch_id).bind(sale_id).bind(option_id).bind(option.0).bind(option.1).bind(option.2).bind(option.3).bind(tender.amount_minor).bind(&tender.reference).fetch_one(&mut *tx).await.map_err(|_|SaleError::Database)?;
        if is_cash {
            if let Some(location_id) = cash_location_id {
                sqlx::query("INSERT INTO hosted_cash_location_events(tenant_id,branch_id,location_id,event_type,source_sale_tender_id,signed_amount_minor,actor_membership_id) VALUES($1::uuid,$2::uuid,$3,'sale_tender_inflow',$4,$5,$6::uuid)")
                    .bind(tenant_id).bind(branch_id).bind(location_id).bind(tender_id).bind(tender.amount_minor).bind(membership_id).execute(&mut *tx).await.map_err(|_| SaleError::Database)?;
            }
        }
    }
    crate::sale_accounting::post_sale(&mut tx, tenant_id, branch_id, membership_id, sale_id)
        .await
        .map_err(|error| {
            #[cfg(test)]
            eprintln!("hosted sale accounting journal failed: {error}");
            #[cfg(not(test))]
            let _ = error;
            SaleError::Database
        })?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'sale_completed','sale',$3,jsonb_build_object('receiptNumber',$4,'totalMinor',$5,'stationId',$6::uuid))")
        .bind(tenant_id).bind(membership_id).bind(sale_id.to_string()).bind(&receipt_number).bind(total).bind(station_id).execute(&mut *tx).await.map_err(|_|SaleError::Database)?;
    let consumed=sqlx::query("UPDATE station_financial_proof_challenges SET consumed_at=now() WHERE id=$1::uuid AND consumed_at IS NULL AND expires_at>now() AND payload_hash=$2").bind(challenge_id).bind(&payload_hash).execute(&mut *tx).await.map_err(|_|SaleError::Database)?;
    if consumed.rows_affected() != 1 {
        return Err(SaleError::InvalidProof);
    }
    if let (Some(ticket_id), Some(fingerprint)) =
        (held_ticket_id, shared_checkout_fingerprint.as_ref())
    {
        let completed = sqlx::query(
            "UPDATE branch_held_tickets SET status='completed',completed_sale_id=$4::uuid,
                    claim_lease_id=NULL,claim_expires_at=NULL,updated_by=$5::uuid,
                    revision=revision+1,updated_at=clock_timestamp()
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3
               AND status='checkout_pending' AND claim_lease_id=$6::uuid
               AND checkout_key=$7 AND checkout_fingerprint=$8",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(ticket_id)
        .bind(sale_id)
        .bind(membership_id)
        .bind(lease_id)
        .bind(idempotency_key)
        .bind(fingerprint)
        .execute(&mut *tx)
        .await
        .map_err(|_| SaleError::Database)?;
        if completed.rows_affected() != 1 {
            return Err(SaleError::IdempotencyConflict);
        }
    }
    tx.commit().await.map_err(|_| SaleError::Database)?;
    Ok(CommittedSale {
        id: sale_id.to_string(),
        customer_id: customer_id.map(|value| value.to_string()),
        receipt_number,
        cashier_name: lease.2,
        station_name: lease.3,
        currency_code: currency.trim().to_owned(),
        subtotal_minor: subtotal,
        tax_minor: tax,
        total_minor: total,
        on_account_minor: input.on_account_minor,
        created_at: created,
        replayed: false,
    })
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    station: &str,
) -> Result<(), SaleError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", branch),
        ("pos.station_id", station),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| SaleError::Database)?;
    }
    Ok(())
}
fn valid_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}
fn decode_hex(value: &str, len: usize) -> Option<Vec<u8>> {
    if value.len() != len * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|p| Some(((p[0] as char).to_digit(16)? * 16 + (p[1] as char).to_digit(16)?) as u8))
        .collect()
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
struct UtcDateStamp;
impl UtcDateStamp {
    fn today() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let days = secs / 86400;
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let mut y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        y += i64::from(m <= 2);
        format!("{y:04}{m:02}{d:02}")
    }
}
