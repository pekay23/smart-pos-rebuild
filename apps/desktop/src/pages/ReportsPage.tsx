// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState } from "react";
import { BarChart3, CalendarDays, Download, ReceiptText } from "lucide-react";
import { posApi } from "../lib/api";
import type { ProfitReport, SalesQueryResult, SessionUser, StationWorkspace } from "../lib/models";
import { formatDateTime, formatMoney } from "../lib/money";
import { downloadSalesCsv } from "../lib/salesExport";
import { downloadProfitLossCsv } from "../lib/profitLossExport";

const pageSize = 50;

function dateInputValue(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function initialDateRange() {
  const today = new Date();
  const weekStart = new Date(today);
  weekStart.setDate(today.getDate() - 6);
  return { from: dateInputValue(weekStart), to: dateInputValue(today) };
}

function localMidnightUtc(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day).toISOString();
}

function nextLocalMidnightUtc(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  const next = new Date(year, month - 1, day);
  next.setDate(next.getDate() + 1);
  return next.toISOString();
}

export function ReportsPage({ user }: { user: SessionUser }) {
  const [initialRange] = useState(initialDateRange);
  const [from, setFrom] = useState(initialRange.from);
  const [to, setTo] = useState(initialRange.to);
  const [stationKey, setStationKey] = useState("");
  const [stations, setStations] = useState<StationWorkspace["stations"]>([]);
  const [stationLoadError, setStationLoadError] = useState(false);
  const [offset, setOffset] = useState(0);
  const [loaded, setLoaded] = useState<{ key: string; result: SalesQueryResult; profit: ProfitReport } | null>(null);
  const [loadError, setLoadError] = useState<{ key: string; message: string } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState("");
  const [profitExportError, setProfitExportError] = useState("");
  const snapshot = useRef<{ filterKey: string; rowId: number } | null>(null);
  const filterKey = `${from}\u0000${to}\u0000${stationKey}`;
  const requestKey = `${filterKey}\u0000${offset}`;
  const result = loaded?.key === requestKey ? loaded.result : null;
  const profit = loaded?.key === requestKey ? loaded.profit : null;
  const error = loadError?.key === requestKey ? loadError.message : "";
  const validRange = Boolean(from && to && from <= to);
  const selectedStation = stations.find((station) => station.stationKey === stationKey);

  useEffect(() => {
    let active = true;
    void posApi.stationWorkspace().then((workspace) => {
      if (active) {
        setStations(workspace.stations);
        setStationLoadError(false);
      }
    }).catch(() => {
      if (active) setStationLoadError(true);
    });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!validRange) return;
    let active = true;
    const fromUtc = localMidnightUtc(from);
    const toUtcExclusive = nextLocalMidnightUtc(to);
    void Promise.all([
      posApi.querySales({
        fromUtc,
        toUtcExclusive,
        stationKey: stationKey || undefined,
        offset,
        pageSize,
        snapshotRowId: offset > 0 && snapshot.current?.filterKey === filterKey ? snapshot.current.rowId : undefined,
      }),
      posApi.queryProfitReport({ fromUtc, toUtcExclusive, stationKey: stationKey || undefined }),
    ]).then(([response, profitResponse]) => {
      if (active) {
        snapshot.current = { filterKey, rowId: response.snapshotRowId };
        setLoaded({ key: requestKey, result: response, profit: profitResponse });
      }
    }).catch(() => {
      if (active) setLoadError({ key: requestKey, message: "Could not load report data for this period." });
    });
    return () => { active = false; };
  }, [filterKey, from, offset, requestKey, stationKey, to, validRange]);

  async function exportCsv() {
    if (!validRange) return;
    setExporting(true);
    setExportError("");
    try {
      await downloadSalesCsv({
        fromUtc: localMidnightUtc(from),
        toUtcExclusive: nextLocalMidnightUtc(to),
        stationKey: stationKey || undefined,
      }, user.currencyCode, `advanced-pos-sales-report-${from}-to-${to}-${stationKey.replace(/[^a-z0-9-]/gi, "") || "all-registers"}.csv`);
    } catch {
      setExportError("The complete sales report could not be exported. Try again.");
    } finally {
      setExporting(false);
    }
  }

  function exportProfitCsv() {
    if (!validRange || !profit) return;
    setProfitExportError("");
    try {
      downloadProfitLossCsv(profit, from, to, user.currencyCode);
    } catch {
      setProfitExportError("The profit and loss summary could not be exported. Try again.");
    }
  }

  const rows = result?.rows ?? [];
  const firstRow = result && rows.length ? result.offset + 1 : 0;
  const lastRow = result ? result.offset + rows.length : 0;
  const hasPrevious = Boolean(result && result.offset > 0);
  const hasNext = Boolean(result && result.offset + rows.length < result.matchedCount);
  const average = result?.completedCount ? Math.round(result.completedTotalMinor / result.completedCount) : 0;

  return <section className="reports-page">
    <div className="page-heading">
      <div><p className="eyebrow">STORE PERFORMANCE</p><h1>Reports</h1><p className="page-subtitle">Sales and contribution figures follow the register filter. Clear the filter for the full store result.</p></div>
      <button className="button button-secondary" onClick={() => void exportCsv()} disabled={!result?.matchedCount || exporting || !validRange}><Download size={17}/>{exporting ? "Preparing sales…" : "Export sales CSV"}</button>
    </div>

    <article className="panel report-filter-panel">
      <div className="report-filter-title"><span className="report-icon"><CalendarDays size={17}/></span><div><strong>Report period</strong><small>Dates use this device’s local time.</small></div></div>
      <label>From<input type="date" value={from} max={to} onChange={(event) => { snapshot.current = null; setFrom(event.target.value); setOffset(0); }}/></label>
      <label>To<input type="date" value={to} min={from} onChange={(event) => { snapshot.current = null; setTo(event.target.value); setOffset(0); }}/></label>
      <label>Register<select className="report-station-select" value={stationKey} onChange={(event) => { snapshot.current = null; setStationKey(event.target.value); setOffset(0); }}><option value="">All registers</option>{stations.map((station) => <option key={station.stationKey} value={station.stationKey}>{station.displayName}{station.isActive ? "" : " (inactive)"}</option>)}</select></label>
      {stationLoadError && <small className="report-station-warning" role="status">Register list unavailable; showing all registers.</small>}
    </article>

    <div className="report-metrics">
      <article className="metric-card metric-primary"><div className="metric-top"><span>Completed sales</span><span className="metric-icon"><ReceiptText size={18}/></span></div><strong>{result?.completedCount ?? "—"}</strong><small>{result?.matchedCount ?? "—"} transactions {selectedStation ? `at ${selectedStation.displayName}` : "across all registers"}</small></article>
      <article className="metric-card"><div className="metric-top"><span>Sales total</span><span className="metric-icon icon-blue"><BarChart3 size={18}/></span></div><strong>{result ? formatMoney(result.completedTotalMinor, user.currencyCode) : "—"}</strong><small>Completed transactions only · {selectedStation?.displayName ?? "all registers"}</small></article>
      <article className="metric-card"><div className="metric-top"><span>Average sale</span><span className="metric-icon icon-amber"><ReceiptText size={18}/></span></div><strong>{result ? formatMoney(average, user.currencyCode) : "—"}</strong><small>Completed sales total divided by count</small></article>
    </div>

    <article className="panel report-breakdown" aria-label="Sales amount breakdown">
      <div><span>{selectedStation ? `${selectedStation.displayName} subtotal` : "Subtotal"}</span><strong>{result ? formatMoney(result.completedSubtotalMinor, user.currencyCode) : "—"}</strong></div>
      <div><span>Discounts</span><strong>{result ? formatMoney(result.completedDiscountMinor, user.currencyCode) : "—"}</strong></div>
      <div><span>Tax</span><strong>{result ? formatMoney(result.completedTaxMinor, user.currencyCode) : "—"}</strong></div>
      <div><span>Net sales</span><strong>{result ? formatMoney(result.completedTotalMinor, user.currencyCode) : "—"}</strong></div>
    </article>

    <article className="panel report-breakdown profit-breakdown" aria-label="Profit and loss summary">
      <div className="profit-report-header"><div><h2>{selectedStation ? "Station contribution" : "Profit and loss"} <small>{selectedStation ? selectedStation.displayName : "Entire store"}</small></h2><small>{selectedStation ? "Before shared overhead and legacy unassigned costs." : "Includes direct costs, store overhead, and legacy unassigned expenses."}</small></div><button type="button" className="button button-secondary button-small" onClick={exportProfitCsv} disabled={!profit || !validRange}><Download size={14}/>Export {selectedStation ? "contribution" : "P&L"} CSV</button></div>
      <div><span>Net revenue before tax</span><strong>{profit ? formatMoney(profit.netRevenueBeforeTaxMinor, user.currencyCode) : "—"}</strong></div>
      <div><span>Net cost of goods sold</span><strong>{profit ? profit.uncostedUnits === 0 ? formatMoney(profit.costOfGoodsMinor, user.currencyCode) : "Incomplete" : "—"}</strong></div>
      <div><span>Gross profit</span><strong>{profit?.grossProfitMinor !== null && profit ? formatMoney(profit.grossProfitMinor, user.currencyCode) : "—"}</strong></div>
      <div><span>{selectedStation ? "Direct station expenses" : "Direct station expenses · all registers"}</span><strong>{profit ? formatMoney(profit.directExpensesMinor, user.currencyCode) : "—"}</strong></div>
      {!selectedStation && <><div><span>Store overhead</span><strong>{profit ? formatMoney(profit.storeOverheadExpensesMinor, user.currencyCode) : "—"}</strong></div><div><span>Legacy unassigned expenses</span><strong>{profit ? formatMoney(profit.legacyUnassignedExpensesMinor, user.currencyCode) : "—"}</strong></div><div><span>All operating expenses</span><strong>{profit ? formatMoney(profit.operatingExpensesMinor, user.currencyCode) : "—"}</strong></div></>}
      <div><span>{selectedStation ? "Contribution before overhead" : "Net operating result"}</span><strong>{selectedStation ? profit?.stationContributionMinor !== null && profit ? formatMoney(profit.stationContributionMinor, user.currencyCode) : "—" : profit?.netOperatingProfitMinor !== null && profit ? formatMoney(profit.netOperatingProfitMinor, user.currencyCode) : "—"}</strong></div>
      {profit && <p className="profit-report-note">Tax collected: {formatMoney(profit.taxCollectedMinor, user.currencyCode)} · {profit.saleCount} sales recorded{profit.unassignedSalesCount ? selectedStation ? ` · ${profit.unassignedSalesCount} legacy unassigned sales (${formatMoney(profit.unassignedSalesRevenueMinor, user.currencyCode)} net revenue) are not included in this station view` : ` · ${profit.unassignedSalesCount} legacy unassigned sales (${formatMoney(profit.unassignedSalesRevenueMinor, user.currencyCode)} net revenue) remain in the unassigned bucket` : ""}{profit.uncostedUnits ? ` · ${profit.uncostedUnits} sold or returned item units lack a sale-time cost, so profit is withheld` : " · all included item costs are available"}. Expenses and reimbursements use their recorded dates.</p>}
      {profitExportError && <p className="profit-report-note profit-export-error" role="alert">{profitExportError}</p>}
    </article>

    <div className="reports-columns">
      <article className="panel report-panel">
        <div className="table-toolbar"><div><h2>Payment methods</h2><p>Completed amounts for the selected period · {selectedStation?.displayName ?? "all registers"}</p></div></div>
        {result?.paymentSummary.length ? <div className="payment-report-list">{result.paymentSummary.map(({ method, saleCount, totalMinor }) => <div className="payment-report-row" key={method}><span className="payment-method-mark">{method.replaceAll("_", " ")}</span><span>{saleCount} sale{saleCount === 1 ? "" : "s"}</span><strong>{formatMoney(totalMinor, user.currencyCode)}</strong><div className="payment-report-track"><i style={{ width: `${result.completedTotalMinor ? Math.max(4, totalMinor / result.completedTotalMinor * 100) : 0}%` }}/></div></div>)}</div> : <p className="report-empty">{result ? "No completed sales in this period." : "Loading payment totals…"}</p>}
      </article>
      <article className="panel report-panel report-note"><span className="report-icon"><BarChart3 size={18}/></span><h2>Sales summary</h2><p>Period totals include every matching local transaction{selectedStation ? ` for ${selectedStation.displayName}` : " across all registers"}. The table shows one page at a time.</p><p className="report-range">{from} <span>→</span> {to}</p></article>
    </div>

    <article className="panel sales-table-panel report-transactions">
      <div className="table-toolbar"><div><h2>Transactions in period</h2><p>{result?.matchedCount ?? "—"} record{result?.matchedCount === 1 ? "" : "s"} · {selectedStation?.displayName ?? "all registers"} · newest first</p></div></div>
      <div className="table-scroll"><table><thead><tr><th>RECEIPT</th><th>DATE &amp; TIME</th><th>CASHIER</th><th>REGISTER</th><th>ITEMS</th><th>PAYMENT</th><th>STATUS</th><th className="align-right">TOTAL</th></tr></thead><tbody>{rows.map((sale) => <tr key={sale.id}><td><strong className="mono-value">{sale.receiptNumber}</strong></td><td>{formatDateTime(sale.createdAt)}</td><td>{sale.cashierName}</td><td>{sale.stationNameSnapshot}</td><td>{sale.itemCount}</td><td><span className="payment-method-mark">{sale.paymentMethod.replaceAll("_", " ")}</span></td><td><span className={`status-pill ${sale.status === "completed" ? "status-active" : "status-inactive"}`}><i/>{sale.status.replaceAll("_", " ")}</span></td><td className="align-right"><strong>{formatMoney(sale.totalMinor, user.currencyCode)}</strong></td></tr>)}{!rows.length && <tr><td colSpan={8}><div className="table-empty"><span className="empty-icon"><ReceiptText size={23}/></span><strong>{!validRange ? "Choose a valid date range" : error || (result ? "No transactions in this period" : "Loading transactions…")}</strong><span>{error ? "Check the local store and try again." : result ? "Try a wider period or complete a sale at the register." : ""}</span></div></td></tr>}</tbody></table></div>
      <div className="sales-pagination"><span>{result ? `${firstRow}–${lastRow} of ${result.matchedCount}` : error || "Loading report…"}</span><div><button className="button button-secondary button-small" type="button" onClick={() => setOffset(Math.max(0, offset - pageSize))} disabled={!hasPrevious}>Previous</button><button className="button button-secondary button-small" type="button" onClick={() => setOffset(offset + pageSize)} disabled={!hasNext}>Next</button></div></div>
    </article>
    {exportError && <p className="form-error" role="alert">{exportError}</p>}
    <footer className="page-footer"><span>REPORTS <b>·</b> Local store transactions</span><span>Sales rows and P&amp;L summary export separately for the selected period</span></footer>
  </section>;
}
