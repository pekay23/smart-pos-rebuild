// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{commands::active_session, db::AppState, models::SaleSummary};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params_from_iter, types::Value, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesQueryInput {
    #[serde(default)]
    pub from_utc: Option<String>,
    #[serde(default)]
    pub to_utc_exclusive: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub station_key: Option<String>,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub page_size: u16,
    #[serde(default)]
    pub snapshot_row_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesPaymentSummary {
    pub method: String,
    pub sale_count: i64,
    pub total_minor: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesQueryResult {
    pub rows: Vec<SaleSummary>,
    pub offset: u32,
    pub page_size: u16,
    pub matched_count: i64,
    pub completed_count: i64,
    pub completed_subtotal_minor: i64,
    pub completed_discount_minor: i64,
    pub completed_tax_minor: i64,
    pub completed_total_minor: i64,
    pub payment_summary: Vec<SalesPaymentSummary>,
    pub snapshot_row_id: i64,
}

struct SalesFilter {
    where_sql: String,
    values: Vec<Value>,
    snapshot_row_id: i64,
}

fn ensure_javascript_safe_integer(value: i64) -> Result<(), String> {
    if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value) {
        return Err("A report value exceeds the safe number range in this client.".to_owned());
    }
    Ok(())
}

pub(crate) fn canonical_timestamp(value: &str) -> Result<String, String> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| "Use a valid date and time for the report range.".to_owned())?;
    Ok(parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitReportInput {
    pub from_utc: String,
    pub to_utc_exclusive: String,
    #[serde(default)]
    pub station_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitReport {
    pub station_key: Option<String>,
    pub sale_count: i64,
    pub unassigned_sales_count: i64,
    pub unassigned_sales_revenue_minor: i64,
    pub net_revenue_before_tax_minor: i64,
    pub tax_collected_minor: i64,
    pub customer_total_minor: i64,
    pub cost_of_goods_minor: i64,
    pub costed_units: i64,
    pub uncosted_units: i64,
    pub direct_expenses_minor: i64,
    pub store_overhead_expenses_minor: i64,
    pub legacy_unassigned_expenses_minor: i64,
    pub operating_expenses_minor: i64,
    pub gross_profit_minor: Option<i64>,
    pub station_contribution_minor: Option<i64>,
    pub net_operating_profit_minor: Option<i64>,
}

fn run_profit_report(
    connection: &Connection,
    store_id: &str,
    input: &ProfitReportInput,
) -> Result<ProfitReport, String> {
    let from = canonical_timestamp(&input.from_utc)?;
    let to = canonical_timestamp(&input.to_utc_exclusive)?;
    if from >= to {
        return Err("The report start must be earlier than its end.".to_owned());
    }
    let station_key = input
        .station_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if station_key.is_some_and(|value| value.len() > 80) {
        return Err("Choose a valid register station.".to_owned());
    }
    if let Some(station_key) = station_key {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM register_stations WHERE store_id=?1 AND station_key=?2)",
                rusqlite::params![store_id,station_key],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !exists {
            return Err("Choose a register station in this store.".to_owned());
        }
    }
    let (sale_count, sold_revenue, sold_tax, sold_total) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(subtotal_minor-discount_minor),0),
                COALESCE(SUM(tax_minor),0), COALESCE(SUM(total_minor),0)
         FROM sales WHERE store_id=?1 AND created_at>=?2 AND created_at<?3
           AND (?4 IS NULL OR station_key=?4)",
            rusqlite::params![store_id, from, to, station_key],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let (refunded_revenue, refunded_tax, refunded_total) = connection
        .query_row(
            "SELECT COALESCE(SUM(ri.subtotal_minor-ri.discount_minor),0),
                COALESCE(SUM(ri.tax_minor),0), COALESCE(SUM(ri.amount_minor),0)
         FROM refunds r JOIN refund_items ri ON ri.refund_id=r.id
         JOIN sales s ON s.id=r.sale_id AND s.store_id=r.store_id
         WHERE r.store_id=?1 AND r.created_at>=?2 AND r.created_at<?3
           AND (?4 IS NULL OR s.station_key=?4)",
            rusqlite::params![store_id, from, to, station_key],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let net_revenue = sold_revenue - refunded_revenue;
    let tax = sold_tax - refunded_tax;
    let customer_total = sold_total - refunded_total;

    let (unassigned_sales_count, unassigned_sales_revenue_minor): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(s.subtotal_minor-s.discount_minor),0)
             -COALESCE((SELECT SUM(ri.subtotal_minor-ri.discount_minor)
                        FROM refunds r JOIN refund_items ri ON ri.refund_id=r.id
                        JOIN sales origin ON origin.id=r.sale_id AND origin.store_id=r.store_id
                        WHERE r.store_id=?1 AND origin.station_key IS NULL
                          AND r.created_at>=?2 AND r.created_at<?3),0)
             FROM sales s WHERE s.store_id=?1 AND s.station_key IS NULL
               AND s.created_at>=?2 AND s.created_at<?3",
            rusqlite::params![store_id, from, to],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;

    let (sold_cost, sold_costed_units, sold_uncosted_units) = connection.query_row(
        "SELECT COALESCE(SUM(CASE WHEN si.unit_cost_minor_snapshot IS NOT NULL THEN si.quantity*si.unit_cost_minor_snapshot ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN si.unit_cost_minor_snapshot IS NOT NULL THEN si.quantity ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN si.unit_cost_minor_snapshot IS NULL THEN si.quantity ELSE 0 END),0)
         FROM sales s JOIN sale_items si ON si.sale_id=s.id
         WHERE s.store_id=?1 AND s.created_at>=?2 AND s.created_at<?3
           AND (?4 IS NULL OR s.station_key=?4)",
        rusqlite::params![store_id, from, to, station_key],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
    ).map_err(|error| error.to_string())?;
    let (returned_cost, returned_costed_units, returned_uncosted_units) = connection.query_row(
        "SELECT COALESCE(SUM(CASE WHEN si.unit_cost_minor_snapshot IS NOT NULL THEN ri.quantity*si.unit_cost_minor_snapshot ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN si.unit_cost_minor_snapshot IS NOT NULL THEN ri.quantity ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN si.unit_cost_minor_snapshot IS NULL THEN ri.quantity ELSE 0 END),0)
         FROM refunds r JOIN refund_items ri ON ri.refund_id=r.id
         JOIN sale_items si ON si.id=ri.sale_item_id JOIN sales s ON s.id=r.sale_id
         WHERE r.store_id=?1 AND s.store_id=?1 AND r.created_at>=?2 AND r.created_at<?3
           AND (?4 IS NULL OR s.station_key=?4)",
        rusqlite::params![store_id, from, to, station_key],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
    ).map_err(|error| error.to_string())?;
    let cost_of_goods = sold_cost - returned_cost;
    let costed_units = sold_costed_units + returned_costed_units;
    let uncosted_units = sold_uncosted_units + returned_uncosted_units;
    let (direct_expenses, store_overhead, legacy_unassigned): (i64, i64, i64) = connection
        .query_row(
            "WITH attributed_expenses AS (
                SELECT e.id,e.store_id,e.amount_minor,e.occurred_at,
                       COALESCE(r.next_kind,e.cost_center_kind) AS cost_center_kind,
                       CASE WHEN r.id IS NULL THEN e.cost_center_station_key ELSE r.next_station_key END AS cost_center_station_key
                FROM expenses e LEFT JOIN expense_cost_center_reclassifications r
                  ON r.store_id=e.store_id AND r.expense_id=e.id
                 AND r.revision=(SELECT MAX(latest.revision) FROM expense_cost_center_reclassifications latest
                                 WHERE latest.store_id=e.store_id AND latest.expense_id=e.id)
                WHERE e.store_id=?1
             ), expense_events AS (
                SELECT cost_center_kind,cost_center_station_key,amount_minor AS signed_amount,
                       occurred_at AS effective_at FROM attributed_expenses
                UNION ALL
                SELECT e.cost_center_kind,e.cost_center_station_key,-r.amount_minor AS signed_amount,
                       r.returned_at AS effective_at
                FROM expense_reimbursements r JOIN attributed_expenses e ON e.id=r.expense_id AND e.store_id=r.store_id
                WHERE r.store_id=?1
             )
             SELECT COALESCE(SUM(CASE WHEN cost_center_kind='station_direct'
                                           AND (?4 IS NULL OR cost_center_station_key=?4)
                                      THEN signed_amount ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN cost_center_kind='store_overhead'
                                      THEN signed_amount ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN cost_center_kind='legacy_unassigned'
                                      THEN signed_amount ELSE 0 END),0)
             FROM expense_events WHERE effective_at>=?2 AND effective_at<?3",
            rusqlite::params![store_id, from, to, station_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| error.to_string())?;
    let operating_expenses = if station_key.is_some() {
        direct_expenses
    } else {
        direct_expenses + store_overhead + legacy_unassigned
    };
    for value in [
        sale_count,
        unassigned_sales_count,
        unassigned_sales_revenue_minor,
        net_revenue,
        tax,
        customer_total,
        cost_of_goods,
        costed_units,
        uncosted_units,
        direct_expenses,
        store_overhead,
        legacy_unassigned,
        operating_expenses,
    ] {
        ensure_javascript_safe_integer(value)?;
    }
    let gross_profit = (uncosted_units == 0).then(|| net_revenue - cost_of_goods);
    let station_contribution =
        station_key.and_then(|_| gross_profit.map(|gross| gross - direct_expenses));
    let operating_profit = station_key
        .is_none()
        .then(|| gross_profit.map(|gross| gross - operating_expenses))
        .flatten();
    if let Some(value) = gross_profit {
        ensure_javascript_safe_integer(value)?;
    }
    if let Some(value) = operating_profit {
        ensure_javascript_safe_integer(value)?;
    }
    if let Some(value) = station_contribution {
        ensure_javascript_safe_integer(value)?;
    }
    Ok(ProfitReport {
        station_key: station_key.map(str::to_owned),
        sale_count,
        unassigned_sales_count,
        unassigned_sales_revenue_minor,
        net_revenue_before_tax_minor: net_revenue,
        tax_collected_minor: tax,
        customer_total_minor: customer_total,
        cost_of_goods_minor: cost_of_goods,
        costed_units,
        uncosted_units,
        direct_expenses_minor: direct_expenses,
        store_overhead_expenses_minor: store_overhead,
        legacy_unassigned_expenses_minor: legacy_unassigned,
        operating_expenses_minor: operating_expenses,
        gross_profit_minor: gross_profit,
        station_contribution_minor: station_contribution,
        net_operating_profit_minor: operating_profit,
    })
}

#[tauri::command]
pub fn query_profit_report(
    input: ProfitReportInput,
    state: State<'_, AppState>,
) -> Result<ProfitReport, String> {
    let session = active_session(&state, "reports:read")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.")?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| error.to_string())?;
    let report = run_profit_report(&transaction, &session.store_id, &input)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(report)
}

fn build_filter(
    store_id: &str,
    input: &SalesQueryInput,
    snapshot_row_id: i64,
) -> Result<SalesFilter, String> {
    let bounds = match (&input.from_utc, &input.to_utc_exclusive) {
        (None, None) => None,
        (Some(from), Some(to)) => {
            let from = canonical_timestamp(from)?;
            let to = canonical_timestamp(to)?;
            if from >= to {
                return Err("The report start must be earlier than its end.".to_owned());
            }
            Some((from, to))
        }
        _ => return Err("Choose both a start date and an end date.".to_owned()),
    };

    let search = input.search.as_deref().unwrap_or_default().trim();
    if search.chars().count() > 100 {
        return Err("Sales search is limited to 100 characters.".to_owned());
    }

    let mut values = vec![
        Value::Text(store_id.to_owned()),
        Value::Integer(snapshot_row_id),
    ];
    let mut conditions = vec!["s.store_id = ?1".to_owned(), "s.rowid <= ?2".to_owned()];
    if let Some((from, to)) = bounds {
        values.push(Value::Text(from));
        conditions.push(format!("s.created_at >= ?{}", values.len()));
        values.push(Value::Text(to));
        conditions.push(format!("s.created_at < ?{}", values.len()));
    }
    if !search.is_empty() {
        values.push(Value::Text(search.to_owned()));
        let needle = values.len();
        conditions.push(format!(
            "(instr(lower(s.receipt_number), lower(?{needle})) > 0
             OR instr(lower(u.full_name), lower(?{needle})) > 0
             OR EXISTS (
                 SELECT 1 FROM payments ps
                 WHERE ps.sale_id = s.id AND instr(lower(ps.method), lower(?{needle})) > 0
             ))"
        ));
    }
    if let Some(station_key) = input.station_key.as_deref() {
        let station_key = station_key.trim();
        if station_key.is_empty() || station_key.len() > 80 {
            return Err("Choose a valid register station.".to_owned());
        }
        values.push(Value::Text(station_key.to_owned()));
        conditions.push(format!("s.station_key = ?{}", values.len()));
    }
    Ok(SalesFilter {
        where_sql: conditions.join(" AND "),
        values,
        snapshot_row_id,
    })
}

fn run_query(
    connection: &Connection,
    store_id: &str,
    input: &SalesQueryInput,
) -> Result<SalesQueryResult, String> {
    let current_high_water = connection
        .query_row(
            "SELECT COALESCE(MAX(rowid), 0) FROM sales WHERE store_id = ?1",
            [store_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    let snapshot_row_id = match input.snapshot_row_id {
        Some(value) if value < 0 => {
            return Err("The sales snapshot is invalid. Refresh the report.".to_owned());
        }
        Some(value) => value.min(current_high_water),
        None => current_high_water,
    };
    let filter = build_filter(store_id, input, snapshot_row_id)?;
    let offset = input.offset;
    let page_size = match input.page_size {
        0 => 50,
        requested => requested.min(100),
    };

    let aggregate_sql = format!(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN s.status IN ('completed','partially_refunded') THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN s.status IN ('completed','partially_refunded') THEN
                    s.subtotal_minor - COALESCE((SELECT SUM(ri.subtotal_minor) FROM refund_items ri JOIN refunds r ON r.id=ri.refund_id WHERE r.sale_id=s.id AND r.store_id=s.store_id),0)
                    ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN s.status IN ('completed','partially_refunded') THEN
                    s.discount_minor - COALESCE((SELECT SUM(ri.discount_minor) FROM refund_items ri JOIN refunds r ON r.id=ri.refund_id WHERE r.sale_id=s.id AND r.store_id=s.store_id),0)
                    ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN s.status IN ('completed','partially_refunded') THEN
                    s.tax_minor - COALESCE((SELECT SUM(ri.tax_minor) FROM refund_items ri JOIN refunds r ON r.id=ri.refund_id WHERE r.sale_id=s.id AND r.store_id=s.store_id),0)
                    ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN s.status IN ('completed','partially_refunded') THEN
                    s.total_minor - COALESCE((SELECT SUM(r.total_minor) FROM refunds r WHERE r.sale_id=s.id AND r.store_id=s.store_id),0)
                    ELSE 0 END), 0)
         FROM sales s JOIN users u ON u.id = s.cashier_id
         WHERE {}",
        filter.where_sql
    );
    let (
        matched_count,
        completed_count,
        completed_subtotal_minor,
        completed_discount_minor,
        completed_tax_minor,
        completed_total_minor,
    ) = connection
        .query_row(
            &aggregate_sql,
            params_from_iter(filter.values.iter()),
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    for value in [
        matched_count,
        completed_count,
        completed_subtotal_minor,
        completed_discount_minor,
        completed_tax_minor,
        completed_total_minor,
    ] {
        ensure_javascript_safe_integer(value)?;
    }

    let payment_sql = format!(
        "SELECT method, SUM(sale_count), SUM(amount_minor) FROM (
            SELECT p.method AS method, COUNT(DISTINCT s.id) AS sale_count, SUM(p.amount_minor) AS amount_minor
            FROM sales s JOIN users u ON u.id=s.cashier_id JOIN payments p ON p.sale_id=s.id
            WHERE {} GROUP BY p.method
            UNION ALL
            SELECT t.method AS method, 0 AS sale_count, -SUM(t.amount_minor) AS amount_minor
            FROM sales s JOIN users u ON u.id=s.cashier_id JOIN refunds r ON r.sale_id=s.id
                JOIN refund_tenders t ON t.refund_id=r.id
            WHERE {} GROUP BY t.method
         ) GROUP BY method HAVING SUM(amount_minor) <> 0 OR SUM(sale_count) > 0
         ORDER BY SUM(amount_minor) DESC, method ASC",
        filter.where_sql, filter.where_sql
    );
    let mut payment_statement = connection
        .prepare(&payment_sql)
        .map_err(|error| error.to_string())?;
    let payment_summary = payment_statement
        .query_map(params_from_iter(filter.values.iter()), |row| {
            Ok(SalesPaymentSummary {
                method: row.get(0)?,
                sale_count: row.get(1)?,
                total_minor: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for payment in &payment_summary {
        ensure_javascript_safe_integer(payment.sale_count)?;
        ensure_javascript_safe_integer(payment.total_minor)?;
    }

    let limit_parameter = filter.values.len() + 1;
    let offset_parameter = limit_parameter + 1;
    let row_sql = format!(
        "SELECT s.id, s.receipt_number, s.created_at, u.full_name,
                COALESCE((SELECT SUM(si.quantity) FROM sale_items si WHERE si.sale_id = s.id), 0),
                CASE WHEN s.status IN ('refunded','voided') THEN 0 ELSE
                    s.total_minor - COALESCE((SELECT SUM(r.total_minor) FROM refunds r WHERE r.sale_id=s.id AND r.store_id=s.store_id),0)
                END, s.status,
                COALESCE((
                    SELECT CASE WHEN COUNT(*) > 1 THEN 'multiple' ELSE MAX(p.method) END
                    FROM payments p WHERE p.sale_id = s.id
                ), 'other'),
                s.station_key, COALESCE(s.station_name_snapshot,'Main register')
         FROM sales s JOIN users u ON u.id = s.cashier_id
         WHERE {}
         ORDER BY s.created_at DESC, s.id DESC
         LIMIT ?{} OFFSET ?{}",
        filter.where_sql, limit_parameter, offset_parameter
    );
    let mut row_values = filter.values;
    row_values.push(Value::Integer(i64::from(page_size)));
    row_values.push(Value::Integer(i64::from(offset)));
    let mut statement = connection
        .prepare(&row_sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params_from_iter(row_values.iter()), |row| {
            Ok(SaleSummary {
                id: row.get(0)?,
                receipt_number: row.get(1)?,
                created_at: row.get(2)?,
                cashier_name: row.get(3)?,
                item_count: row.get(4)?,
                total_minor: row.get(5)?,
                status: row.get(6)?,
                payment_method: row.get(7)?,
                station_key: row.get(8)?,
                station_name_snapshot: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for sale in &rows {
        ensure_javascript_safe_integer(sale.item_count)?;
        ensure_javascript_safe_integer(sale.total_minor)?;
    }

    Ok(SalesQueryResult {
        rows,
        offset,
        page_size,
        matched_count,
        completed_count,
        completed_subtotal_minor,
        completed_discount_minor,
        completed_tax_minor,
        completed_total_minor,
        payment_summary,
        snapshot_row_id: filter.snapshot_row_id,
    })
}

#[tauri::command]
pub fn query_sales(
    input: SalesQueryInput,
    state: State<'_, AppState>,
) -> Result<SalesQueryResult, String> {
    let session = active_session(&state, "sales:read")?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| error.to_string())?;
    let result = run_query(&transaction, &session.store_id, &input)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_uses_store_scope_half_open_dates_and_deterministic_pages() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE users(id TEXT PRIMARY KEY, full_name TEXT NOT NULL);
             CREATE TABLE sales(id TEXT PRIMARY KEY, store_id TEXT NOT NULL, cashier_id TEXT NOT NULL,
                 receipt_number TEXT NOT NULL, created_at TEXT NOT NULL, subtotal_minor INTEGER NOT NULL,
                 discount_minor INTEGER NOT NULL, tax_minor INTEGER NOT NULL, total_minor INTEGER NOT NULL, status TEXT NOT NULL,
                 station_key TEXT, station_name_snapshot TEXT);
             CREATE TABLE sale_items(sale_id TEXT NOT NULL, quantity INTEGER NOT NULL);
             CREATE TABLE payments(sale_id TEXT NOT NULL, method TEXT NOT NULL, amount_minor INTEGER NOT NULL);
             CREATE TABLE refunds(id TEXT, sale_id TEXT, total_minor INTEGER, store_id TEXT);
             CREATE TABLE refund_items(refund_id TEXT, subtotal_minor INTEGER, discount_minor INTEGER, tax_minor INTEGER);
             CREATE TABLE refund_tenders(refund_id TEXT, method TEXT, amount_minor INTEGER);
             CREATE TABLE expense_reimbursements(id TEXT, store_id TEXT, amount_minor INTEGER, returned_at TEXT);
             INSERT INTO users VALUES('cashier','Demo Cashier');",
        ).unwrap();
        for index in 0..260 {
            let id = format!("sale-{index:03}");
            let created = if index == 259 {
                "2026-10-30T00:00:00.000Z".to_owned()
            } else {
                "2026-10-29T12:00:00.000Z".to_owned()
            };
            connection
                .execute(
                "INSERT INTO sales(id,store_id,cashier_id,receipt_number,created_at,subtotal_minor,discount_minor,tax_minor,total_minor,status) VALUES(?1,'store-a','cashier',?2,?3,100,0,0,100,'completed')",
                    rusqlite::params![id, format!("R-{index:03}"), created],
                )
                .unwrap();
            connection
                .execute("INSERT INTO sale_items VALUES(?1,2)", [id.as_str()])
                .unwrap();
            connection
                .execute("INSERT INTO payments VALUES(?1,'cash',100)", [id.as_str()])
                .unwrap();
        }
        connection.execute(
            "INSERT INTO sales(id,store_id,cashier_id,receipt_number,created_at,subtotal_minor,discount_minor,tax_minor,total_minor,status) VALUES('other-store-sale','store-b','cashier','OTHER','2026-10-29T12:00:00.000Z',900,0,0,900,'completed')",
            [],
        ).unwrap();

        let first = run_query(
            &connection,
            "store-a",
            &SalesQueryInput {
                from_utc: Some("2026-10-29T00:00:00Z".into()),
                to_utc_exclusive: Some("2026-10-30T00:00:00Z".into()),
                search: None,
                station_key: None,
                offset: 0,
                page_size: 100,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        assert_eq!(first.matched_count, 259);
        assert_eq!(first.completed_count, 259);
        assert_eq!(first.completed_subtotal_minor, 25_900);
        assert_eq!(first.completed_discount_minor, 0);
        assert_eq!(first.completed_tax_minor, 0);
        assert_eq!(first.completed_total_minor, 25_900);
        assert_eq!(first.rows.len(), 100);
        assert_eq!(first.rows[0].id, "sale-258");
        assert_eq!(first.rows[0].item_count, 2);
        connection.execute(
            "INSERT INTO sales(id,store_id,cashier_id,receipt_number,created_at,subtotal_minor,discount_minor,tax_minor,total_minor,status) VALUES('late-sale','store-a','cashier','LATE','2026-10-29T12:00:00.000Z',100,0,0,100,'completed')",
            [],
        ).unwrap();
        connection
            .execute("INSERT INTO payments VALUES('late-sale','cash',100)", [])
            .unwrap();

        let second = run_query(
            &connection,
            "store-a",
            &SalesQueryInput {
                from_utc: Some("2026-10-29T00:00:00Z".into()),
                to_utc_exclusive: Some("2026-10-30T00:00:00Z".into()),
                search: None,
                station_key: None,
                offset: 100,
                page_size: 100,
                snapshot_row_id: Some(first.snapshot_row_id),
            },
        )
        .unwrap();
        assert_eq!(second.matched_count, 259);
        assert_eq!(second.rows.len(), 100);
        assert!(first
            .rows
            .iter()
            .all(|row| !second.rows.iter().any(|later| later.id == row.id)));
        assert_eq!(second.payment_summary[0].total_minor, 25_900);

        let literal_search = run_query(
            &connection,
            "store-a",
            &SalesQueryInput {
                from_utc: None,
                to_utc_exclusive: None,
                search: Some("R-001".into()),
                station_key: None,
                offset: 0,
                page_size: 50,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        assert_eq!(literal_search.matched_count, 1);
        assert_eq!(literal_search.rows[0].receipt_number, "R-001");

        connection.execute(
            "UPDATE sales SET station_key='front',station_name_snapshot='Front till' WHERE receipt_number='R-001'",
            [],
        ).unwrap();
        let station_search = run_query(
            &connection,
            "store-a",
            &SalesQueryInput {
                from_utc: None,
                to_utc_exclusive: None,
                search: None,
                station_key: Some("front".into()),
                offset: 0,
                page_size: 50,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        assert_eq!(station_search.matched_count, 1);
        assert_eq!(station_search.completed_count, 1);
        assert_eq!(station_search.completed_subtotal_minor, 100);
        assert_eq!(station_search.completed_total_minor, 100);
        assert_eq!(station_search.payment_summary.len(), 1);
        assert_eq!(station_search.payment_summary[0].total_minor, 100);
        assert_eq!(station_search.rows[0].station_key.as_deref(), Some("front"));
        assert_eq!(station_search.rows[0].station_name_snapshot, "Front till");
    }

    #[test]
    fn invalid_or_partial_ranges_fail_before_querying() {
        let input = SalesQueryInput {
            from_utc: Some("2026-10-30T00:00:00Z".into()),
            to_utc_exclusive: None,
            search: None,
            station_key: None,
            offset: 0,
            page_size: 50,
            snapshot_row_id: None,
        };
        assert!(build_filter("store-a", &input, 1).is_err());
    }

    #[test]
    fn report_values_are_checked_for_positive_and_negative_javascript_overflow() {
        assert!(ensure_javascript_safe_integer(9_007_199_254_740_991).is_ok());
        assert!(ensure_javascript_safe_integer(-9_007_199_254_740_991).is_ok());
        assert!(ensure_javascript_safe_integer(9_007_199_254_740_992).is_err());
        assert!(ensure_javascript_safe_integer(-9_007_199_254_740_992).is_err());
    }

    #[test]
    fn partial_refunds_reduce_net_reports_and_show_the_refund_tender() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE users(id TEXT PRIMARY KEY, full_name TEXT NOT NULL);
             CREATE TABLE sales(id TEXT PRIMARY KEY, store_id TEXT NOT NULL, cashier_id TEXT NOT NULL,
                 receipt_number TEXT NOT NULL, created_at TEXT NOT NULL, subtotal_minor INTEGER NOT NULL,
                 discount_minor INTEGER NOT NULL, tax_minor INTEGER NOT NULL, total_minor INTEGER NOT NULL, status TEXT NOT NULL,
                 station_key TEXT, station_name_snapshot TEXT);
             CREATE TABLE sale_items(sale_id TEXT NOT NULL, quantity INTEGER NOT NULL);
             CREATE TABLE payments(sale_id TEXT NOT NULL, method TEXT NOT NULL, amount_minor INTEGER NOT NULL);
             CREATE TABLE refunds(id TEXT, sale_id TEXT, total_minor INTEGER, store_id TEXT);
             CREATE TABLE refund_items(refund_id TEXT, subtotal_minor INTEGER, discount_minor INTEGER, tax_minor INTEGER);
             CREATE TABLE refund_tenders(refund_id TEXT, method TEXT, amount_minor INTEGER);
             INSERT INTO users VALUES('cashier','Demo Cashier');
             INSERT INTO sales(id,store_id,cashier_id,receipt_number,created_at,subtotal_minor,discount_minor,tax_minor,total_minor,status) VALUES('sale-a','store-a','cashier','R-1','2026-10-29T12:00:00Z',900,0,100,1000,'partially_refunded');
             INSERT INTO sale_items VALUES('sale-a',2);
             INSERT INTO payments VALUES('sale-a','card',1000);
             INSERT INTO refunds VALUES('refund-a','sale-a',300,'store-a');
             INSERT INTO refund_items VALUES('refund-a',270,0,30);
             INSERT INTO refund_tenders VALUES('refund-a','cash',300);",
        ).unwrap();
        let report = run_query(
            &connection,
            "store-a",
            &SalesQueryInput {
                from_utc: None,
                to_utc_exclusive: None,
                search: None,
                station_key: None,
                offset: 0,
                page_size: 20,
                snapshot_row_id: None,
            },
        )
        .unwrap();
        assert_eq!(report.completed_count, 1);
        assert_eq!(report.completed_subtotal_minor, 630);
        assert_eq!(report.completed_tax_minor, 70);
        assert_eq!(report.completed_total_minor, 700);
        assert_eq!(report.payment_summary.len(), 2);
        assert_eq!(report.payment_summary[0].method, "card");
        assert_eq!(report.payment_summary[0].total_minor, 1_000);
        assert_eq!(report.payment_summary[1].method, "cash");
        assert_eq!(report.payment_summary[1].total_minor, -300);
    }

    #[test]
    fn profit_report_restores_returned_cost_and_withholds_unknown_profit() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE sales(id TEXT PRIMARY KEY, store_id TEXT, subtotal_minor INTEGER, discount_minor INTEGER, tax_minor INTEGER, total_minor INTEGER, status TEXT, created_at TEXT, station_key TEXT);
             CREATE TABLE sale_items(id TEXT PRIMARY KEY, sale_id TEXT, quantity INTEGER, unit_cost_minor_snapshot INTEGER);
             CREATE TABLE refunds(id TEXT PRIMARY KEY, sale_id TEXT, store_id TEXT, total_minor INTEGER, created_at TEXT);
             CREATE TABLE refund_items(refund_id TEXT, sale_item_id TEXT, quantity INTEGER, subtotal_minor INTEGER, discount_minor INTEGER, tax_minor INTEGER, amount_minor INTEGER);
             CREATE TABLE expenses(id TEXT, store_id TEXT, amount_minor INTEGER, occurred_at TEXT, cost_center_kind TEXT, cost_center_station_key TEXT, cost_center_station_name_snapshot TEXT);
             CREATE TABLE expense_reimbursements(id TEXT, expense_id TEXT, store_id TEXT, amount_minor INTEGER, returned_at TEXT);
             CREATE TABLE register_stations(store_id TEXT, station_key TEXT);
             CREATE TABLE expense_cost_center_reclassifications(id TEXT,store_id TEXT,expense_id TEXT,revision INTEGER,next_kind TEXT,next_station_key TEXT,next_station_name_snapshot TEXT);
             INSERT INTO register_stations VALUES('store-a','station-a');
             INSERT INTO sales VALUES('sale-a','store-a',1000,100,90,990,'partially_refunded','2026-10-29T12:00:00.000Z','station-a');
             INSERT INTO sale_items VALUES('line-a','sale-a',2,300);
             INSERT INTO refunds VALUES('refund-a','sale-a','store-a',495,'2026-10-29T12:30:00.000Z');
             INSERT INTO refund_items VALUES('refund-a','line-a',1,500,50,45,495);
             INSERT INTO expenses VALUES('expense-a','store-a',50,'2026-10-29T13:00:00.000Z','station_direct','station-a','Main');
             INSERT INTO expense_reimbursements VALUES('return-a','expense-a','store-a',10,'2026-10-30T14:00:00.000Z');
             INSERT INTO sales VALUES('legacy-sale','store-a',200,0,0,200,'completed','2026-10-29T15:00:00.000Z',NULL);
             INSERT INTO sale_items VALUES('line-legacy-sale','legacy-sale',1,100);
             INSERT INTO expenses VALUES('overhead-a','store-a',30,'2026-10-29T14:00:00.000Z','store_overhead',NULL,NULL);
             INSERT INTO expenses VALUES('legacy-expense','store-a',20,'2026-10-29T14:30:00.000Z','legacy_unassigned',NULL,NULL);
             INSERT INTO expenses VALUES('expense-b','store-b',30000,'2026-10-29T13:00:00.000Z','legacy_unassigned',NULL,NULL);
             INSERT INTO sales VALUES('other-store-sale','store-b',50000,0,0,50000,'completed','2026-10-29T12:00:00.000Z',NULL);",
        )
        .unwrap();
        let input = ProfitReportInput {
            from_utc: "2026-10-29T00:00:00Z".into(),
            to_utc_exclusive: "2026-10-30T00:00:00Z".into(),
            station_key: None,
        };
        let complete = run_profit_report(&connection, "store-a", &input).unwrap();
        assert_eq!(complete.sale_count, 2);
        assert_eq!(complete.net_revenue_before_tax_minor, 650);
        assert_eq!(complete.tax_collected_minor, 45);
        assert_eq!(complete.customer_total_minor, 695);
        assert_eq!(complete.cost_of_goods_minor, 400);
        assert_eq!(complete.costed_units, 4);
        assert_eq!(complete.uncosted_units, 0);
        assert_eq!(complete.operating_expenses_minor, 100);
        assert_eq!(complete.direct_expenses_minor, 50);
        assert_eq!(complete.store_overhead_expenses_minor, 30);
        assert_eq!(complete.legacy_unassigned_expenses_minor, 20);
        assert_eq!(complete.unassigned_sales_count, 1);
        assert_eq!(complete.unassigned_sales_revenue_minor, 200);
        assert_eq!(complete.gross_profit_minor, Some(250));
        assert_eq!(complete.net_operating_profit_minor, Some(150));

        let station = run_profit_report(
            &connection,
            "store-a",
            &ProfitReportInput {
                from_utc: "2026-10-29T00:00:00Z".into(),
                to_utc_exclusive: "2026-10-30T00:00:00Z".into(),
                station_key: Some("station-a".into()),
            },
        )
        .unwrap();
        assert_eq!(station.net_revenue_before_tax_minor, 450);
        assert_eq!(station.cost_of_goods_minor, 300);
        assert_eq!(station.operating_expenses_minor, 50);
        assert_eq!(station.station_contribution_minor, Some(100));
        assert_eq!(station.net_operating_profit_minor, None);
        assert_eq!(station.store_overhead_expenses_minor, 30);
        assert_eq!(station.legacy_unassigned_expenses_minor, 20);

        connection
            .execute(
                "INSERT INTO expense_cost_center_reclassifications
                 VALUES('reclass-a','store-a','expense-a',1,'store_overhead',NULL,NULL)",
                [],
            )
            .unwrap();
        let corrected_store = run_profit_report(&connection, "store-a", &input).unwrap();
        assert_eq!(corrected_store.operating_expenses_minor, 100);
        assert_eq!(corrected_store.direct_expenses_minor, 0);
        assert_eq!(corrected_store.store_overhead_expenses_minor, 80);
        assert_eq!(corrected_store.net_operating_profit_minor, Some(150));
        let corrected_station = run_profit_report(
            &connection,
            "store-a",
            &ProfitReportInput {
                from_utc: "2026-10-29T00:00:00Z".into(),
                to_utc_exclusive: "2026-10-30T00:00:00Z".into(),
                station_key: Some("station-a".into()),
            },
        )
        .unwrap();
        assert_eq!(corrected_station.station_contribution_minor, Some(150));

        let return_day = run_profit_report(
            &connection,
            "store-a",
            &ProfitReportInput {
                from_utc: "2026-10-30T00:00:00Z".into(),
                to_utc_exclusive: "2026-10-31T00:00:00Z".into(),
                station_key: None,
            },
        )
        .unwrap();
        assert_eq!(return_day.operating_expenses_minor, -10);
        assert_eq!(return_day.direct_expenses_minor, 0);
        assert_eq!(return_day.store_overhead_expenses_minor, -10);

        connection
            .execute(
                "UPDATE refunds SET created_at='2026-10-30T09:00:00.000Z' WHERE id='refund-a'",
                [],
            )
            .unwrap();
        let refund_day = run_profit_report(
            &connection,
            "store-a",
            &ProfitReportInput {
                from_utc: "2026-10-30T00:00:00Z".into(),
                to_utc_exclusive: "2026-10-31T00:00:00Z".into(),
                station_key: None,
            },
        )
        .unwrap();
        assert_eq!(refund_day.net_revenue_before_tax_minor, -450);
        assert_eq!(refund_day.tax_collected_minor, -45);
        assert_eq!(refund_day.customer_total_minor, -495);
        assert_eq!(refund_day.cost_of_goods_minor, -300);
        assert_eq!(refund_day.gross_profit_minor, Some(-150));

        connection
            .execute(
                "INSERT INTO sale_items VALUES('line-legacy','sale-a',1,NULL)",
                [],
            )
            .unwrap();
        let incomplete = run_profit_report(&connection, "store-a", &input).unwrap();
        assert_eq!(incomplete.uncosted_units, 1);
        assert_eq!(incomplete.gross_profit_minor, None);
        assert_eq!(incomplete.net_operating_profit_minor, None);
    }
}
