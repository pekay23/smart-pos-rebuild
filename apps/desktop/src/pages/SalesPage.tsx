// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState } from "react";
import { Download, Search, ShoppingBag } from "lucide-react";
import { RefundSaleDialog } from "../components/RefundSaleDialog";
import { posApi } from "../lib/api";
import type { SalesQueryResult, SessionUser, StationWorkspace } from "../lib/models";
import { formatDateTime, formatMoney } from "../lib/money";
import { downloadSalesCsv } from "../lib/salesExport";

const pageSize = 50;

export function SalesPage({ user }: { user: SessionUser }) {
  const [query, setQuery] = useState("");
  const [stationKey, setStationKey] = useState("");
  const [stations, setStations] = useState<StationWorkspace["stations"]>([]);
  const [offset, setOffset] = useState(0);
  const [loaded, setLoaded] = useState<{ key: string; result: SalesQueryResult } | null>(null);
  const [loadError, setLoadError] = useState<{ key: string; message: string } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState("");
  const [selectedSaleId, setSelectedSaleId] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const snapshot = useRef<number | null>(null);
  const normalizedSearch = query.trim();
  const requestKey = `${normalizedSearch}\u0000${stationKey}\u0000${offset}\u0000${refreshKey}`;
  const result = loaded?.key === requestKey ? loaded.result : null;
  const error = loadError?.key === requestKey ? loadError.message : "";

  useEffect(() => {
    let active = true;
    void posApi.stationWorkspace().then((workspace) => {
      if (active) setStations(workspace.stations);
    }).catch(() => { if (active) setStations([]); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    void posApi.querySales({ search: normalizedSearch, stationKey: stationKey || undefined, offset, pageSize, snapshotRowId: offset ? snapshot.current ?? undefined : undefined })
      .then((response) => {
        if (!active) return;
        snapshot.current = response.snapshotRowId;
        setLoaded({ key: requestKey, result: response });
      })
      .catch(() => { if (active) setLoadError({ key: requestKey, message: "Could not load sales history." }); });
    return () => { active = false; };
  }, [normalizedSearch, offset, requestKey, refreshKey, stationKey]);

  async function exportCsv() {
    setExporting(true);
    setExportError("");
    try {
      await downloadSalesCsv({ search: normalizedSearch, stationKey: stationKey || undefined }, user.currencyCode, `advanced-pos-sales-${new Date().toISOString().slice(0, 10)}.csv`);
    } catch {
      setExportError("Sales could not be exported. Try again.");
    } finally {
      setExporting(false);
    }
  }

  const rows = result?.rows ?? [];
  const firstRow = result && rows.length ? result.offset + 1 : 0;
  const lastRow = result ? result.offset + rows.length : 0;
  const hasPrevious = Boolean(result && result.offset > 0);
  const hasNext = Boolean(result && result.offset + rows.length < result.matchedCount);

  return <section className="sales-page">
    <div className="page-heading"><div><p className="eyebrow">TRANSACTION RECORDS</p><h1>Sales history</h1><p className="page-subtitle">Search and review every transaction recorded in this store.</p></div><button className="button button-secondary" onClick={() => void exportCsv()} disabled={!result?.matchedCount || exporting}><Download size={17}/>{exporting ? "Preparing CSV…" : "Export all results"}</button></div>
    <div className="sales-summary"><div><small>Matching transactions</small><strong>{result?.matchedCount ?? "—"}</strong></div><div><small>Completed sales total</small><strong>{result ? formatMoney(result.completedTotalMinor, user.currencyCode) : "—"}</strong></div><div><small>Showing</small><strong>{result ? `${firstRow}–${lastRow}` : "—"} <em>transactions</em></strong></div></div>
    <article className="panel sales-table-panel"><div className="table-toolbar"><div><h2>All transactions</h2><p>Newest first · {result?.completedCount ?? "—"} completed</p></div><label className="table-search"><Search size={16}/><input value={query} onChange={(event) => { snapshot.current = null; setQuery(event.target.value); setOffset(0); }} placeholder="Search receipt, cashier, or payment…" aria-label="Search sales"/></label><select className="input-control" aria-label="Filter by register" value={stationKey} onChange={(event) => { snapshot.current = null; setStationKey(event.target.value); setOffset(0); }}><option value="">All registers</option>{stations.map((station) => <option key={station.stationKey} value={station.stationKey}>{station.displayName}{station.isActive ? "" : " (inactive)"}</option>)}</select></div><div className="table-scroll"><table><thead><tr><th>RECEIPT</th><th>DATE &amp; TIME</th><th>CASHIER</th><th>REGISTER</th><th>ITEMS</th><th>PAYMENT</th><th>STATUS</th><th className="align-right">TOTAL</th><th>ACTIONS</th></tr></thead><tbody>{rows.map((sale) => <tr key={sale.id}><td><strong className="mono-value">{sale.receiptNumber}</strong></td><td>{formatDateTime(sale.createdAt)}</td><td>{sale.cashierName}</td><td>{sale.stationNameSnapshot}</td><td>{sale.itemCount}</td><td><span className="payment-method-mark">{sale.paymentMethod.replaceAll("_", " ")}</span></td><td><span className={`status-pill ${sale.status === "completed" ? "status-active" : "status-inactive"}`}><i/>{sale.status.replaceAll("_", " ")}</span></td><td className="align-right"><strong>{formatMoney(sale.totalMinor, user.currencyCode)}</strong></td><td><button type="button" className="button button-secondary button-small" onClick={() => setSelectedSaleId(sale.id)}>Review</button></td></tr>)}{!rows.length && <tr><td colSpan={9}><div className="table-empty"><span className="empty-icon"><ShoppingBag size={23}/></span><strong>{error || (result ? (normalizedSearch ? "No matching transactions" : "No sales recorded yet") : "Loading sales…")}</strong><span>{error ? "Check the local store and try again." : result ? (normalizedSearch ? "Try a different receipt number, cashier, or payment method." : "Once you complete a checkout, the receipt and transaction summary will appear here.") : ""}</span></div></td></tr>}</tbody></table></div><div className="sales-pagination"><span>{result ? `${firstRow}–${lastRow} of ${result.matchedCount}` : error || "Loading sales…"}</span><div><button className="button button-secondary button-small" type="button" onClick={() => setOffset(Math.max(0, offset - pageSize))} disabled={!hasPrevious}>Previous</button><button className="button button-secondary button-small" type="button" onClick={() => setOffset(offset + pageSize)} disabled={!hasNext}>Next</button></div></div></article>
    {exportError && <p className="form-error" role="alert">{exportError}</p>}
    {selectedSaleId && <RefundSaleDialog saleId={selectedSaleId} user={user} onClose={() => setSelectedSaleId(null)} onUpdated={() => setRefreshKey((value) => value + 1)}/>}
    <footer className="page-footer"><span>SALES <b>·</b> Store-scoped local history</span><span>CSV export includes every matching transaction</span></footer>
  </section>;
}
